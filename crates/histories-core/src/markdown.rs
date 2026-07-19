use crate::{Conversation, Message, MessageKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportMode {
    Clean,
    Full,
}

impl ExportMode {
    pub fn includes(self, message: &Message) -> bool {
        match self {
            Self::Clean => matches!(message.kind, MessageKind::User | MessageKind::Assistant),
            Self::Full => true,
        }
    }
}

pub fn render_markdown(conversation: &Conversation, mode: ExportMode) -> String {
    let cwd = conversation.cwd.as_deref().unwrap_or("");
    let started_via = conversation.started_via.as_deref().unwrap_or("");
    let command = conversation.start_command.as_deref().unwrap_or("");
    let mut lines = vec![
        "---".to_owned(),
        format!("session_id: {}", yaml_string(&conversation.external_id)),
        format!("app: {}", conversation.source),
        format!("date: {}", yaml_string(&conversation.started_at)),
        format!("updated: {}", yaml_string(&conversation.updated_at)),
        format!("title: {}", yaml_string(&conversation.title)),
        format!("cwd: {}", yaml_string(cwd)),
        format!("started_via: {}", yaml_string(started_via)),
        format!("command: {}", yaml_string(command)),
        format!(
            "models: {}",
            serde_json::to_string(&conversation.models).unwrap_or_else(|_| "[]".to_owned())
        ),
        format!("source_path: {}", yaml_string(&conversation.source_path)),
        format!(
            "mode: {}",
            match mode {
                ExportMode::Clean => "clean",
                ExportMode::Full => "full",
            }
        ),
        "---".to_owned(),
        String::new(),
        format!("# {}", conversation.title),
        String::new(),
        if cwd.is_empty() {
            format!(
                "*{} · {}*",
                conversation.started_at,
                conversation.source.agent_label()
            )
        } else {
            format!(
                "*{} · {} · `{cwd}`*",
                conversation.started_at,
                conversation.source.agent_label()
            )
        },
        String::new(),
    ];

    for message in conversation
        .messages
        .iter()
        .filter(|message| mode.includes(message))
    {
        lines.push(format!("## {}", speaker(conversation, message)));
        lines.push(String::new());
        lines.push(message.content.trim().to_owned());
        lines.push(String::new());
    }

    lines.join("\n").trim_end().to_owned() + "\n"
}

pub fn render_bundle(conversations: &[Conversation], mode: ExportMode) -> String {
    let mut lines = vec![
        "# Conversation history bundle".to_owned(),
        String::new(),
        format!("Sessions: {}", conversations.len()),
        format!(
            "Mode: {}",
            match mode {
                ExportMode::Clean => "clean",
                ExportMode::Full => "full",
            }
        ),
        String::new(),
    ];

    for (index, conversation) in conversations.iter().enumerate() {
        if index > 0 {
            lines.push("---".to_owned());
            lines.push(String::new());
        }
        lines.push(render_markdown(conversation, mode));
    }
    lines.join("\n").trim_end().to_owned() + "\n"
}

fn speaker(conversation: &Conversation, message: &Message) -> String {
    match message.kind {
        MessageKind::User => "User".to_owned(),
        MessageKind::Assistant => conversation.source.agent_label().to_owned(),
        MessageKind::Tool => message
            .author
            .as_deref()
            .map(|author| format!("Tool · {author}"))
            .unwrap_or_else(|| "Tool".to_owned()),
        MessageKind::System => "System".to_owned(),
        MessageKind::Reasoning => "Reasoning".to_owned(),
        MessageKind::Other => message.author.clone().unwrap_or_else(|| "Other".to_owned()),
    }
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourceKind;

    fn fixture() -> Conversation {
        Conversation {
            source: SourceKind::Codex,
            external_id: "abc".to_owned(),
            source_path: "/tmp/abc.jsonl".to_owned(),
            title: "Example".to_owned(),
            cwd: Some("/tmp/project".to_owned()),
            started_via: Some("codex".to_owned()),
            start_command: Some("codex --model gpt-5.6-sol".to_owned()),
            models: vec!["gpt-5.6-sol".to_owned()],
            started_at: "2026-07-01T10:00:00Z".to_owned(),
            updated_at: "2026-07-01T10:01:00Z".to_owned(),
            messages: vec![
                Message {
                    sequence: 0,
                    kind: MessageKind::User,
                    author: None,
                    content: "Question".to_owned(),
                    created_at: None,
                },
                Message {
                    sequence: 1,
                    kind: MessageKind::Tool,
                    author: Some("exec".to_owned()),
                    content: "output".to_owned(),
                    created_at: None,
                },
                Message {
                    sequence: 2,
                    kind: MessageKind::Assistant,
                    author: None,
                    content: "Answer".to_owned(),
                    created_at: None,
                },
            ],
        }
    }

    #[test]
    fn clean_markdown_excludes_tool_payloads() {
        let markdown = render_markdown(&fixture(), ExportMode::Clean);
        assert!(markdown.contains("## User\n\nQuestion"));
        assert!(markdown.contains("## Codex\n\nAnswer"));
        assert!(markdown.contains("started_via: \"codex\""));
        assert!(markdown.contains("command: \"codex --model gpt-5.6-sol\""));
        assert!(markdown.contains("models: [\"gpt-5.6-sol\"]"));
        assert!(!markdown.contains("output"));
    }

    #[test]
    fn full_markdown_includes_tool_payloads() {
        let markdown = render_markdown(&fixture(), ExportMode::Full);
        assert!(markdown.contains("## Tool · exec\n\noutput"));
    }
}
