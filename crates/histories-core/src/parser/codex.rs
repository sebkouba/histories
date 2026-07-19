use crate::{Conversation, Message, MessageKind, SourceKind};
use serde_json::Value;
use std::{collections::HashMap, path::Path};

use super::{
    ParseError, content_text, end_timestamp, first_line, is_injected_user_message, push_unique,
    read_jsonl, string,
};

pub fn parse_codex(
    path: &Path,
    session_names: &HashMap<String, String>,
) -> Result<Option<Conversation>, ParseError> {
    let records = read_jsonl(path)?;
    let mut external_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("codex-session")
        .to_owned();
    let mut started_at = String::new();
    let mut cwd = None;
    let mut started_via = None;
    let mut models = Vec::new();
    let mut messages = Vec::new();

    for record in records {
        let record_type = record
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let timestamp = string(record.get("timestamp"));
        let payload = record.get("payload").unwrap_or(&Value::Null);

        match record_type {
            "session_meta" => {
                external_id = string(payload.get("id"))
                    .or_else(|| string(payload.get("session_id")))
                    .unwrap_or(external_id);
                started_at = string(payload.get("timestamp"))
                    .or(timestamp)
                    .unwrap_or(started_at);
                cwd = string(payload.get("cwd")).or(cwd);
                started_via = codex_started_via(payload).or(started_via);
            }
            "turn_context" => push_unique(&mut models, string(payload.get("model"))),
            "event_msg" if payload.get("type").and_then(Value::as_str) == Some("user_message") => {
                if let Some(content) = string(payload.get("message")) {
                    let kind = if is_injected_user_message(&content) {
                        MessageKind::System
                    } else {
                        MessageKind::User
                    };
                    push_message(&mut messages, kind, None, content, timestamp);
                }
            }
            "response_item" => match payload.get("type").and_then(Value::as_str) {
                Some("message") => {
                    let role = payload
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    // Visible user turns come from event_msg. response_item user records also
                    // contain injected context, so keeping both would duplicate the transcript.
                    if role == "user" {
                        continue;
                    }
                    let content = content_text(payload.get("content"));
                    if content.is_empty() {
                        continue;
                    }
                    let kind = match role {
                        "assistant" => MessageKind::Assistant,
                        "developer" | "system" => MessageKind::System,
                        _ => MessageKind::Other,
                    };
                    push_message(
                        &mut messages,
                        kind,
                        (!role.is_empty()).then(|| role.to_owned()),
                        content,
                        timestamp,
                    );
                }
                Some("reasoning") => {
                    let content = payload
                        .get("summary")
                        .and_then(Value::as_array)
                        .map(|parts| {
                            parts
                                .iter()
                                .filter_map(|part| string(part.get("text")))
                                .collect::<Vec<_>>()
                                .join("\n\n")
                        })
                        .unwrap_or_default();
                    if !content.is_empty() {
                        push_message(
                            &mut messages,
                            MessageKind::Reasoning,
                            None,
                            content,
                            timestamp,
                        );
                    }
                }
                Some("custom_tool_call") | Some("function_call") => {
                    let name = string(payload.get("name"));
                    let content = string(payload.get("input"))
                        .or_else(|| string(payload.get("arguments")))
                        .unwrap_or_default();
                    if !content.is_empty() || name.is_some() {
                        push_message(&mut messages, MessageKind::Tool, name, content, timestamp);
                    }
                }
                Some("custom_tool_call_output") | Some("function_call_output") => {
                    let content = string(payload.get("output"))
                        .or_else(|| string(payload.get("content")))
                        .unwrap_or_default();
                    if !content.is_empty() {
                        push_message(
                            &mut messages,
                            MessageKind::Tool,
                            Some("tool result".to_owned()),
                            content,
                            timestamp,
                        );
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    if !messages
        .iter()
        .any(|message| matches!(message.kind, MessageKind::User | MessageKind::Assistant))
    {
        return Ok(None);
    }

    if started_at.is_empty() {
        started_at = messages
            .iter()
            .find_map(|message| message.created_at.clone())
            .unwrap_or_default();
    }
    let title = session_names
        .get(&external_id)
        .cloned()
        .or_else(|| {
            messages
                .iter()
                .find(|message| message.kind == MessageKind::User)
                .map(|message| first_line(&message.content))
        })
        .unwrap_or_else(|| "Codex conversation".to_owned());
    let updated_at = end_timestamp(&started_at, &messages);

    Ok(Some(Conversation {
        source: SourceKind::Codex,
        external_id,
        source_path: path.display().to_string(),
        title,
        cwd,
        started_via,
        models,
        started_at,
        updated_at,
        messages,
    }))
}

fn codex_started_via(payload: &Value) -> Option<String> {
    let source = payload.get("source");
    if source.and_then(|value| value.get("subagent")).is_some() {
        return Some("Codex subagent".to_owned());
    }

    let originator = string(payload.get("originator"));
    match originator.as_deref() {
        Some("codex-tui") => Some("codex".to_owned()),
        Some("Codex Desktop" | "codex_work_desktop") => Some("Codex Desktop".to_owned()),
        Some(originator) => Some(originator.to_owned()),
        None => match source.and_then(Value::as_str) {
            Some("cli") => Some("codex".to_owned()),
            Some("vscode") => Some("Codex Desktop".to_owned()),
            _ => None,
        },
    }
}

fn push_message(
    messages: &mut Vec<Message>,
    kind: MessageKind,
    author: Option<String>,
    content: String,
    created_at: Option<String>,
) {
    messages.push(Message {
        sequence: messages.len(),
        kind,
        author,
        content,
        created_at,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn keeps_one_copy_of_visible_codex_turns_and_preserves_tools() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let records = [
            serde_json::json!({"type":"session_meta","timestamp":"2026-07-01T10:00:00Z","payload":{"id":"session-1","cwd":"/tmp/project","originator":"codex-tui","source":"cli"}}),
            serde_json::json!({"type":"turn_context","timestamp":"2026-07-01T10:00:00Z","payload":{"model":"gpt-5.6-sol"}}),
            serde_json::json!({"type":"event_msg","timestamp":"2026-07-01T10:00:01Z","payload":{"type":"user_message","message":"Investigate this"}}),
            serde_json::json!({"type":"event_msg","timestamp":"2026-07-01T10:00:02Z","payload":{"type":"agent_message","message":"duplicate answer"}}),
            serde_json::json!({"type":"response_item","timestamp":"2026-07-01T10:00:02Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"The answer"}]}}),
            serde_json::json!({"type":"response_item","timestamp":"2026-07-01T10:00:03Z","payload":{"type":"custom_tool_call","name":"exec","input":"{\"cmd\":\"pwd\"}"}}),
        ];
        for record in records {
            writeln!(file, "{record}").unwrap();
        }

        let conversation = parse_codex(file.path(), &HashMap::new()).unwrap().unwrap();
        assert_eq!(conversation.title, "Investigate this");
        assert_eq!(conversation.started_via.as_deref(), Some("codex"));
        assert_eq!(conversation.models, ["gpt-5.6-sol"]);
        assert_eq!(conversation.clean_messages().count(), 2);
        assert_eq!(conversation.messages.len(), 3);
        assert!(
            !conversation
                .messages
                .iter()
                .any(|message| message.content == "duplicate answer")
        );
    }
}
