use m80_firecracker::{ExecRequest, PtyRequest};

use super::pty;

pub(super) fn exec_request_for_run(
    program: &str,
    args: &[String],
    env: Option<Vec<(String, String)>>,
    cwd: Option<String>,
    stdin: Option<Vec<u8>>,
) -> ExecRequest {
    ExecRequest {
        program: program.to_owned(),
        args: args.to_vec(),
        env,
        cwd,
        stdin,
        timeout_ms: None,
        streaming: false,
    }
}

pub(super) fn pty_request_for_run(
    program: &str,
    args: &[String],
    env: Option<Vec<(String, String)>>,
    cwd: Option<String>,
) -> PtyRequest {
    PtyRequest {
        program: program.to_owned(),
        args: args.to_vec(),
        cwd,
        env: pty_env_for_request(env, |key| std::env::var(key).ok()),
        timeout_ms: None,
        size: pty::current_pty_size(),
    }
}

fn pty_env_for_request(
    env: Option<Vec<(String, String)>>,
    host_env: impl Fn(&str) -> Option<String>,
) -> Option<Vec<(String, String)>> {
    let mut pairs = env.unwrap_or_default();
    for key in ["TERM", "COLORTERM", "LANG", "LC_ALL", "LC_CTYPE"] {
        if pairs.iter().any(|(name, _)| name == key) {
            continue;
        }
        if let Some(value) = host_env(key) {
            pairs.push((key.to_owned(), value));
        }
    }
    (!pairs.is_empty()).then_some(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_host_env(key: &str) -> Option<String> {
        match key {
            "TERM" => Some("xterm-256color".to_owned()),
            "COLORTERM" => Some("truecolor".to_owned()),
            "LANG" => Some("en_US.UTF-8".to_owned()),
            _ => None,
        }
    }

    #[test]
    fn pty_env_projects_terminal_metadata_when_present() {
        let env = pty_env_for_request(None, fake_host_env).unwrap();

        assert_eq!(
            env,
            vec![
                ("TERM".to_owned(), "xterm-256color".to_owned()),
                ("COLORTERM".to_owned(), "truecolor".to_owned()),
                ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
            ]
        );
    }

    #[test]
    fn pty_env_keeps_explicit_values_over_host_terminal_metadata() {
        let env = pty_env_for_request(
            Some(vec![
                ("TERM".to_owned(), "screen".to_owned()),
                ("EXTRA".to_owned(), "1".to_owned()),
            ]),
            fake_host_env,
        )
        .unwrap();

        assert_eq!(
            env,
            vec![
                ("TERM".to_owned(), "screen".to_owned()),
                ("EXTRA".to_owned(), "1".to_owned()),
                ("COLORTERM".to_owned(), "truecolor".to_owned()),
                ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
            ]
        );
    }

    #[test]
    fn pty_env_stays_none_without_explicit_or_host_values() {
        assert_eq!(pty_env_for_request(None, |_| None), None);
    }
}
