mod chatgpt;
mod codex;
mod pi;

use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};
use thiserror::Error;

pub use chatgpt::parse_chatgpt_export;
pub use codex::parse_codex;
pub use pi::parse_pi;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("could not read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{0}")]
    Invalid(String),
}

pub(super) fn read_jsonl(path: &Path) -> Result<Vec<Value>, ParseError> {
    let file = File::open(path).map_err(|source| ParseError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut records = Vec::new();
    let mut reader = BufReader::new(file);
    let mut line = String::new();

    loop {
        line.clear();
        let count = reader
            .read_line(&mut line)
            .map_err(|source| ParseError::Io {
                path: path.display().to_string(),
                source,
            })?;
        if count == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(line.trim_end()) {
            Ok(record) => records.push(record),
            Err(source) if !line.ends_with('\n') => {
                // A session may be read while its final JSONL record is still being written.
                let _ = source;
                break;
            }
            Err(source) => {
                return Err(ParseError::Json {
                    path: path.display().to_string(),
                    source,
                });
            }
        }
    }

    Ok(records)
}

pub(super) fn string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(super) fn first_line(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Untitled conversation")
        .chars()
        .take(140)
        .collect()
}

pub(super) fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(value)) => value.trim().to_owned(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| {
                block.as_str().map(str::to_owned).or_else(|| {
                    string(block.get("text"))
                        .or_else(|| string(block.get("input_text")))
                        .or_else(|| string(block.get("output_text")))
                })
            })
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

pub(super) fn end_timestamp(started_at: &str, messages: &[crate::Message]) -> String {
    messages
        .iter()
        .rev()
        .find_map(|message| message.created_at.clone())
        .unwrap_or_else(|| started_at.to_owned())
}

pub(super) fn is_injected_user_message(content: &str) -> bool {
    let trimmed = content.trim_start();
    trimmed.starts_with("<skill")
        || trimmed.starts_with("<system-reminder")
        || trimmed.starts_with("<environment_context")
}
