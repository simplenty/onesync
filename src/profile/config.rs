use crate::utils::unix_timestamp;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigEdit {
    pub sync_dir: String,
    pub skip_file: Vec<String>,
    pub skip_dir: Vec<String>,
    pub sync_list: String,
    pub download_only: bool,
    pub upload_only: bool,
    pub local_first: bool,
    pub no_remote_delete: bool,
    pub dry_run: bool,
    pub delay_inotify_processing: bool,
    pub rate_limit: String,
    pub threads: String,
    pub connect_timeout: String,
    pub data_timeout: String,
    pub dns_timeout: String,
    pub operation_timeout: String,
    pub monitor_interval: String,
    pub monitor_fullscan_frequency: String,
}

#[derive(Debug, Clone)]
enum ConfigLine {
    Blank,
    Comment(String),
    Pair { key: String, values: Vec<String> },
    Raw(String),
}

#[derive(Debug, Clone, Default)]
pub struct OneDriveConfig {
    lines: Vec<ConfigLine>,
}

impl OneDriveConfig {
    #[must_use]
    pub fn parse(content: &str) -> Self {
        let mut lines = Vec::new();
        let mut current_pair: Option<(String, Vec<String>)> = None;

        for raw in content.lines() {
            let trimmed = raw.trim();
            if raw.starts_with(char::is_whitespace)
                && current_pair.is_some()
                && !trimmed.is_empty()
                && !trimmed.starts_with('#')
            {
                if let Some((_, values)) = current_pair.as_mut() {
                    values.push(unquote(trimmed).to_string());
                }
                continue;
            }

            if let Some((key, values)) = current_pair.take() {
                lines.push(ConfigLine::Pair { key, values });
            }

            if trimmed.is_empty() {
                lines.push(ConfigLine::Blank);
            } else if trimmed.starts_with('#') {
                lines.push(ConfigLine::Comment(raw.to_string()));
            } else if let Some((key, value)) = trimmed.split_once('=') {
                current_pair = Some((
                    key.trim().to_string(),
                    vec![unquote(value.trim()).to_string()],
                ));
            } else {
                lines.push(ConfigLine::Raw(raw.to_string()));
            }
        }

        if let Some((key, values)) = current_pair {
            lines.push(ConfigLine::Pair { key, values });
        }

        Self { lines }
    }

    pub fn read(path: impl AsRef<Path>) -> io::Result<Self> {
        let content = fs::read_to_string(path)?;
        Ok(Self::parse(&content))
    }

    pub fn apply_edit(&mut self, edit: &ConfigEdit) {
        self.set_single("sync_dir", &edit.sync_dir);
        self.set_multi("skip_file", &edit.skip_file);
        self.set_multi("skip_dir", &edit.skip_dir);
        self.set_optional("sync_list", &edit.sync_list);
        self.set_bool("download_only", edit.download_only);
        self.set_bool("upload_only", edit.upload_only);
        self.set_bool("local_first", edit.local_first);
        self.set_bool("no_remote_delete", edit.no_remote_delete);
        self.set_bool("dry_run", edit.dry_run);
        self.set_bool("delay_inotify_processing", edit.delay_inotify_processing);
        self.set_optional("rate_limit", &edit.rate_limit);
        self.set_optional("threads", &edit.threads);
        self.set_optional("connect_timeout", &edit.connect_timeout);
        self.set_optional("data_timeout", &edit.data_timeout);
        self.set_optional("dns_timeout", &edit.dns_timeout);
        self.set_optional("operation_timeout", &edit.operation_timeout);
        self.set_optional("monitor_interval", &edit.monitor_interval);
        self.set_optional(
            "monitor_fullscan_frequency",
            &edit.monitor_fullscan_frequency,
        );
    }

    pub fn enable_transfer_metrics(&mut self) {
        self.set_single("display_transfer_metrics", "true");
    }

    pub fn write_with_backup(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if path.exists() {
            let backup = backup_path(path);
            fs::copy(path, backup)?;
        }
        fs::write(path, self.to_string())
    }

    fn set_single(&mut self, key: &str, value: &str) {
        self.set_values(key, &[value.to_string()], false);
    }

    fn set_optional(&mut self, key: &str, value: &str) {
        if value.trim().is_empty() {
            self.remove_key(key);
        } else {
            self.set_single(key, value);
        }
    }

