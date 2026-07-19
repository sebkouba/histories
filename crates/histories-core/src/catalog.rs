use crate::{ExportMode, HistoriesError, Result, SourceKind, config::secure_file};
use rusqlite::{Connection, OptionalExtension, ToSql, params, params_from_iter};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, time::Duration};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationOrder {
    #[default]
    StartedAt,
    UpdatedAt,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchRequest {
    pub query: Option<String>,
    pub sources: Vec<SourceKind>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub cwd: Option<String>,
    pub sort_by: ConversationOrder,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: i64,
    pub source: SourceKind,
    pub external_id: String,
    pub title: String,
    pub cwd: Option<String>,
    pub started_via: Option<String>,
    pub start_command: Option<String>,
    pub models: Vec<String>,
    pub started_at: String,
    pub updated_at: String,
    pub approx_clean_tokens: usize,
    pub approx_full_tokens: usize,
    pub message_count: usize,
    pub user_message_count: usize,
    pub assistant_message_count: usize,
    pub clean_markdown_path: String,
    pub full_markdown_path: String,
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationDocument {
    #[serde(flatten)]
    pub summary: ConversationSummary,
    pub markdown: String,
}

pub struct Catalog {
    connection: Connection,
}

impl Catalog {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| HistoriesError::Io {
                path: parent.to_owned(),
                source,
            })?;
        }
        let connection = Connection::open(path)?;
        secure_file(path)?;
        Self::from_connection(connection)
    }

    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.busy_timeout(Duration::from_secs(10))?;
        connection.execute_batch(SCHEMA)?;
        let metadata_columns_added = ensure_column(
            &connection,
            "conversations",
            "started_via",
            "started_via TEXT",
        )? | ensure_column(
            &connection,
            "conversations",
            "models_json",
            "models_json TEXT NOT NULL DEFAULT '[]'",
        )? | ensure_column(
            &connection,
            "conversations",
            "start_command",
            "start_command TEXT",
        )?;
        if metadata_columns_added {
            // Source files themselves are authoritative. Invalidating their fingerprints
            // makes the next refresh populate metadata for already-cataloged conversations.
            connection.execute("DELETE FROM source_files", [])?;
        }
        Ok(Self { connection })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_conversation(
        &self,
        source: SourceKind,
        external_id: &str,
        source_path: &str,
        title: &str,
        cwd: Option<&str>,
        started_via: Option<&str>,
        start_command: Option<&str>,
        models: &[String],
        started_at: &str,
        updated_at: &str,
        approx_clean_tokens: usize,
        approx_full_tokens: usize,
        message_count: usize,
        user_message_count: usize,
        assistant_message_count: usize,
        clean_markdown_path: &str,
        full_markdown_path: &str,
        clean_markdown: &str,
    ) -> Result<i64> {
        let models_json = serde_json::to_string(models)?;
        self.connection.execute(
            "INSERT INTO conversations (
                source, external_id, source_path, title, cwd, started_via, start_command, models_json,
                started_at, updated_at,
                approx_clean_tokens, approx_full_tokens, message_count,
                user_message_count, assistant_message_count, clean_markdown_path,
                full_markdown_path, clean_markdown
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
             ON CONFLICT(source, external_id) DO UPDATE SET
                source_path=excluded.source_path, title=excluded.title, cwd=excluded.cwd,
                started_via=excluded.started_via, start_command=excluded.start_command,
                models_json=excluded.models_json,
                started_at=excluded.started_at, updated_at=excluded.updated_at,
                approx_clean_tokens=excluded.approx_clean_tokens,
                approx_full_tokens=excluded.approx_full_tokens,
                message_count=excluded.message_count,
                user_message_count=excluded.user_message_count,
                assistant_message_count=excluded.assistant_message_count,
                clean_markdown_path=excluded.clean_markdown_path,
                full_markdown_path=excluded.full_markdown_path,
                clean_markdown=excluded.clean_markdown",
            params![
                source.as_str(),
                external_id,
                source_path,
                title,
                cwd,
                started_via,
                start_command,
                models_json,
                started_at,
                updated_at,
                approx_clean_tokens as i64,
                approx_full_tokens as i64,
                message_count as i64,
                user_message_count as i64,
                assistant_message_count as i64,
                clean_markdown_path,
                full_markdown_path,
                clean_markdown,
            ],
        )?;
        Ok(self.connection.query_row(
            "SELECT id FROM conversations WHERE source=?1 AND external_id=?2",
            params![source.as_str(), external_id],
            |row| row.get(0),
        )?)
    }

    pub fn source_file_is_current(&self, path: &str, modified_ns: i64, size: u64) -> Result<bool> {
        Ok(self
            .connection
            .query_row(
                "SELECT modified_ns=?2 AND size=?3 FROM source_files WHERE path=?1",
                params![path, modified_ns, size as i64],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    pub fn mark_source_file(
        &self,
        path: &str,
        source: SourceKind,
        modified_ns: i64,
        size: u64,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO source_files(path, source, modified_ns, size, imported_at)
             VALUES(?1, ?2, ?3, ?4, datetime('now'))
             ON CONFLICT(path) DO UPDATE SET source=excluded.source,
                modified_ns=excluded.modified_ns, size=excluded.size,
                imported_at=excluded.imported_at",
            params![path, source.as_str(), modified_ns, size as i64],
        )?;
        Ok(())
    }

    pub fn delete_conversations_for_path(&self, path: &str) -> Result<usize> {
        Ok(self
            .connection
            .execute("DELETE FROM conversations WHERE source_path=?1", [path])?)
    }

    pub fn reconcile_source_files(
        &self,
        source: SourceKind,
        seen_paths: &[String],
    ) -> Result<usize> {
        let known = {
            let mut statement = self
                .connection
                .prepare("SELECT path FROM source_files WHERE source=?1")?;
            statement
                .query_map([source.as_str()], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let stale = known
            .into_iter()
            .filter(|path| !seen_paths.contains(path))
            .collect::<Vec<_>>();
        let mut deleted = 0;
        for path in stale {
            deleted += self.delete_conversations_for_path(&path)?;
            self.connection
                .execute("DELETE FROM source_files WHERE path=?1", [&path])?;
        }
        Ok(deleted)
    }

    pub fn all_export_paths(&self) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT clean_markdown_path FROM conversations
             UNION SELECT full_markdown_path FROM conversations",
        )?;
        Ok(statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn search(&self, request: &SearchRequest) -> Result<Vec<ConversationSummary>> {
        let query = request.query.as_deref().and_then(fts_query);
        let using_fts = query.is_some();
        let mut sql = if using_fts {
            format!(
                "{SUMMARY_SELECT} FROM conversation_fts JOIN conversations c ON c.id=conversation_fts.rowid WHERE conversation_fts MATCH ?"
            )
        } else {
            format!("{SUMMARY_SELECT} FROM conversations c WHERE 1=1")
        };
        let mut values: Vec<Box<dyn ToSql>> = Vec::new();
        if let Some(query) = query {
            values.push(Box::new(query));
        }
        if !request.sources.is_empty() {
            sql.push_str(" AND c.source IN (");
            sql.push_str(
                &std::iter::repeat_n("?", request.sources.len())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            sql.push(')');
            values.extend(
                request
                    .sources
                    .iter()
                    .map(|source| Box::new(source.as_str().to_owned()) as Box<dyn ToSql>),
            );
        }
        if let Some(since) = request.since.as_ref() {
            sql.push_str(" AND c.started_at >= ?");
            values.push(Box::new(since.clone()));
        }
        if let Some(until) = request.until.as_ref() {
            sql.push_str(" AND c.started_at <= ?");
            values.push(Box::new(until.clone()));
        }
        if let Some(cwd) = request.cwd.as_ref().filter(|cwd| !cwd.trim().is_empty()) {
            sql.push_str(" AND c.cwd LIKE ?");
            values.push(Box::new(format!("%{cwd}%")));
        }
        let order_column = match request.sort_by {
            ConversationOrder::StartedAt => "c.started_at",
            ConversationOrder::UpdatedAt => "c.updated_at",
        };
        if using_fts {
            // This tool is primarily for recalling recent work. Relevance still
            // breaks ties, but an older keyword-dense coding session should not
            // jump ahead of yesterday's conversation.
            sql.push_str(&format!(
                " ORDER BY {order_column} DESC, bm25(conversation_fts)"
            ));
        } else {
            sql.push_str(&format!(" ORDER BY {order_column} DESC"));
        }
        sql.push_str(" LIMIT ? OFFSET ?");
        values.push(Box::new(request.limit.clamp(1, 1000) as i64));
        values.push(Box::new(request.offset as i64));

        let mut statement = self.connection.prepare(&sql)?;
        Ok(statement
            .query_map(
                params_from_iter(values.iter().map(|value| value.as_ref())),
                map_summary,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn documents(&self, ids: &[i64], mode: ExportMode) -> Result<Vec<ConversationDocument>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut sql =
            format!("{SUMMARY_SELECT}, c.clean_markdown FROM conversations c WHERE c.id IN (");
        sql.push_str(
            &std::iter::repeat_n("?", ids.len())
                .collect::<Vec<_>>()
                .join(","),
        );
        sql.push_str(") ORDER BY c.started_at ASC");
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(ids.iter()), |row| {
            let summary = map_summary(row)?;
            let clean_markdown: String = row.get(18)?;
            Ok((summary, clean_markdown))
        })?;
        rows.map(|row| {
            let (summary, clean_markdown) = row?;
            let markdown = match mode {
                ExportMode::Clean => clean_markdown,
                ExportMode::Full => {
                    fs::read_to_string(&summary.full_markdown_path).map_err(|source| {
                        HistoriesError::Io {
                            path: summary.full_markdown_path.clone().into(),
                            source,
                        }
                    })?
                }
            };
            Ok(ConversationDocument { summary, markdown })
        })
        .collect()
    }

    pub fn resolve_id(&self, reference: &str) -> Result<i64> {
        if let Ok(id) = reference.parse::<i64>()
            && self
                .connection
                .query_row("SELECT 1 FROM conversations WHERE id=?1", [id], |_| Ok(()))
                .optional()?
                .is_some()
        {
            return Ok(id);
        }
        let pattern = format!("{reference}%");
        let mut statement = self.connection.prepare(
            "SELECT id FROM conversations WHERE external_id=?1 OR external_id LIKE ?2 ORDER BY external_id LIMIT 2",
        )?;
        let ids = statement
            .query_map(params![reference, pattern], |row| row.get::<_, i64>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        match ids.as_slice() {
            [id] => Ok(*id),
            [] => Err(HistoriesError::Message(format!(
                "conversation not found: {reference}"
            ))),
            _ => Err(HistoriesError::Message(format!(
                "conversation reference is ambiguous: {reference}"
            ))),
        }
    }

    pub fn count(&self) -> Result<usize> {
        Ok(self
            .connection
            .query_row("SELECT count(*) FROM conversations", [], |row| {
                row.get::<_, i64>(0)
            })? as usize)
    }
}

fn map_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationSummary> {
    let source: String = row.get(1)?;
    Ok(ConversationSummary {
        id: row.get(0)?,
        source: source.parse().map_err(|error: String| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                Box::new(HistoriesError::Message(error)),
            )
        })?,
        external_id: row.get(2)?,
        title: row.get(3)?,
        cwd: row.get(4)?,
        started_via: row.get(15)?,
        models: serde_json::from_str(&row.get::<_, String>(16)?).unwrap_or_default(),
        start_command: row.get(17)?,
        started_at: row.get(5)?,
        updated_at: row.get(6)?,
        approx_clean_tokens: row.get::<_, i64>(7)? as usize,
        approx_full_tokens: row.get::<_, i64>(8)? as usize,
        message_count: row.get::<_, i64>(9)? as usize,
        user_message_count: row.get::<_, i64>(10)? as usize,
        assistant_message_count: row.get::<_, i64>(11)? as usize,
        clean_markdown_path: row.get(12)?,
        full_markdown_path: row.get(13)?,
        source_path: row.get(14)?,
    })
}

fn fts_query(query: &str) -> Option<String> {
    let terms = query
        .split_whitespace()
        .map(|term| {
            term.trim_matches(|character: char| {
                !character.is_alphanumeric() && character != '_' && character != '-'
            })
        })
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

const SUMMARY_SELECT: &str = "SELECT c.id, c.source, c.external_id, c.title, c.cwd,
    c.started_at, c.updated_at, c.approx_clean_tokens, c.approx_full_tokens,
    c.message_count, c.user_message_count, c.assistant_message_count,
    c.clean_markdown_path, c.full_markdown_path, c.source_path, c.started_via, c.models_json,
    c.start_command";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS conversations (
    id INTEGER PRIMARY KEY,
    source TEXT NOT NULL,
    external_id TEXT NOT NULL,
    source_path TEXT NOT NULL,
    title TEXT NOT NULL,
    cwd TEXT,
    started_via TEXT,
    start_command TEXT,
    models_json TEXT NOT NULL DEFAULT '[]',
    started_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    approx_clean_tokens INTEGER NOT NULL,
    approx_full_tokens INTEGER NOT NULL,
    message_count INTEGER NOT NULL,
    user_message_count INTEGER NOT NULL,
    assistant_message_count INTEGER NOT NULL,
    clean_markdown_path TEXT NOT NULL,
    full_markdown_path TEXT NOT NULL,
    clean_markdown TEXT NOT NULL,
    UNIQUE(source, external_id)
);
CREATE INDEX IF NOT EXISTS conversations_started_at ON conversations(started_at DESC);
CREATE INDEX IF NOT EXISTS conversations_updated_at ON conversations(updated_at DESC);
CREATE INDEX IF NOT EXISTS conversations_source ON conversations(source);
CREATE TABLE IF NOT EXISTS source_files (
    path TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    modified_ns INTEGER NOT NULL,
    size INTEGER NOT NULL,
    imported_at TEXT NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS conversation_fts USING fts5(
    title, cwd, clean_markdown,
    content='conversations', content_rowid='id', tokenize='unicode61'
);
CREATE TRIGGER IF NOT EXISTS conversations_ai AFTER INSERT ON conversations BEGIN
    INSERT INTO conversation_fts(rowid, title, cwd, clean_markdown)
    VALUES (new.id, new.title, new.cwd, new.clean_markdown);
END;
CREATE TRIGGER IF NOT EXISTS conversations_ad AFTER DELETE ON conversations BEGIN
    INSERT INTO conversation_fts(conversation_fts, rowid, title, cwd, clean_markdown)
    VALUES ('delete', old.id, old.title, old.cwd, old.clean_markdown);
END;
CREATE TRIGGER IF NOT EXISTS conversations_au AFTER UPDATE ON conversations BEGIN
    INSERT INTO conversation_fts(conversation_fts, rowid, title, cwd, clean_markdown)
    VALUES ('delete', old.id, old.title, old.cwd, old.clean_markdown);
    INSERT INTO conversation_fts(rowid, title, cwd, clean_markdown)
    VALUES (new.id, new.title, new.cwd, new.clean_markdown);
END;
";

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if columns.iter().any(|existing| existing == column) {
        return Ok(false);
    }
    connection.execute(&format!("ALTER TABLE {table} ADD COLUMN {definition}"), [])?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(catalog: &Catalog, id: &str, title: &str, body: &str, date: &str) {
        catalog
            .upsert_conversation(
                SourceKind::Codex,
                id,
                "/tmp/source",
                title,
                Some("/tmp/project"),
                Some("codex"),
                None,
                &["gpt-5.6-sol".to_owned()],
                date,
                date,
                10,
                20,
                2,
                1,
                1,
                "/tmp/clean.md",
                "/tmp/full.md",
                body,
            )
            .unwrap();
    }

    #[test]
    fn search_uses_fts_and_date_order_for_recent_lists() {
        let catalog = Catalog::in_memory().unwrap();
        insert(
            &catalog,
            "one",
            "IFS session",
            "unblending from a protector",
            "2026-07-01T10:00:00Z",
        );
        insert(
            &catalog,
            "two",
            "Coding",
            "a database migration",
            "2026-07-02T10:00:00Z",
        );
        let all = catalog
            .search(&SearchRequest {
                limit: 50,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            all.iter()
                .map(|item| item.external_id.as_str())
                .collect::<Vec<_>>(),
            ["two", "one"]
        );
        let hits = catalog
            .search(&SearchRequest {
                query: Some("protector".to_owned()),
                limit: 50,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].external_id, "one");
    }

    #[test]
    fn search_can_order_by_last_message() {
        let catalog = Catalog::in_memory().unwrap();
        catalog
            .upsert_conversation(
                SourceKind::Codex,
                "older-active",
                "/tmp/older-active",
                "Older active session",
                Some("/tmp/project"),
                Some("codex"),
                None,
                &["gpt-5.6-sol".to_owned()],
                "2026-07-01T10:00:00Z",
                "2026-07-03T10:00:00Z",
                10,
                20,
                2,
                1,
                1,
                "/tmp/older-active.md",
                "/tmp/older-active.full.md",
                "continued later",
            )
            .unwrap();
        insert(
            &catalog,
            "newer-finished",
            "Newer finished session",
            "finished immediately",
            "2026-07-02T10:00:00Z",
        );

        let by_start = catalog
            .search(&SearchRequest {
                limit: 50,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_start[0].external_id, "newer-finished");

        let by_last_message = catalog
            .search(&SearchRequest {
                sort_by: ConversationOrder::UpdatedAt,
                limit: 50,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_last_message[0].external_id, "older-active");
    }

    #[test]
    fn upsert_replaces_fts_content() {
        let catalog = Catalog::in_memory().unwrap();
        insert(
            &catalog,
            "one",
            "Session",
            "old phrase",
            "2026-07-01T10:00:00Z",
        );
        insert(
            &catalog,
            "one",
            "Session",
            "new phrase",
            "2026-07-01T10:00:00Z",
        );
        assert!(
            catalog
                .search(&SearchRequest {
                    query: Some("old".to_owned()),
                    limit: 50,
                    ..Default::default()
                })
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            catalog
                .search(&SearchRequest {
                    query: Some("new".to_owned()),
                    limit: 50,
                    ..Default::default()
                })
                .unwrap()
                .len(),
            1
        );
    }
}
