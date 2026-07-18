use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, Local, Utc};
use clap::{Args, Parser, Subcommand, ValueEnum};
use histories_core::{
    AppPaths, Catalog, ConversationDocument, ExportMode, RefreshReport, Refresher, SearchRequest,
    SourceConfig, SourceKind, parse_chatgpt_export,
};
use std::{fs, path::PathBuf, thread, time::Duration as StdDuration};

#[derive(Debug, Parser)]
#[command(
    name = "histories",
    version,
    about = "Search and export local AI conversation histories",
    long_about = "A local-first CLI for current Codex, Pi, and ChatGPT history. Read commands refresh changed source files before querying by default."
)]
struct Cli {
    /// Override the Histories application data directory.
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    /// Query the existing catalog without checking source files first.
    #[arg(long, global = true)]
    skip_refresh: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Refresh changed source histories and generated Markdown.
    Refresh {
        #[arg(long)]
        json: bool,
    },
    /// List recent conversations (the default command).
    List {
        #[command(flatten)]
        filters: Filters,
        #[arg(long)]
        json: bool,
    },
    /// Full-text search user and assistant messages.
    Search {
        query: String,
        #[command(flatten)]
        filters: Filters,
        #[arg(long)]
        json: bool,
    },
    /// Print one conversation or its generated Markdown path.
    Show {
        /// Numeric row ID, full session ID, or an unambiguous session-ID prefix.
        reference: String,
        #[arg(long, value_enum, default_value = "clean")]
        mode: ModeArg,
        /// Print the Markdown path instead of its contents.
        #[arg(long)]
        path: bool,
    },
    /// Combine matching conversations into one context-ready Markdown file.
    Export {
        /// Full-text query. Omit when selecting explicit --id values.
        #[arg(long)]
        query: Option<String>,
        /// Conversation row/session ID. Repeat to select multiple conversations.
        #[arg(long = "id")]
        references: Vec<String>,
        #[command(flatten)]
        filters: Filters,
        #[arg(long, value_enum, default_value = "clean")]
        mode: ModeArg,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Print database, export, and configuration paths.
    Paths {
        #[arg(long)]
        json: bool,
    },
    /// Show source coverage and paths.
    Sources {
        #[arg(long)]
        json: bool,
    },
    /// Register a ChatGPT conversations.json export and import it.
    AddChatgptExport { path: PathBuf },
    /// Poll source directories and refresh changed conversations.
    Watch {
        #[arg(long, default_value_t = 2)]
        interval: u64,
    },
}

#[derive(Debug, Args, Clone)]
struct Filters {
    /// Relative window (20d, 48h, 2w) or RFC 3339/date lower bound.
    #[arg(long)]
    since: Option<String>,
    /// RFC 3339/date upper bound.
    #[arg(long)]
    until: Option<String>,
    /// Restrict to an app. Repeat for multiple apps.
    #[arg(long = "source", value_enum)]
    sources: Vec<SourceArg>,
    /// Match a substring of the working directory.
    #[arg(long)]
    cwd: Option<String>,
    /// Maximum conversations to return.
    #[arg(short, long, default_value_t = 100)]
    limit: usize,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            since: Some("7d".to_owned()),
            until: None,
            sources: Vec::new(),
            cwd: None,
            limit: 100,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SourceArg {
    Codex,
    Pi,
    Chatgpt,
}

impl From<SourceArg> for SourceKind {
    fn from(value: SourceArg) -> Self {
        match value {
            SourceArg::Codex => Self::Codex,
            SourceArg::Pi => Self::Pi,
            SourceArg::Chatgpt => Self::Chatgpt,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ModeArg {
    Clean,
    Full,
}

impl From<ModeArg> for ExportMode {
    fn from(value: ModeArg) -> Self {
        match value {
            ModeArg::Clean => Self::Clean,
            ModeArg::Full => Self::Full,
        }
    }
}

fn main() -> Result<()> {
    let Cli {
        data_dir,
        skip_refresh,
        command,
    } = Cli::parse();
    let command = command.unwrap_or(Command::List {
        filters: Filters::default(),
        json: false,
    });

    match command {
        Command::Refresh { json } => {
            let report = Refresher::discover(data_dir)?.refresh()?;
            print_refresh(&report, json)?;
        }
        Command::List { filters, json } => {
            let paths = prepare(data_dir, skip_refresh, true)?;
            let catalog = Catalog::open(&paths.database_path)?;
            let results = catalog.search(&request(None, filters)?)?;
            print_results(&results, json)?;
        }
        Command::Search {
            query,
            filters,
            json,
        } => {
            let paths = prepare(data_dir, skip_refresh, true)?;
            let catalog = Catalog::open(&paths.database_path)?;
            let results = catalog.search(&request(Some(query), filters)?)?;
            print_results(&results, json)?;
        }
        Command::Show {
            reference,
            mode,
            path,
        } => {
            let paths = prepare(data_dir, skip_refresh, true)?;
            let catalog = Catalog::open(&paths.database_path)?;
            let id = catalog.resolve_id(&reference)?;
            let refresher = Refresher::discover(Some(paths.data_dir.clone()))?;
            let mut documents = refresher.documents(&[id], mode.into())?;
            let document = documents.pop().context("conversation not found")?;
            if path {
                println!(
                    "{}",
                    match mode {
                        ModeArg::Clean => document.summary.clean_markdown_path,
                        ModeArg::Full => document.summary.full_markdown_path,
                    }
                );
            } else {
                print!("{}", document.markdown);
            }
        }
        Command::Export {
            query,
            references,
            filters,
            mode,
            output,
        } => {
            let paths = prepare(data_dir, skip_refresh, true)?;
            let catalog = Catalog::open(&paths.database_path)?;
            let ids = if references.is_empty() {
                catalog
                    .search(&request(query, filters)?)?
                    .into_iter()
                    .map(|conversation| conversation.id)
                    .collect()
            } else {
                references
                    .iter()
                    .map(|reference| catalog.resolve_id(reference))
                    .collect::<histories_core::Result<Vec<_>>>()?
            };
            if ids.is_empty() {
                bail!("no conversations matched; no file was written");
            }
            let mode = mode.into();
            let refresher = Refresher::discover(Some(paths.data_dir.clone()))?;
            let documents = refresher.documents(&ids, mode)?;
            let bundle = document_bundle(&documents, mode);
            if let Some(parent) = output
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)
                    .with_context(|| format!("could not create {}", parent.display()))?;
            }
            fs::write(&output, bundle)
                .with_context(|| format!("could not write {}", output.display()))?;
            println!("{}", output.canonicalize().unwrap_or(output).display());
        }
        Command::Paths { json } => {
            let paths = AppPaths::discover(data_dir)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&paths)?);
            } else {
                println!("database\t{}", paths.database_path.display());
                println!("exports\t{}", paths.exports_dir.display());
                println!("config\t{}", paths.config_path.display());
            }
        }
        Command::Sources { json } => {
            let refresher = Refresher::discover(data_dir)?;
            let report = if skip_refresh {
                None
            } else {
                Some(refresher.refresh()?)
            };
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"config": refresher.config, "refresh": report})
                    )?
                );
            } else {
                for root in &refresher.config.codex_roots {
                    println!("codex\t{}\t{}", availability(root), root.display());
                }
                for root in &refresher.config.pi_roots {
                    println!("pi\t{}\t{}", availability(root), root.display());
                }
                if refresher.config.chatgpt_exports.is_empty() {
                    println!(
                        "chatgpt\tneeds-export\tRegister conversations.json with `histories add-chatgpt-export PATH`"
                    );
                } else {
                    for root in &refresher.config.chatgpt_exports {
                        println!("chatgpt\t{}\t{}", availability(root), root.display());
                    }
                }
            }
        }
        Command::AddChatgptExport { path } => {
            let path = path
                .canonicalize()
                .with_context(|| format!("could not access {}", path.display()))?;
            let conversation_count = parse_chatgpt_export(&path)?.len();
            if conversation_count == 0 {
                bail!("the ChatGPT export contains no usable conversations");
            }
            let paths = AppPaths::discover(data_dir)?;
            let mut config = SourceConfig::load(&paths)?;
            config.add_chatgpt_export(&path);
            config.save(&paths)?;
            let report = Refresher { paths, config }.refresh()?;
            print_refresh(&report, false)?;
        }
        Command::Watch { interval } => {
            let refresher = Refresher::discover(data_dir)?;
            eprintln!(
                "Watching configured histories every {}s. Press Ctrl-C to stop.",
                interval.max(1)
            );
            loop {
                match refresher.refresh() {
                    Ok(report)
                        if report.imported > 0
                            || report.removed > 0
                            || !report.failures.is_empty() =>
                    {
                        print_refresh(&report, false)?
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("refresh failed: {error}"),
                }
                thread::sleep(StdDuration::from_secs(interval.max(1)));
            }
        }
    }
    Ok(())
}

