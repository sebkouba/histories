use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Codex,
    Pi,
    Chatgpt,
}

impl SourceKind {
    pub const ALL: [Self; 3] = [Self::Codex, Self::Pi, Self::Chatgpt];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Pi => "pi",
            Self::Chatgpt => "chatgpt",
        }
    }

    pub fn agent_label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Pi => "Pi",
            Self::Chatgpt => "ChatGPT",
        }
    }
}

impl fmt::Display for SourceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for SourceKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "codex" => Ok(Self::Codex),
            "pi" => Ok(Self::Pi),
            "chatgpt" | "chat-gpt" | "chat_gpt" => Ok(Self::Chatgpt),
            _ => Err(format!("unknown source: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    User,
    Assistant,
    Tool,
    System,
    Reasoning,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub sequence: usize,
    pub kind: MessageKind,
    pub author: Option<String>,
    pub content: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub source: SourceKind,
    pub external_id: String,
    pub source_path: String,
    pub title: String,
    pub cwd: Option<String>,
    pub started_via: Option<String>,
    pub start_command: Option<String>,
    pub models: Vec<String>,
    pub started_at: String,
    pub updated_at: String,
    pub messages: Vec<Message>,
}

impl Conversation {
    pub fn clean_messages(&self) -> impl Iterator<Item = &Message> {
        self.messages
            .iter()
            .filter(|message| matches!(message.kind, MessageKind::User | MessageKind::Assistant))
    }

    pub fn approximate_tokens(&self, clean: bool) -> usize {
        let characters = if clean {
            self.clean_messages()
                .map(|message| message.content.chars().count())
                .sum::<usize>()
        } else {
            self.messages
                .iter()
                .map(|message| message.content.chars().count())
                .sum::<usize>()
        };
        characters.div_ceil(4)
    }
}
