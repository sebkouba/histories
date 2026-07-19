use crate::{
    AppPaths, Catalog, Conversation, ExportMode, HistoriesError, MessageKind, Result, SourceConfig,
    SourceKind, config::atomic_write, parse_chatgpt_export, parse_codex, parse_pi, render_markdown,
};
use chrono::{DateTime, Datelike, Timelike};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    fs::File,
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
        let _lock = RefreshLock::acquire(&self.paths.data_dir.join("refresh.lock"))?;
        self.refresh_unlocked()
    }

    fn refresh_unlocked(&self) -> Result<RefreshReport> {
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
        self.prune_orphaned_exports(&catalog, &mut report)?;
        Ok(report)
    }

    pub fn documents(
        &self,
        ids: &[i64],
        mode: ExportMode,
    ) -> Result<Vec<crate::ConversationDocument>> {
        let _lock = (mode == ExportMode::Full)
            .then(|| RefreshLock::acquire(&self.paths.data_dir.join("refresh.lock")))
            .transpose()?;
        let catalog = Catalog::open(&self.paths.database_path)?;
        if mode == ExportMode::Full {
            self.ensure_full_exports(&catalog, ids)?;
        }
        catalog.documents(ids, mode)
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
        let clean_path = export_path(&self.paths.exports_dir, conversation, ExportMode::Clean);
        let full_path = export_path(&self.paths.exports_dir, conversation, ExportMode::Full);
        atomic_write(&clean_path, clean_markdown.as_bytes())?;
        if full_path.exists() {
            fs::remove_file(&full_path).map_err(|source| HistoriesError::Io {
                path: full_path.clone(),
                source,
            })?;
        }

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
            conversation.started_via.as_deref(),
            &conversation.models,
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

    fn ensure_full_exports(&self, catalog: &Catalog, ids: &[i64]) -> Result<()> {
        let documents = catalog.documents(ids, ExportMode::Clean)?;
        let names = load_codex_names(&self.config.codex_roots);
        let mut chatgpt_cache = HashMap::<String, Vec<Conversation>>::new();

        for document in documents {
            let summary = document.summary;
            let source_path = Path::new(&summary.source_path);
            let conversation = match summary.source {
                SourceKind::Codex => parse_codex(source_path, &names)?,
                SourceKind::Pi => parse_pi(source_path)?,
                SourceKind::Chatgpt => {
                    if !chatgpt_cache.contains_key(&summary.source_path) {
                        chatgpt_cache.insert(
                            summary.source_path.clone(),
                            parse_chatgpt_export(source_path)?,
                        );
                    }
                    chatgpt_cache
                        .get(&summary.source_path)
                        .and_then(|conversations| {
                            conversations
                                .iter()
                                .find(|conversation| {
                                    conversation.external_id == summary.external_id
                                })
                                .cloned()
                        })
                }
            }
            .filter(|conversation| conversation.external_id == summary.external_id)
            .ok_or_else(|| {
                HistoriesError::Message(format!(
                    "could not reconstruct {} conversation {} from {}",
                    summary.source, summary.external_id, summary.source_path
                ))
            })?;
            let markdown = render_markdown(&conversation, ExportMode::Full);
            atomic_write(Path::new(&summary.full_markdown_path), markdown.as_bytes())?;
        }
        Ok(())
    }

    fn prune_orphaned_exports(&self, catalog: &Catalog, report: &mut RefreshReport) -> Result<()> {
        let known = catalog
            .all_export_paths()?
            .into_iter()
            .map(PathBuf::from)
            .collect::<HashSet<_>>();
        let mut entries = WalkDir::new(&self.paths.exports_dir)
            .min_depth(1)
            .contents_first(true)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.depth()));
        for entry in entries {
            let path = entry.path();
            if entry.file_type().is_file() && !known.contains(path) {
                if let Err(error) = fs::remove_file(path) {
                    report.failures.push(RefreshFailure {
                        path: path.display().to_string(),
                        error: error.to_string(),
                    });
                }
            } else if entry.file_type().is_dir() {
                let _ = fs::remove_dir(path);
            }
        }
        Ok(())
    }
}

struct RefreshLock(File);