fn prepare(data_dir: Option<PathBuf>, skip_refresh: bool, refresh: bool) -> Result<AppPaths> {
    let refresher = Refresher::discover(data_dir)?;
    if refresh && !skip_refresh {
        let report = refresher.refresh()?;
        if report.imported > 0 || report.removed > 0 {
            eprintln!(
                "Refreshed {} changed conversation(s), removed {}.",
                report.imported, report.removed
            );
        }
        for failure in report.failures {
            eprintln!("Warning: {}: {}", failure.path, failure.error);
        }
    }
    Ok(refresher.paths)
}

fn request(query: Option<String>, filters: Filters) -> Result<SearchRequest> {
    Ok(SearchRequest {
        query,
        sources: filters.sources.into_iter().map(Into::into).collect(),
        since: filters.since.as_deref().map(parse_bound).transpose()?,
        until: filters.until.as_deref().map(parse_bound).transpose()?,
        cwd: filters.cwd,
        limit: filters.limit,
        offset: 0,
    })
}

fn parse_bound(value: &str) -> Result<String> {
    let value = value.trim();
    let relative = value.chars().last().and_then(|unit| {
        let number = value[..value.len().saturating_sub(unit.len_utf8())]
            .parse::<i64>()
            .ok()?;
        match unit {
            'm' => Some(Duration::minutes(number)),
            'h' => Some(Duration::hours(number)),
            'd' => Some(Duration::days(number)),
            'w' => Some(Duration::weeks(number)),
            _ => None,
        }
    });
    if let Some(duration) = relative {
        return Ok((Utc::now() - duration).to_rfc3339());
    }
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Ok(date.to_rfc3339());
    }
    if value.len() == 10 {
        return Ok(format!("{value}T00:00:00Z"));
    }
    bail!("invalid time bound `{value}`; use 20d, 48h, 2w, YYYY-MM-DD, or RFC 3339")
}

