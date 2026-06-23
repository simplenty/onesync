use std::{
    env,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn config_root() -> PathBuf {
    if let Ok(path) = env::var("XDG_CONFIG_HOME") {
        return Path::new(&path).join("onesync");
    }
    home_dir().join(".config").join("onesync")
}

pub fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(path)
}

fn home_dir() -> PathBuf {
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'').trim()
}

pub struct SyncPath<'a> {
    pub parent: Option<&'a str>,
    pub name: &'a str,
}

pub fn sync_path(path: &str) -> SyncPath<'_> {
    let normalized = path.trim_start_matches("./").trim_matches('/');
    match normalized.rsplit_once('/') {
        Some((parent, name)) => SyncPath {
            parent: Some(parent),
            name,
        },
        None => SyncPath {
            parent: None,
            name: normalized,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_path_splits_nested_path_into_parent_and_name() {
        let s = sync_path("docs/archive/a.txt");
        assert_eq!(s.parent, Some("docs/archive"));
        assert_eq!(s.name, "a.txt");
    }

    #[test]
    fn sync_path_returns_none_parent_for_bare_name() {
        let s = sync_path("a.txt");
        assert_eq!(s.parent, None);
        assert_eq!(s.name, "a.txt");
    }
}
