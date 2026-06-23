use crate::utils::{unix_timestamp, unquote};
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
    pub no_remote_delete: bool,
    pub monitor_interval: String,
    pub monitor_fullscan_frequency: String,
}

impl ConfigEdit {
    #[must_use]
    pub fn requires_resync_from(&self, next: &Self) -> bool {
        self.sync_dir != next.sync_dir
            || self.skip_file != next.skip_file
            || self.skip_dir != next.skip_dir
            || normalize_lines(&self.sync_list) != normalize_lines(&next.sync_list)
    }
}

#[derive(Debug, Clone)]
enum ConfigLine {
    Blank,
    Verbatim(String),
    Pair { key: String, values: Vec<String> },
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
                lines.push(ConfigLine::Verbatim(raw.to_string()));
            } else if let Some((key, value)) = trimmed.split_once('=') {
                current_pair = Some((
                    key.trim().to_string(),
                    vec![unquote(value.trim()).to_string()],
                ));
            } else {
                lines.push(ConfigLine::Verbatim(raw.to_string()));
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
        self.set_bool("no_remote_delete", edit.no_remote_delete);
        self.set_optional("monitor_interval", &edit.monitor_interval);
        self.set_optional(
            "monitor_fullscan_frequency",
            &edit.monitor_fullscan_frequency,
        );
    }

    #[must_use]
    pub fn to_edit(&self) -> ConfigEdit {
        ConfigEdit {
            sync_dir: self.single_value("sync_dir").unwrap_or_default(),
            skip_file: self.values("skip_file"),
            skip_dir: self.values("skip_dir"),
            sync_list: self.single_value("sync_list").unwrap_or_default(),
            download_only: self.bool_value("download_only"),
            upload_only: self.bool_value("upload_only"),
            no_remote_delete: self.bool_value("no_remote_delete"),
            monitor_interval: self.single_value("monitor_interval").unwrap_or_default(),
            monitor_fullscan_frequency: self
                .single_value("monitor_fullscan_frequency")
                .unwrap_or_default(),
        }
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

    fn values(&self, key: &str) -> Vec<String> {
        self.lines
            .iter()
            .filter_map(|line| match line {
                ConfigLine::Pair {
                    key: line_key,
                    values,
                } if line_key == key => Some(values),
                _ => None,
            })
            .flat_map(|values| values.iter())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect()
    }

    fn single_value(&self, key: &str) -> Option<String> {
        self.values(key).into_iter().next()
    }

    fn bool_value(&self, key: &str) -> bool {
        self.single_value(key)
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
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
                ConfigLine::Verbatim(comment) => {
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

pub fn read_sync_list(config_dir: impl AsRef<Path>) -> io::Result<String> {
    let path = config_dir.as_ref().join("sync_list");
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(path)
}

pub fn write_sync_list(config_dir: impl AsRef<Path>, content: &str) -> io::Result<()> {
    let config_dir = config_dir.as_ref();
    fs::create_dir_all(config_dir)?;
    let path = config_dir.join("sync_list");
    if content.trim().is_empty() {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    } else {
        fs::write(path, content)
    }
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension(format!("bak-{}", unix_timestamp()))
}

fn normalize_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
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
            monitor_interval: "300".to_string(),
            monitor_fullscan_frequency: "10".to_string(),
            ..ConfigEdit::default()
        };
        config.apply_edit(&edit);
        let rendered = config.to_string();

        assert!(!rendered.contains("download_only"));
        assert!(rendered.contains("delay_inotify_processing"));
        assert!(rendered.contains("rate_limit"));
        assert!(rendered.contains("threads = \"4\""));
        assert!(rendered.contains("connect_timeout = \"10\""));
        assert!(rendered.contains("data_timeout = \"60\""));
        assert!(rendered.contains("dns_timeout = \"15\""));
        assert!(rendered.contains("operation_timeout = \"3600\""));
        assert!(rendered.contains("upload_only = \"true\""));
        assert!(rendered.contains("no_remote_delete = \"true\""));
    }

    #[test]
    fn reads_config_edit_from_existing_config() {
        let config = OneDriveConfig::parse(
            "sync_dir = \"~/WorkDrive\"\n\
skip_file = \"*.tmp\"\n\
\t\"*.bak\"\n\
skip_dir = \"node_modules\"\n\
\t\"target\"\n\
download_only = \"true\"\n\
no_remote_delete = \"true\"\n\
monitor_interval = \"120\"\n\
monitor_fullscan_frequency = \"6\"\n",
        );

        let edit = config.to_edit();

        assert_eq!(edit.sync_dir, "~/WorkDrive");
        assert_eq!(edit.skip_file, vec!["*.tmp", "*.bak"]);
        assert_eq!(edit.skip_dir, vec!["node_modules", "target"]);
        assert!(edit.download_only);
        assert!(!edit.upload_only);
        assert!(edit.no_remote_delete);
        assert_eq!(edit.monitor_interval, "120");
        assert_eq!(edit.monitor_fullscan_frequency, "6");
    }

    #[test]
    fn detects_resync_required_for_scope_changes() {
        let old = ConfigEdit {
            sync_dir: "~/OneDrive".to_string(),
            skip_file: vec!["*.tmp".to_string()],
            skip_dir: vec!["node_modules".to_string()],
            sync_list: "Documents\nPictures".to_string(),
            ..ConfigEdit::default()
        };
        let same = old.clone();
        let changed_direction = ConfigEdit {
            upload_only: true,
            ..old.clone()
        };
        let changed_scope = ConfigEdit {
            skip_dir: vec!["target".to_string()],
            ..old.clone()
        };

        assert!(!old.requires_resync_from(&same));
        assert!(!old.requires_resync_from(&changed_direction));
        assert!(old.requires_resync_from(&changed_scope));
    }

    #[test]
    fn reads_missing_sync_list_as_empty() {
        let root =
            std::env::temp_dir().join(format!("onesync-sync-list-missing-{}", unix_timestamp()));
        fs::create_dir_all(&root).unwrap();

        let content = read_sync_list(&root).unwrap();

        assert_eq!(content, "");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn writes_and_reads_sync_list() {
        let root =
            std::env::temp_dir().join(format!("onesync-sync-list-write-{}", unix_timestamp()));
        fs::create_dir_all(&root).unwrap();

        write_sync_list(&root, "Documents\nPictures/Trips\n").unwrap();
        let content = read_sync_list(&root).unwrap();

        assert_eq!(content, "Documents\nPictures/Trips\n");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn removes_sync_list_file_when_content_is_empty() {
        let root =
            std::env::temp_dir().join(format!("onesync-sync-list-remove-{}", unix_timestamp()));
        fs::create_dir_all(&root).unwrap();
        write_sync_list(&root, "Documents\n").unwrap();

        write_sync_list(&root, "  \n\t\n").unwrap();

        assert!(!root.join("sync_list").exists());
        let _ = fs::remove_dir_all(root);
    }
}
