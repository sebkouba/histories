use crate::{Conversation, Message, MessageKind, SourceKind};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use super::{ParseError, end_timestamp, first_line, is_injected_user_message, read_jsonl, string};

pub fn parse_pi(path: &Path) -> Result<Option<Conversation>, ParseError> {
    let records = read_jsonl(path)?;
    let mut header = None;
    let mut title = None;
    let mut nodes = HashMap::<String, Value>::new();
    let mut all_ids = HashSet::<String>::new();
    let mut parent_ids = HashSet::<String>::new();

    for record in records {
        match record.get("type").and_then(Value::as_str) {
            Some("session") => header = Some(record.clone()),
            Some("session_info") => title = string(record.get("name")).or(title),
            _ => {}
        }
        if let Some(id) = string(record.get("id")) {
            if let Some(parent_id) = string(record.get("parentId")) {
                parent_ids.insert(parent_id);
            }
            all_ids.insert(id.clone());
            nodes.insert(id, record);
        }
    }

    let Some(header) = header else {
        return Ok(None);
    };
    let active_leaf = all_ids
        .difference(&parent_ids)
        .filter_map(|id| nodes.get(id).map(|node| (id, node)))
        .max_by_key(|(_, node)| string(node.get("timestamp")).unwrap_or_default())
        .map(|(id, _)| id.clone());
    let Some(mut current) = active_leaf else {
        return Ok(None);
    };

    let mut path_ids = Vec::new();
    let mut visited = HashSet::new();
    while visited.insert(current.clone()) {
        let Some(node) = nodes.get(&current) else {
            break;
        };
        path_ids.push(current.clone());
        let Some(parent_id) = string(node.get("parentId")) else {
            break;
        };
        current = parent_id;
    }
    path_ids.reverse();

    let mut messages = Vec::new();
    for id in path_ids {
        let Some(record) = nodes.get(&id) else {
            continue;
        };
        let timestamp = string(record.get("timestamp"));
        match record.get("type").and_then(Value::as_str) {
            Some("message") => parse_message(record, timestamp, &mut messages),
            Some("custom_message") => {
                if let Some(content) = string(record.get("content")) {
                    push_message(
                        &mut messages,
                        MessageKind::Other,
                        string(record.get("customType")),
                        content,
                        timestamp,
                    );
                }
            }
            _ => {}
        }
    }

    if !messages
        .iter()
        .any(|message| matches!(message.kind, MessageKind::User | MessageKind::Assistant))
    {
        return Ok(None);
    }

    let external_id = string(header.get("id")).unwrap_or_else(|| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("pi-session")
            .to_owned()
    });
    let started_at = string(header.get("timestamp"))
        .or_else(|| {
            messages
                .iter()
                .find_map(|message| message.created_at.clone())
        })
        .unwrap_or_default();
    let updated_at = end_timestamp(&started_at, &messages);
    let title = title
        .or_else(|| {
            messages
                .iter()
                .find(|message| message.kind == MessageKind::User)
                .map(|message| first_line(&message.content))
        })
        .unwrap_or_else(|| "Pi conversation".to_owned());

    Ok(Some(Conversation {
        source: SourceKind::Pi,
        external_id,
        source_path: path.display().to_string(),
        title,
        cwd: string(header.get("cwd")),
        started_at,
        updated_at,
        messages,
    }))
}

fn parse_message(record: &Value, timestamp: Option<String>, messages: &mut Vec<Message>) {
    let message = record.get("message").unwrap_or(&Value::Null);
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let timestamp = string(message.get("timestamp")).or(timestamp);
    let content = message.get("content");

    if let Some(text) = content
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        let kind = role_kind(role, text);
        push_message(
            messages,
            kind,
            string(message.get("toolName")),
            text.to_owned(),
            timestamp,
        );
        return;
    }

    for block in content.and_then(Value::as_array).into_iter().flatten() {
        let block_type = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match block_type {
            "text" => {
                if let Some(text) = string(block.get("text")) {
                    push_message(
                        messages,
                        role_kind(role, &text),
                        string(message.get("toolName")),
                        text,
                        timestamp.clone(),
                    );
                }
            }
            "thinking" => {
                if let Some(thinking) = string(block.get("thinking")) {
                    push_message(
                        messages,
                        MessageKind::Reasoning,
                        None,
                        thinking,
                        timestamp.clone(),
                    );
                }
            }
            "toolCall" => {
                let name = string(block.get("name"));
                let content = block
                    .get("arguments")
                    .map(|value| match value {
                        Value::String(value) => value.clone(),
                        _ => serde_json::to_string_pretty(value).unwrap_or_default(),
                    })
                    .unwrap_or_default();
                push_message(
                    messages,
                    MessageKind::Tool,
                    name,
                    content,
                    timestamp.clone(),
                );
            }
            _ => {}
        }
    }
}

fn role_kind(role: &str, content: &str) -> MessageKind {
    match role {
        "user" if is_injected_user_message(content) => MessageKind::System,
        "user" => MessageKind::User,
        "assistant" => MessageKind::Assistant,
        "tool" | "toolResult" => MessageKind::Tool,
        "system" => MessageKind::System,
        _ => MessageKind::Other,
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
    fn follows_the_latest_pi_branch_and_keeps_text_around_tool_calls() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let records = [
            serde_json::json!({"type":"session","id":"pi-1","timestamp":"2026-07-01T10:00:00Z","cwd":"/tmp/pi"}),
            serde_json::json!({"type":"message","id":"u1","parentId":null,"timestamp":"2026-07-01T10:00:01Z","message":{"role":"user","content":[{"type":"text","text":"Help me"}]}}),
            serde_json::json!({"type":"message","id":"a-old","parentId":"u1","timestamp":"2026-07-01T10:00:02Z","message":{"role":"assistant","content":[{"type":"text","text":"old branch"}]}}),
            serde_json::json!({"type":"message","id":"a-new","parentId":"u1","timestamp":"2026-07-01T10:00:03Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"privately"},{"type":"toolCall","name":"bash","arguments":{"cmd":"pwd"}},{"type":"text","text":"new branch"}]}}),
        ];
        for record in records {
            writeln!(file, "{record}").unwrap();
        }

        let conversation = parse_pi(file.path()).unwrap().unwrap();
        let clean = conversation
            .clean_messages()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(clean, ["Help me", "new branch"]);
        assert_eq!(conversation.messages.len(), 4);
        assert!(
            !conversation
                .messages
                .iter()
                .any(|message| message.content == "old branch")
        );
    }
}
