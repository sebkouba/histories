use crate::{
    AppPaths, Catalog, Conversation, ExportMode, HistoriesError, MessageKind, Result, SourceConfig,
    SourceKind, config::atomic_write, parse_chatgpt_export, parse_codex, parse_pi, render_markdown,
};
use chrono::{DateTime, Datelike, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use walkdir::WalkDir;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RefreshReport {
    pub scanned: usize,
    pub imported: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub failures: Vec<RefreshFailure>,
    pub sources: Vec<SourceReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshFailure {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceReport {
    pub source: SourceKind,
    pub configured: bool,
    pub available: bool,
    pub scanned: usize,
    pub imported: usize,
}

pub struct Refresher {
    pub paths: AppPaths,
    pub config: SourceConfig,
}

impl Refresher {
    pub fn discover(data_dir: Option<PathBuf>) -> Result<Self> {
        let paths = AppPaths::discover(data_dir)?;
        let config = SourceConfig::load(&paths)?;
        Ok(Self { paths, config })
    }

    pub fn refresh(&self) -> Result<RefreshReport> {
        let catalog = Catalog::open(&self.paths.database_path)?;
        let names = load_codex_names(&self.config.codex_roots);
        let mut report = RefreshReport::default();

        self.refresh_jsonl_source(
            &catalog,
            SourceKind::Codex,
            &self.config.codex_roots,
            |path| parse_codex(path, &names),
            &mut report,
        )?;
        self.refresh_jsonl_source(
            &catalog,
            SourceKind::Pi,
            &self.config.pi_roots,
            parse_pi,
            &mut report,
        )?;
        self.refresh_chatgpt(&catalog, &mut report)?;
        Ok(report)
    }

    fn refresh_jsonl_source<F>(
        &self,
        catalog: &Catalog,
        source: SourceKind,
        roots: &[PathBuf],
        parser: F,
        report: &mut RefreshReport,
    ) -> Result<()>
    where
        F: Fn(&Path) -> std::result::Result<Option<Conversation>, crate::ParseError>,
    {
        let files = jsonl_files(roots);
        let mut source_report = SourceReport {
            source,
            configured: !roots.is_empty(),
            available: roots.iter().any(|root| root.exists()),
            scanned: 0,
            imported: 0,
        };
        let seen_paths = files
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();

        for path in files {
            report.scanned += 1;
            source_report.scanned += 1;
            let path_string = path.display().to_string();
            let (modified_ns, size) = match file_fingerprint(&path) {
                Ok(value) => value,
                Err(error) => {
                    report.failures.push(RefreshFailure {
                        path: path_string,
                        error: error.to_string(),
                    });
                    continue;
                }
            };
            if catalog.source_file_is_current(&path_string, modified_ns, size)? {
                report.unchanged += 1;
                continue;
            }
            match parser(&path) {
                Ok(Some(conversation)) => {
                    self.store_conversation(catalog, &conversation)?;
                    catalog.mark_source_file(&path_string, source, modified_ns, size)?;
                    report.imported += 1;
                    source_report.imported += 1;
                }
                Ok(None) => {
                    catalog.mark_source_file(&path_string, source, modified_ns, size)?;
                    report.unchanged += 1;
                }
                Err(error) => report.failures.push(RefreshFailure {
                    path: path_string,
                    error: error.to_string(),
                }),
            }
        }
        let removed = catalog.reconcile_source_files(source, &seen_paths)?;
        report.removed += removed;
        report.sources.push(source_report);
        Ok(())
    }

    fn refresh_chatgpt(&self, catalog: &Catalog, report: &mut RefreshReport) -> Result<()> {
        let source = SourceKind::Chatgpt;
        let mut source_report = SourceReport {
            source,
            configured: !self.config.chatgpt_exports.is_empty(),
            available: self.config.chatgpt_exports.iter().any(|path| path.exists()),
            scanned: 0,
            imported: 0,
        };
        let seen_paths = self
            .config
            .chatgpt_exports
            .iter()
            .filter(|path| path.exists())
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
        for path in self
            .config
            .chatgpt_exports
            .iter()
            .filter(|path| path.exists())
        {
            report.scanned += 1;
            source_report.scanned += 1;
            let path_string = path.display().to_string();
            let (modified_ns, size) = file_fingerprint(path)?;
            if catalog.source_file_is_current(&path_string, modified_ns, size)? {
                report.unchanged += 1;
                continue;
            }
            match parse_chatgpt_export(path) {
                Ok(conversations) => {
                    catalog.delete_conversations_for_path(&path_string)?;
                    for conversation in conversations {
                        self.store_conversation(catalog, &conversation)?;
                        report.imported += 1;
                        source_report.imported += 1;
                    }
                    catalog.mark_source_file(&path_string, source, modified_ns, size)?;
                }
                Err(error) => report.failures.push(RefreshFailure {
                    path: path_string,
                    error: error.to_string(),
                }),
            }
        }
        let removed = catalog.reconcile_source_files(source, &seen_paths)?;
        report.removed += removed;
        report.sources.push(source_report);
        Ok(())
    }

    fn store_conversation(&self, catalog: &Catalog, conversation: &Conversation) -> Result<()> {
        let clean_markdown = render_markdown(conversation, ExportMode::Clean);
        let full_markdown = render_markdown(conversation, ExportMode::Full);
        let clean_path = export_path(&self.paths.exports_dir, conversation, ExportMode::Clean);
        let full_path = export_path(&self.paths.exports_dir, conversation, ExportMode::Full);
        atomic_write(&clean_path, clean_markdown.as_bytes())?;
        atomic_write(&full_path, full_markdown.as_bytes())?;

        let user_count = conversation
            .messages
            .iter()
            .filter(|message| message.kind == MessageKind::User)
            .count();
        let assistant_count = conversation
            .messages
            .iter()
            .filter(|message| message.kind == MessageKind::Assistant)
            .count();
        catalog.upsert_conversation(
            conversation.source,
            &conversation.external_id,
            &conversation.source_path,
            &conversation.title,
            conversation.cwd.as_deref(),
            &conversation.started_at,
            &conversation.updated_at,
            conversation.approximate_tokens(true),
            conversation.approximate_tokens(false),
            conversation.messages.len(),
            user_count,
            assistant_count,
            &clean_path.display().to_string(),
            &full_path.display().to_string(),
            &clean_markdown,
        )?;
        Ok(())
    }
}

fn jsonl_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = roots
        .iter()
        .filter(|root| root.exists())
        .flat_map(|root| {
            WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_map(std::result::Result::ok)
                .filter(|entry| {
                    entry.file_type().is_file()
                        && entry.path().extension().and_then(|value| value.to_str())
                            == Some("jsonl")
                })
                .map(|entry| entry.into_path())
        })
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

fn load_codex_names(roots: &[PathBuf]) -> HashMap<String, String> {
    let Some(codex_dir) = roots.first().and_then(|root| root.parent()) else {
        return HashMap::new();
    };
    let path = codex_dir.join("session_index.jsonl");
    let Ok(raw) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    raw.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|entry| {
            Some((
                entry.get("id")?.as_str()?.to_owned(),
                entry.get("thread_name")?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

fn file_fingerprint(path: &Path) -> Result<(i64, u64)> {
    let metadata = fs::metadata(path).map_err(|source| HistoriesError::Io {
        path: path.to_owned(),
        source,
    })?;
    let modified = metadata.modified().map_err(|source| HistoriesError::Io {
        path: path.to_owned(),
        source,
    })?;
    let duration = modified.duration_since(UNIX_EPOCH).map_err(|error| {
        HistoriesError::Message(format!(
            "invalid modification time for {}: {error}",
            path.display()
        ))
    })?;
    Ok((
        duration.as_nanos().min(i64::MAX as u128) as i64,
        metadata.len(),
    ))
}

fn export_path(root: &Path, conversation: &Conversation, mode: ExportMode) -> PathBuf {
    let parsed = DateTime::parse_from_rfc3339(&conversation.started_at).ok();
    let (month, timestamp) = parsed
        .map(|date| {
            (
                format!("{:04}-{:02}", date.year(), date.month()),
                format!(
                    "{:04}-{:02}-{:02}_{:02}{:02}",
                    date.year(),
                    date.month(),
                    date.day(),
                    date.hour(),
                    date.minute()
                ),
            )
        })
        .unwrap_or_else(|| ("unknown".to_owned(), "unknown-date".to_owned()));
    let id = sanitize_component(&conversation.external_id)
        .chars()
        .take(16)
        .collect::<String>();
    let filename = match mode {
        ExportMode::Clean => format!("{timestamp}_{id}.md"),
        ExportMode::Full => format!("{timestamp}_{id}.full.md"),
    };
    root.join(mode_name(mode))
        .join(conversation.source.as_str())
        .join(month)
        .join(filename)
}

fn mode_name(mode: ExportMode) -> &'static str {
    match mode {
        ExportMode::Clean => "clean",
        ExportMode::Full => "full",
    }
}

fn sanitize_component(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let value = value.trim_matches('-');
    if value.is_empty() {
        "conversation".to_owned()
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_paths_are_stable_and_separate_modes() {
        let conversation = Conversation {
            source: SourceKind::Pi,
            external_id: "abc/123".to_owned(),
            source_path: "/tmp/source".to_owned(),
            title: "Title".to_owned(),
            cwd: None,
            started_at: "2026-07-01T10:22:00Z".to_owned(),
            updated_at: "2026-07-01T10:23:00Z".to_owned(),
            messages: Vec::new(),
        };
        let root = Path::new("/tmp/exports");
        assert_eq!(
            export_path(root, &conversation, ExportMode::Clean),
            Path::new("/tmp/exports/clean/pi/2026-07/2026-07-01_1022_abc-123.md")
        );
        assert_eq!(
            export_path(root, &conversation, ExportMode::Full),
            Path::new("/tmp/exports/full/pi/2026-07/2026-07-01_1022_abc-123.full.md")
        );
    }
}
