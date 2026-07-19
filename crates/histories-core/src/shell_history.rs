use chrono::DateTime;
use std::{fs, path::Path};

const MAX_START_DELTA_SECONDS: i64 = 5;

#[derive(Debug, Clone)]
struct HistoryEntry {
    timestamp: i64,
    command: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ShellHistory {
    entries: Vec<HistoryEntry>,
}

impl ShellHistory {
    pub(crate) fn load(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .map(|raw| Self::parse(&String::from_utf8_lossy(&raw)))
            .unwrap_or_default()
    }

    pub(crate) fn pi_command_near(&self, started_at: &str) -> Option<String> {
        let session_timestamp = DateTime::parse_from_rfc3339(started_at).ok()?.timestamp();
        self.entries
            .iter()
            .filter_map(|entry| {
                let delta = session_timestamp - entry.timestamp;
                ((-1..=MAX_START_DELTA_SECONDS).contains(&delta)
                    && is_pi_launch_command(&entry.command))
                .then_some((delta.unsigned_abs(), &entry.command))
            })
            .min_by_key(|(distance, _)| *distance)
            .map(|(_, command)| command.clone())
    }

    fn parse(raw: &str) -> Self {
        let mut entries = Vec::new();
        let mut current: Option<(i64, String)> = None;

        for line in raw.lines() {
            if let Some((timestamp, command)) = parse_entry_start(line) {
                if let Some((timestamp, command)) = current.take() {
                    push_entry(&mut entries, timestamp, command);
                }
                current = Some((timestamp, command.to_owned()));
            } else if let Some((_, command)) = current.as_mut() {
                command.push('\n');
                command.push_str(line);
            }
        }
        if let Some((timestamp, command)) = current {
            push_entry(&mut entries, timestamp, command);
        }

        Self { entries }
    }
}

fn parse_entry_start(line: &str) -> Option<(i64, &str)> {
    let rest = line.strip_prefix(": ")?;
    let (timestamp, rest) = rest.split_once(':')?;
    let (_, command) = rest.split_once(';')?;
    Some((timestamp.parse().ok()?, command))
}

fn push_entry(entries: &mut Vec<HistoryEntry>, timestamp: i64, command: String) {
    let command = normalize_command(&command);
    if !command.is_empty() {
        entries.push(HistoryEntry { timestamp, command });
    }
}

fn normalize_command(command: &str) -> String {
    command
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let line = line
                .strip_suffix("\\\\")
                .or_else(|| line.strip_suffix('\\'))
                .unwrap_or(line)
                .trim_end();
            (!line.is_empty()).then_some(line)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_pi_launch_command(command: &str) -> bool {
    let mut words = command.split_whitespace();
    let executable = words
        .by_ref()
        .find(|word| !is_environment_assignment(word))
        .map(unquote);
    let Some(executable) = executable else {
        return false;
    };
    if Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        != Some("pi")
    {
        return false;
    }

    !matches!(
        words.next().map(unquote),
        Some(
            "config"
                | "install"
                | "remove"
                | "uninstall"
                | "update"
                | "--fork"
                | "--help"
                | "--resume"
                | "--session"
                | "--version"
                | "-h"
                | "-V"
        )
    )
}

fn is_environment_assignment(word: &str) -> bool {
    let word = unquote(word);
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
    })
}

fn unquote(word: &str) -> &str {
    word.strip_prefix('\'')
        .and_then(|word| word.strip_suffix('\''))
        .or_else(|| {
            word.strip_prefix('"')
                .and_then(|word| word.strip_suffix('"'))
        })
        .unwrap_or(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstructs_multiline_pi_commands_near_session_start() {
        let history = ShellHistory::parse(
            r#": 1784463600:0;echo unrelated
: 1784463625:0;pi \\
     --no-context-files \\
     --extension ../.pi/extensions/time-context.ts \\
     --system-prompt "$(cat ifs-seb-2.md)"
"#,
        );

        assert_eq!(
            history
                .pi_command_near("2026-07-19T12:20:26.211Z")
                .as_deref(),
            Some(
                "pi --no-context-files --extension ../.pi/extensions/time-context.ts --system-prompt \"$(cat ifs-seb-2.md)\""
            )
        );
    }

    #[test]
    fn requires_a_tight_match_and_ignores_pi_management_commands() {
        let history = ShellHistory::parse(
            ": 1784463600:0;pi --extension custom.ts\n: 1784463625:0;pi update --extensions\n",
        );

        assert_eq!(history.pi_command_near("2026-07-19T12:20:26.211Z"), None);
        assert_eq!(history.pi_command_near("2026-07-19T12:20:10.211Z"), None);
    }

    #[test]
    fn accepts_quoted_and_environment_prefixed_pi_executables() {
        assert!(is_pi_launch_command("'pi' tech"));
        assert!(is_pi_launch_command(
            "PI_PROVIDER=xai /opt/bin/pi --extension custom.ts"
        ));
        assert!(!is_pi_launch_command("npm update pi"));
    }
}
