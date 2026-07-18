use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Codex,
    Pi,
    Chatgpt,
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
    pub started_at: String,
    pub updated_at: String,
    pub messages: Vec<Message>,
}
