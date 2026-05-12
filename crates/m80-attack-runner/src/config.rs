//! Fixed in-jail configuration file for env-cleared jailer launches.

use std::path::Path;

/// Absolute path inside the jail where the harness may bind a config file.
pub(crate) const CONFIG_PATH: &str = "/m80-attack-runner.conf";

pub(crate) fn value(key: &str, env: &str, default: &str) -> String {
    std::env::var(env)
        .ok()
        .or_else(|| file_value_at(CONFIG_PATH, key))
        .unwrap_or_else(|| default.to_owned())
}

pub(crate) fn optional_value(key: &str, env: &str) -> Option<String> {
    (!env.is_empty())
        .then(|| std::env::var(env).ok())
        .flatten()
        .or_else(|| file_value_at(CONFIG_PATH, key))
}

fn file_value_at(path: impl AsRef<Path>, key: &str) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    parse_value(&raw, key)
}

fn parse_value(raw: &str, key: &str) -> Option<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .find_map(|(found, value)| (found.trim() == key).then(|| value.trim().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_value_reads_trimmed_key_value_line() {
        let raw = "\n# comment\npeer_run_dir = /peer/run\n";

        assert_eq!(
            parse_value(raw, "peer_run_dir").as_deref(),
            Some("/peer/run")
        );
    }

    #[test]
    fn parse_value_ignores_unknown_keys() {
        let raw = "peer_sentinel=/peer/sentinel\n";

        assert_eq!(parse_value(raw, "peer_pid"), None);
    }

    #[test]
    fn file_value_reads_host_pid_from_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m80-attack-runner.conf");
        std::fs::write(&path, "host_pid = 123\n").unwrap();

        assert_eq!(file_value_at(&path, "host_pid").as_deref(), Some("123"));
    }
}