    fn set_bool(&mut self, key: &str, value: bool) {
        if value {
            self.set_single(key, "true");
        } else {
            self.remove_key(key);
        }
    }

    fn set_multi(&mut self, key: &str, values: &[String]) {
        let values: Vec<String> = values
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        if values.is_empty() {
            self.remove_key(key);
        } else {
            self.set_values(key, &values, true);
        }
    }

    fn set_values(&mut self, key: &str, values: &[String], multiline: bool) {
        self.lines.retain(
            |line| !matches!(line, ConfigLine::Pair { key: line_key, .. } if line_key == key),
        );
        let stored = if multiline {
            values.to_vec()
        } else {
            values.first().cloned().into_iter().collect()
        };
        self.lines.push(ConfigLine::Pair {
            key: key.to_string(),
            values: stored,
        });
    }

    fn remove_key(&mut self, key: &str) {
        self.lines.retain(
            |line| !matches!(line, ConfigLine::Pair { key: line_key, .. } if line_key == key),
        );
    }
}

impl std::fmt::Display for OneDriveConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for line in &self.lines {
            match line {
                ConfigLine::Blank => writeln!(formatter)?,
                ConfigLine::Comment(comment) | ConfigLine::Raw(comment) => {
                    writeln!(formatter, "{comment}")?;
                }
                ConfigLine::Pair { key, values } => {
                    if values.len() <= 1 {
                        let value = values.first().map_or("", String::as_str);
                        if !value.trim().is_empty() {
                            writeln!(formatter, "{key} = \"{}\"", escape(value))?;
                        }
                    } else {
                        writeln!(formatter, "{key} = \"{}\"", escape(&values[0]))?;
                        for value in values.iter().skip(1) {
                            writeln!(formatter, "\t\"{}\"", escape(value))?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn ensure_transfer_metrics_enabled(config_dir: impl AsRef<Path>) -> io::Result<()> {
    let config_path = config_dir.as_ref().join("config");
    let mut config = OneDriveConfig::read(&config_path)?;
    config.enable_transfer_metrics();
    config.write_with_backup(config_path)
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension(format!("bak-{}", unix_timestamp()))
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'').trim()
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_multiline_skip_values() {
        let config = OneDriveConfig::parse(
            "sync_dir = \"~/OneDrive\"\nskip_file = \"*.tmp\"\n\t\"*.part\"\nskip_dir = \"node_modules\"\n",
        );

        let rendered = config.to_string();

        assert!(rendered.contains("sync_dir = \"~/OneDrive\""));
        assert!(rendered.contains("skip_file = \"*.tmp\""));
        assert!(rendered.contains("\t\"*.part\""));
        assert!(rendered.contains("skip_dir = \"node_modules\""));
    }

    #[test]
    fn omits_empty_optional_values() {
        let mut config = OneDriveConfig::parse("sync_list = \"old\"\n");
        config.apply_edit(&ConfigEdit::default());

        assert!(!config.to_string().contains("sync_list"));
    }

    #[test]
    fn round_trips_advanced_sync_options() {
        let mut config = OneDriveConfig::parse(
            "sync_dir = \"~/OneDrive\"\ndownload_only = \"true\"\ndelay_inotify_processing = \"true\"\nrate_limit = \"1024\"\nthreads = \"4\"\nconnect_timeout = \"10\"\ndata_timeout = \"60\"\ndns_timeout = \"15\"\noperation_timeout = \"3600\"\nmonitor_interval = \"300\"\nmonitor_fullscan_frequency = \"10\"\n",
        );

        let edit = ConfigEdit {
            sync_dir: "~/OneDrive".to_string(),
            upload_only: true,
            no_remote_delete: true,
            threads: "4".to_string(),
            connect_timeout: "10".to_string(),
            data_timeout: "60".to_string(),
            dns_timeout: "15".to_string(),
            operation_timeout: "3600".to_string(),
            monitor_interval: "300".to_string(),
            monitor_fullscan_frequency: "10".to_string(),
            ..ConfigEdit::default()
        };
        config.apply_edit(&edit);
        let rendered = config.to_string();

        assert!(!rendered.contains("download_only"));
        assert!(!rendered.contains("delay_inotify_processing"));
        assert!(!rendered.contains("rate_limit"));
        assert!(rendered.contains("upload_only = \"true\""));
        assert!(rendered.contains("no_remote_delete = \"true\""));
    }
}
