use crate::{Conversation, Message, MessageKind, SourceKind};
use chrono::{DateTime, Utc};
use serde::Deserializer as _;
use serde::de::{SeqAccess, Visitor};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fmt,
    fs::File,
    io::BufReader,
    path::Path,
};

use super::{ParseError, content_text, end_timestamp, first_line, push_unique, string};

pub fn parse_chatgpt_export(path: &Path) -> Result<Vec<Conversation>, ParseError> {
    let file = File::open(path).map_err(|source| ParseError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(file));
    let (entry_count, conversations) = deserializer
        .deserialize_seq(ChatgptExportVisitor { path })
        .map_err(|source| ParseError::Json {
            path: path.display().to_string(),
            source,
        })?;
    if entry_count != conversations.len() {
        return Err(ParseError::Invalid(format!(
            "parsed {} of {entry_count} ChatGPT conversations from {}; the export format may have changed",
            conversations.len(),
            path.display()
        )));
    }
    Ok(conversations)
}

struct ChatgptExportVisitor<'a> {
    path: &'a Path,
}

impl<'de> Visitor<'de> for ChatgptExportVisitor<'_> {
    type Value = (usize, Vec<Conversation>);

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a ChatGPT conversations.json array")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut entry_count = 0;
        let mut conversations = Vec::new();
        while let Some(entry) = sequence.next_element::<Value>()? {
            entry_count += 1;
            if let Some(conversation) = parse_conversation(self.path, &entry) {
                conversations.push(conversation);
            }
        }
        Ok((entry_count, conversations))
    }
}

fn parse_conversation(path: &Path, entry: &Value) -> Option<Conversation> {
    let mapping = entry.get("mapping")?.as_object()?;
    let nodes = mapping
        .iter()
        .map(|(id, value)| (id.clone(), value))
        .collect::<HashMap<_, _>>();
    let current_node = string(entry.get("current_node")).or_else(|| {
        nodes
            .iter()
            .filter(|(_, node)| {
                node.get("children")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty)
            })
            .max_by_key(|(_, node)| message_timestamp(node).unwrap_or_default())
            .map(|(id, _)| id.clone())
    })?;

    let mut path_ids = Vec::new();
    let mut current = current_node;
    let mut visited = HashSet::new();
    while visited.insert(current.clone()) {
        let Some(node) = nodes.get(&current) else {
            break;
        };
        path_ids.push(current.clone());
        let Some(parent) = string(node.get("parent")) else {
            break;
        };
        current = parent;
    }
    path_ids.reverse();

    let mut messages = Vec::new();
    let mut models = Vec::new();
    for id in path_ids {
        let node = nodes.get(&id)?;
        let Some(message) = node.get("message").filter(|message| !message.is_null()) else {
            continue;
        };
        let role = message
            .get("author")
            .and_then(|author| author.get("role"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let content = chatgpt_content(message.get("content"));
        if content.is_empty() {
            continue;
        }
        let kind = match role {
            "user" => MessageKind::User,
            "assistant" => MessageKind::Assistant,
            "tool" => MessageKind::Tool,
            "system" => MessageKind::System,
            _ => MessageKind::Other,
        };
        if role == "assistant" {
            push_unique(
                &mut models,
                string(
                    message
                        .get("metadata")
                        .and_then(|metadata| metadata.get("model_slug")),
                ),
            );
        }
        messages.push(Message {
            sequence: messages.len(),
            kind,
            author: string(message.get("author").and_then(|author| author.get("name"))),
            content,
            created_at: timestamp(message.get("create_time")),
        });
    }

    if !messages
        .iter()
        .any(|message| matches!(message.kind, MessageKind::User | MessageKind::Assistant))
    {
        return None;
    }

    let external_id = string(entry.get("conversation_id"))
        .or_else(|| string(entry.get("id")))
        .unwrap_or_else(|| current.clone());
    let started_at = timestamp(entry.get("create_time"))
        .or_else(|| {
            messages
                .iter()
                .find_map(|message| message.created_at.clone())
        })
        .unwrap_or_default();
    let updated_at = timestamp(entry.get("update_time"))
        .unwrap_or_else(|| end_timestamp(&started_at, &messages));
    let title = string(entry.get("title"))
        .or_else(|| {
            messages
                .iter()
                .find(|message| message.kind == MessageKind::User)
                .map(|message| first_line(&message.content))
        })
        .unwrap_or_else(|| "ChatGPT conversation".to_owned());
    if models.is_empty() {
        push_unique(&mut models, string(entry.get("default_model_slug")));
    }

    Some(Conversation {
        source: SourceKind::Chatgpt,
        external_id,
        source_path: path.display().to_string(),
        title,
        cwd: None,
        started_via: None,
        models,
        started_at,
        updated_at,
        messages,
    })
}

fn message_timestamp(node: &Value) -> Option<String> {
    timestamp(
        node.get("message")
            .and_then(|message| message.get("create_time")),
    )
}

fn timestamp(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Number(value)) => value.as_f64().and_then(|epoch| {
            let seconds = epoch.trunc() as i64;
            let nanos = (epoch.fract() * 1_000_000_000.0) as u32;
            DateTime::<Utc>::from_timestamp(seconds, nanos).map(|date| date.to_rfc3339())
        }),
        _ => None,
    }
}

fn chatgpt_content(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    content_text(value.get("parts").or_else(|| value.get("text")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_the_current_chatgpt_branch() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let export = serde_json::json!([{
            "id":"chat-1","title":"A useful chat","create_time":1782896400.0,"update_time":1782896460.0,"current_node":"a-new",
            "mapping":{
                "root":{"id":"root","message":null,"parent":null,"children":["u1"]},
                "u1":{"id":"u1","message":{"author":{"role":"user"},"create_time":1782896401.0,"content":{"content_type":"text","parts":["Question"]}},"parent":"root","children":["a-old","a-new"]},
                "a-old":{"id":"a-old","message":{"author":{"role":"assistant"},"create_time":1782896402.0,"content":{"content_type":"text","parts":["Old answer"]}},"parent":"u1","children":[]},
                "a-new":{"id":"a-new","message":{"author":{"role":"assistant"},"create_time":1782896403.0,"content":{"content_type":"text","parts":["New answer"]},"metadata":{"model_slug":"gpt-5.6"}},"parent":"u1","children":[]}
            }
        }]);
        write!(file, "{export}").unwrap();

        let conversations = parse_chatgpt_export(file.path()).unwrap();
        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].title, "A useful chat");
        assert_eq!(conversations[0].models, ["gpt-5.6"]);
        assert_eq!(
            conversations[0]
                .clean_messages()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            ["Question", "New answer"]
        );
    }

    #[test]
    fn rejects_exports_with_unrecognized_conversation_entries() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, "[{{\"unexpected\":true}}]").unwrap();

        let error = parse_chatgpt_export(file.path()).unwrap_err();
        assert!(error.to_string().contains("export format may have changed"));
    }
}