impl RefreshLock {
    fn acquire(path: &Path) -> Result<Self> {
        let file = crate::config::private_file(path)?;
        file.lock_exclusive().map_err(|source| HistoriesError::Io {
            path: path.to_owned(),
            source,
        })?;
        Ok(Self(file))
    }
}

impl Drop for RefreshLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
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

    fn fixture_refresher() -> (tempfile::TempDir, Refresher, PathBuf) {
        let temporary = tempfile::tempdir().unwrap();
        let source_root = temporary.path().join("codex");
        fs::create_dir_all(&source_root).unwrap();
        let source_path = source_root.join("session.jsonl");
        let records = [
            serde_json::json!({"type":"session_meta","timestamp":"2026-07-01T10:00:00Z","payload":{"id":"session-1","cwd":"/tmp/project","originator":"Codex Desktop","source":"vscode"}}),
            serde_json::json!({"type":"turn_context","timestamp":"2026-07-01T10:00:00Z","payload":{"model":"gpt-5.6-sol"}}),
            serde_json::json!({"type":"event_msg","timestamp":"2026-07-01T10:00:01Z","payload":{"type":"user_message","message":"Private question"}}),
            serde_json::json!({"type":"response_item","timestamp":"2026-07-01T10:00:02Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Private answer"}]}}),
            serde_json::json!({"type":"response_item","timestamp":"2026-07-01T10:00:03Z","payload":{"type":"custom_tool_call_output","output":"sensitive tool output"}}),
        ];
        fs::write(
            &source_path,
            records
                .iter()
                .map(|record| record.to_string())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();
        let paths = AppPaths::discover(Some(temporary.path().join("data"))).unwrap();
        let config = SourceConfig {
            codex_roots: vec![source_root],
            pi_roots: Vec::new(),
            chatgpt_exports: Vec::new(),
        };
        let refresher = Refresher { paths, config };
        (temporary, refresher, source_path)
    }

    #[test]
    fn export_paths_are_stable_and_separate_modes() {
        let conversation = Conversation {
            source: SourceKind::Pi,
            external_id: "abc/123".to_owned(),
            source_path: "/tmp/source".to_owned(),
            title: "Title".to_owned(),
            cwd: None,
            started_via: Some("pi".to_owned()),
            models: vec!["claude-opus-4.8".to_owned()],
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

    #[test]
    fn full_transcripts_are_generated_only_when_requested() {
        let (_temporary, refresher, _source_path) = fixture_refresher();
        refresher.refresh().unwrap();
        let catalog = Catalog::open(&refresher.paths.database_path).unwrap();
        let summary = catalog
            .search(&crate::SearchRequest {
                limit: 10,
                ..Default::default()
            })
            .unwrap()
            .remove(0);

        assert!(Path::new(&summary.clean_markdown_path).exists());
        assert!(!Path::new(&summary.full_markdown_path).exists());
        assert_eq!(summary.started_via.as_deref(), Some("Codex Desktop"));
        assert_eq!(summary.models, ["gpt-5.6-sol"]);

        let documents = refresher
            .documents(&[summary.id], ExportMode::Full)
            .unwrap();
        assert!(documents[0].markdown.contains("sensitive tool output"));
        assert!(Path::new(&summary.full_markdown_path).exists());
    }

    #[test]
    fn removed_conversations_delete_all_derived_exports() {
        let (_temporary, refresher, source_path) = fixture_refresher();
        refresher.refresh().unwrap();
        let catalog = Catalog::open(&refresher.paths.database_path).unwrap();
        let summary = catalog
            .search(&crate::SearchRequest {
                limit: 10,
                ..Default::default()
            })
            .unwrap()
            .remove(0);
        refresher
            .documents(&[summary.id], ExportMode::Full)
            .unwrap();

        fs::remove_file(source_path).unwrap();
        let report = refresher.refresh().unwrap();

        assert_eq!(report.removed, 1);
        assert!(!Path::new(&summary.clean_markdown_path).exists());
        assert!(!Path::new(&summary.full_markdown_path).exists());
        assert_eq!(
            Catalog::open(&refresher.paths.database_path)
                .unwrap()
                .count()
                .unwrap(),
            0
        );
    }
}