fn print_results(results: &[histories_core::ConversationSummary], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(results)?);
        return Ok(());
    }
    if results.is_empty() {
        eprintln!("No conversations found.");
        return Ok(());
    }
    println!("ID\tDATE\tAPP\tTOKENS\tTITLE\tCWD");
    for conversation in results {
        let date = DateTime::parse_from_rfc3339(&conversation.started_at)
            .map(|date| {
                date.with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|_| conversation.started_at.chars().take(16).collect());
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            conversation.id,
            date,
            conversation.source,
            human_tokens(conversation.approx_clean_tokens),
            one_line(&conversation.title),
            conversation.cwd.as_deref().unwrap_or(""),
        );
    }
    Ok(())
}

fn print_refresh(report: &RefreshReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
    } else {
        println!(
            "Scanned {}, imported {}, unchanged {}, removed {}, failures {}.",
            report.scanned,
            report.imported,
            report.unchanged,
            report.removed,
            report.failures.len()
        );
        for source in &report.sources {
            println!(
                "{}\tconfigured={}\tavailable={}\tscanned={}\timported={}",
                source.source, source.configured, source.available, source.scanned, source.imported
            );
        }
        for failure in &report.failures {
            eprintln!("Warning: {}: {}", failure.path, failure.error);
        }
    }
    Ok(())
}

fn document_bundle(documents: &[ConversationDocument], mode: ExportMode) -> String {
    let mut output = format!(
        "# Conversation history bundle\n\nSessions: {}\nMode: {}\n\n",
        documents.len(),
        match mode {
            ExportMode::Clean => "clean",
            ExportMode::Full => "full",
        }
    );
    for (index, document) in documents.iter().enumerate() {
        if index > 0 {
            output.push_str("\n---\n\n");
        }
        output.push_str(&document.markdown);
    }
    output
}

fn availability(path: &std::path::Path) -> &'static str {
    if path.exists() {
        "available"
    } else {
        "missing"
    }
}

fn human_tokens(tokens: usize) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}m", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn one_line(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(100)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relative_and_absolute_bounds() {
        assert!(parse_bound("20d").unwrap().contains('T'));
        assert_eq!(parse_bound("2026-07-01").unwrap(), "2026-07-01T00:00:00Z");
        assert!(parse_bound("nonsense").is_err());
    }
}
