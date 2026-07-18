import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import {
  ChevronIcon,
  CloseIcon,
  CopyIcon,
  ImportIcon,
  PathIcon,
  RefreshIcon,
  RevealIcon,
  SearchIcon,
} from "./Icons";
import { PaneDivider } from "./PaneDivider";
import type {
  AppPaths,
  BootstrapResponse,
  ConversationDocument,
  ConversationSummary,
  ExportMode,
  RefreshReport,
  SearchRequest,
  SourceConfig,
  SourceKind,
} from "./types";
import { usePreviewPaneWidth } from "./usePreviewPaneWidth";

const RANGE_OPTIONS = [
  { label: "2 days", value: 2 },
  { label: "7 days", value: 7 },
  { label: "20 days", value: 20 },
  { label: "30 days", value: 30 },
  { label: "All", value: 0 },
];

const SOURCE_LABELS: Record<SourceKind, string> = {
  codex: "Codex",
  pi: "Pi",
  chatgpt: "ChatGPT",
};

export function App() {
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  const [query, setQuery] = useState("");
  const [rangeDays, setRangeDays] = useState(30);
  const [sources, setSources] = useState<SourceKind[]>([]);
  const [mode, setMode] = useState<ExportMode>("clean");
  const [selected, setSelected] = useState<Record<number, ConversationSummary>>({});
  const [activeId, setActiveId] = useState<number | null>(null);
  const [preview, setPreview] = useState<ConversationDocument | null>(null);
  const [paths, setPaths] = useState<AppPaths | null>(null);
  const [config, setConfig] = useState<SourceConfig | null>(null);
  const [lastRefresh, setLastRefresh] = useState<RefreshReport | null>(null);
  const [ready, setReady] = useState(false);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const searchGeneration = useRef(0);
  const refreshInFlight = useRef(false);
  const searchInput = useRef<HTMLInputElement>(null);
  const previewPane = usePreviewPaneWidth();

  const request = useMemo<SearchRequest>(() => ({
    query: query.trim() || null,
    sources,
    since: rangeDays ? new Date(Date.now() - rangeDays * 86_400_000).toISOString() : null,
    until: null,
    cwd: null,
    limit: 400,
    offset: 0,
  }), [query, rangeDays, sources]);

  const runSearch = useCallback(async (nextRequest = request) => {
    const generation = ++searchGeneration.current;
    try {
      const results = await invoke<ConversationSummary[]>("search_conversations", { request: nextRequest });
      if (generation === searchGeneration.current) {
        setConversations(results);
        setError(null);
      }
    } catch (reason) {
      if (generation === searchGeneration.current) setError(String(reason));
    }
  }, [request]);

  useEffect(() => {
    let cancelled = false;
    invoke<BootstrapResponse>("bootstrap", { request })
      .then((response) => {
        if (cancelled) return;
        setConversations(response.conversations);
        setPaths(response.paths);
        setConfig(response.config);
        setLastRefresh(response.refresh);
        setReady(true);
        setError(response.refresh.failures[0]?.error ?? null);
      })
      .catch((reason) => !cancelled && setError(String(reason)))
      .finally(() => !cancelled && setLoading(false));
    return () => { cancelled = true; };
    // Bootstrap once; later request changes use the debounced search effect.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!ready) return;
    const timer = window.setTimeout(() => void runSearch(), 220);
    return () => window.clearTimeout(timer);
  }, [ready, runSearch]);

  const refresh = useCallback(async (quiet = false) => {
    if (refreshInFlight.current) return;
    refreshInFlight.current = true;
    if (!quiet) setRefreshing(true);
    try {
      const report = await invoke<RefreshReport>("refresh_catalog");
      setLastRefresh(report);
      if (report.imported || report.removed) await runSearch();
      if (report.failures.length) setError(report.failures[0].error);
      else if (!quiet) showToast(report.imported ? `Updated ${report.imported} conversation${report.imported === 1 ? "" : "s"}` : "Everything is current");
    } catch (reason) {
      if (!quiet) setError(String(reason));
    } finally {
      refreshInFlight.current = false;
      if (!quiet) setRefreshing(false);
    }
  }, [runSearch]);

  useEffect(() => {
    if (!ready) return;
    const interval = window.setInterval(() => void refresh(true), 5_000);
    return () => window.clearInterval(interval);
  }, [ready, refresh]);

  useEffect(() => {
    if (activeId === null) {
      setPreview(null);
      return;
    }
    let cancelled = false;
    setPreview(null);
    invoke<ConversationDocument[]>("get_documents", { ids: [activeId], mode })
      .then((documents) => !cancelled && setPreview(documents[0] ?? null))
      .catch((reason) => !cancelled && setError(String(reason)));
    return () => { cancelled = true; };
  }, [activeId, mode]);

  useEffect(() => {
    const close = (event: KeyboardEvent) => event.key === "Escape" && setActiveId(null);
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, []);

  useEffect(() => {
    const focusSearch = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        searchInput.current?.focus();
        searchInput.current?.select();
      }
    };
    window.addEventListener("keydown", focusSearch);
    return () => window.removeEventListener("keydown", focusSearch);
  }, []);

  const selectedItems = Object.values(selected);
  const grouped = useMemo(() => groupByDay(conversations), [conversations]);

  function showToast(message: string) {
    setToast(message);
    window.setTimeout(() => setToast((current) => current === message ? null : current), 2_400);
  }

  function toggleSource(source: SourceKind) {
    setSources((current) => current.includes(source) ? current.filter((item) => item !== source) : [...current, source]);
  }

  function toggleSelected(conversation: ConversationSummary) {
    setSelected((current) => {
      const next = { ...current };
      if (next[conversation.id]) delete next[conversation.id];
      else next[conversation.id] = conversation;
      return next;
    });
  }

  async function copyDocuments(ids: number[]) {
    if (!ids.length) return;
    try {
      const documents = await invoke<ConversationDocument[]>("get_documents", { ids, mode });
      await writeText(bundleDocuments(documents, mode));
      showToast(`Copied ${documents.length} conversation${documents.length === 1 ? "" : "s"}`);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function copyPath(conversation: ConversationSummary) {
    const path = mode === "clean" ? conversation.clean_markdown_path : conversation.full_markdown_path;
    await writeText(path);
    showToast("Markdown path copied");
  }

  async function copySelectedPaths() {
    const ordered = [...selectedItems].sort((left, right) => left.started_at.localeCompare(right.started_at));
    const value = ordered.map((conversation) => mode === "clean" ? conversation.clean_markdown_path : conversation.full_markdown_path).join("\n");
    await writeText(value);
    showToast(`Copied ${ordered.length} path${ordered.length === 1 ? "" : "s"}`);
  }

  async function importChatgpt() {
    const path = await open({
      title: "Choose ChatGPT conversations.json",
      multiple: false,
      directory: false,
      filters: [{ name: "ChatGPT export", extensions: ["json"] }],
    });
    if (typeof path !== "string") return;
    setRefreshing(true);
    try {
      const report = await invoke<RefreshReport>("register_chatgpt_export", { path });
      setLastRefresh(report);
      setConfig((current) => current ? { ...current, chatgpt_exports: [...new Set([...current.chatgpt_exports, path])] } : current);
      await runSearch();
      showToast(`Imported ${report.imported} ChatGPT conversation${report.imported === 1 ? "" : "s"}`);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRefreshing(false);
    }
  }

  const visibleTokens = conversations.reduce((sum, conversation) => sum + (mode === "clean" ? conversation.approx_clean_tokens : conversation.approx_full_tokens), 0);
  const appStyle = { "--preview-width": `${previewPane.width}px` } as CSSProperties;
  const compactLibrary = activeId !== null && previewPane.libraryWidth < 980;

  return (
    <div className={`app-frame ${activeId !== null ? "preview-open" : ""} ${compactLibrary ? "compact-library" : ""}`} style={appStyle}>
      <header className="topbar">
        <div className="brand-block">
          <div className="brand-mark"><i /><i /><i /></div>
          <div>
            <h1>Histories</h1>
            <p>Local conversation library</p>
          </div>
        </div>
        <div className="topbar-actions">
          <span className="freshness"><i className={lastRefresh?.failures.length ? "warning" : ""} />{refreshing ? "Checking sources…" : "Current · checks every 5s"}</span>
          <button className="button subtle" onClick={() => void refresh()} disabled={refreshing}>
            <span className={refreshing ? "spin" : ""}><RefreshIcon /></span> Refresh
          </button>
          <button className="button primary" onClick={() => void importChatgpt()}>
            <ImportIcon /> {config?.chatgpt_exports.length ? "Update ChatGPT export" : "Import ChatGPT"}
          </button>
        </div>
      </header>

      <main className="workspace">
        <section className="controls" aria-label="Conversation filters">
          <div className="search-box">
            <SearchIcon size={20} />
            <input
              aria-label="Search conversations"
              autoFocus
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search every user and agent message…"
              ref={searchInput}
              spellCheck={false}
              value={query}
            />
            {query && <button className="clear-search" aria-label="Clear search" onClick={() => setQuery("")}><CloseIcon size={15} /></button>}
            <kbd>⌘ K</kbd>
          </div>

          <div className="filter-row">
            <div className="filter-cluster">
              <span className="filter-label">When</span>
              <div className="segmented compact">
                {RANGE_OPTIONS.map((option) => (
                  <button className={rangeDays === option.value ? "active" : ""} key={option.value} onClick={() => setRangeDays(option.value)}>{option.label}</button>
                ))}
              </div>
            </div>
            <div className="filter-cluster">
              <span className="filter-label">App</span>
              <div className="source-filters">
                {(Object.keys(SOURCE_LABELS) as SourceKind[]).map((source) => (
                  <button className={sources.includes(source) ? `source-chip active ${source}` : "source-chip"} key={source} onClick={() => toggleSource(source)}>
                    <SourceMark source={source} /> {SOURCE_LABELS[source]}
                  </button>
                ))}
              </div>
            </div>
            <div className="mode-control">
              <span className="filter-label">Transcript</span>
              <div className="segmented mode">
                <button className={mode === "clean" ? "active" : ""} onClick={() => setMode("clean")}>Clean</button>
                <button className={mode === "full" ? "active" : ""} onClick={() => setMode("full")}>Full</button>
              </div>
              <span className="mode-hint">{mode === "clean" ? "You + agent only" : "Includes tools + reasoning"}</span>
            </div>
          </div>
        </section>

        {selectedItems.length > 0 && (
          <section className="selection-bar">
            <div><strong>{selectedItems.length}</strong> selected <button onClick={() => setSelected({})}>Clear</button></div>
            <div className="selection-actions">
              <button className="button subtle" onClick={() => void copySelectedPaths()}><PathIcon /> Copy paths</button>
              <button className="button primary" onClick={() => void copyDocuments(selectedItems.map((item) => item.id))}><CopyIcon /> Copy {mode} transcript{selectedItems.length === 1 ? "" : "s"}</button>
            </div>
          </section>
        )}

        <section className="result-summary">
          <p><strong>{conversations.length}</strong> conversation{conversations.length === 1 ? "" : "s"} <span>·</span> ~{humanTokens(visibleTokens)} tokens in {mode} mode</p>
          {query && <p className="query-note">Ranked matches for “{query}”</p>}
        </section>

        {loading ? (
          <LoadingState />
        ) : error && !conversations.length ? (
          <EmptyState title="The library could not be loaded" detail={error} />
        ) : !conversations.length ? (
          <EmptyState title={query ? "No conversations match that search" : "No conversations in this range"} detail="Try a wider date range, another app, or a different phrase." />
        ) : (
          <div className="conversation-groups">
            {grouped.map(([day, items]) => (
              <section className="day-group" key={day}>
                <header><h2>{dayLabel(day)}</h2><span>{longDate(day)} · {items.length}</span></header>
                <div className="list-head"><span /><span>Time</span><span>Conversation</span><span>Length</span><span>Working directory</span><span>Actions</span></div>
                <div className="conversation-list">
                  {items.map((conversation) => (
                    <article className={`conversation-row ${selected[conversation.id] ? "selected" : ""}`} key={conversation.id}>
                      <label className="check-wrap" title="Select conversation">
                        <input type="checkbox" checked={Boolean(selected[conversation.id])} onChange={() => toggleSelected(conversation)} />
                        <span />
                      </label>
                      <time dateTime={conversation.started_at}>{timeLabel(conversation.started_at)}</time>
                      <button className="conversation-title" onClick={() => setActiveId(conversation.id)}>
                        <span className="title-line"><SourceMark source={conversation.source} /><strong>{conversation.title}</strong></span>
                        <span className="message-count">{conversation.user_message_count} user · {conversation.assistant_message_count} agent messages</span>
                      </button>
                      <div className="token-count"><strong>~{humanTokens(mode === "clean" ? conversation.approx_clean_tokens : conversation.approx_full_tokens)}</strong><span>tokens</span></div>
                      <div className="cwd" title={conversation.cwd ?? "No working directory"}>{shortPath(conversation.cwd)}</div>
                      <div className="row-actions">
                        <IconButton label={`Copy ${mode} Markdown`} onClick={() => void copyDocuments([conversation.id])}><CopyIcon /></IconButton>
                        <IconButton label={`Copy ${mode} Markdown path`} onClick={() => void copyPath(conversation)}><PathIcon /></IconButton>
                        <IconButton label="Reveal Markdown in Finder" onClick={() => void revealItemInDir(mode === "clean" ? conversation.clean_markdown_path : conversation.full_markdown_path)}><RevealIcon /></IconButton>
                        <button className="open-preview" aria-label="Preview conversation" onClick={() => setActiveId(conversation.id)}><ChevronIcon /></button>
                      </div>
                    </article>
                  ))}
                </div>
              </section>
            ))}
          </div>
        )}

        <footer className="library-footer">
          <div className="coverage">
            {lastRefresh?.sources.map((source) => <span key={source.source}><i className={source.available ? "ok" : "missing"} />{SOURCE_LABELS[source.source]} {source.available ? "connected" : source.source === "chatgpt" ? "needs export" : "not found"}</span>)}
          </div>
          {paths && <button title={paths.exports_dir} onClick={() => void revealItemInDir(paths.exports_dir)}><PathIcon size={15} /> Open Markdown library</button>}
        </footer>
      </main>

      {activeId !== null && (
        <aside className="preview-panel" aria-label="Conversation preview">
          <PaneDivider
            maxWidth={previewPane.maxWidth}
            minWidth={previewPane.minWidth}
            onResize={previewPane.setWidth}
            width={previewPane.width}
          />
          <header>
            <div><span className="preview-kicker">{mode} transcript</span><h2 title={preview?.title}>{preview?.title ?? "Loading…"}</h2></div>
            <button className="close-preview" aria-label="Close preview" onClick={() => setActiveId(null)}><CloseIcon /></button>
          </header>
          {preview ? (
            <>
              <div className="preview-meta">
                <SourceMark source={preview.source} />
                <span>{SOURCE_LABELS[preview.source]}</span>
                <span>·</span>
                <span>{formatDateTime(preview.started_at)}</span>
                <span>·</span>
                <span>~{humanTokens(mode === "clean" ? preview.approx_clean_tokens : preview.approx_full_tokens)} tokens</span>
              </div>
              <HighlightedMarkdown markdown={preview.markdown} query={query} />
              <div className="preview-actions">
                <button className="button subtle" onClick={() => void copyPath(preview)}><PathIcon /> Copy path</button>
                <button className="button primary" onClick={() => void copyDocuments([preview.id])}><CopyIcon /> Copy transcript</button>
              </div>
            </>
          ) : <LoadingState compact />}
        </aside>
      )}

      {error && conversations.length > 0 && <div className="error-banner"><span>{error}</span><button onClick={() => setError(null)}><CloseIcon size={15} /></button></div>}
      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

function SourceMark({ source }: { source: SourceKind }) {
  return <span className={`source-mark ${source}`} aria-label={SOURCE_LABELS[source]}>{source === "codex" ? "C" : source === "pi" ? "π" : "G"}</span>;
}

function IconButton({ label, children, onClick }: { label: string; children: React.ReactNode; onClick: () => void }) {
  return <button className="icon-button" aria-label={label} title={label} onClick={onClick}>{children}</button>;
}

function LoadingState({ compact = false }: { compact?: boolean }) {
  return <div className={compact ? "loading-state compact" : "loading-state"}><i /><span>Loading conversations…</span></div>;
}

function EmptyState({ title, detail }: { title: string; detail: string }) {
  return <div className="empty-state"><div className="empty-glyph"><i /><i /><i /></div><h2>{title}</h2><p>{detail}</p></div>;
}

function HighlightedMarkdown({ markdown, query }: { markdown: string; query: string }) {
  const content = useRef<HTMLPreElement>(null);
  const terms = useMemo(() => searchTerms(query), [query]);
  const matcher = useMemo(() => terms.length
    ? new RegExp(`(${terms.map(escapeRegExp).join("|")})`, "giu")
    : null, [terms]);
  const parts = useMemo(() => matcher ? markdown.split(matcher) : [markdown], [markdown, matcher]);

  useEffect(() => {
    const container = content.current;
    if (!container) return;
    const frame = window.requestAnimationFrame(() => {
      const firstMatch = container.querySelector<HTMLElement>("[data-search-match]");
      container.scrollTop = firstMatch
        ? Math.max(0, firstMatch.offsetTop - container.clientHeight * 0.3)
        : 0;
    });
    return () => window.cancelAnimationFrame(frame);
  }, [markdown, matcher]);

  return (
    <pre ref={content}>
      {parts.map((part, index) => index % 2
        ? <mark data-search-match key={index}>{part}</mark>
        : part)}
    </pre>
  );
}

function searchTerms(query: string) {
  const operators = new Set(["and", "or", "not", "near"]);
  const terms = [...query.matchAll(/"([^"]+)"|([\p{L}\p{N}][\p{L}\p{N}_'-]*)/gu)]
    .map((match) => (match[1] ?? match[2]).trim())
    .filter((term) => term && !operators.has(term.toLocaleLowerCase()));
  return [...new Map(terms.map((term) => [term.toLocaleLowerCase(), term])).values()]
    .sort((left, right) => right.length - left.length);
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function groupByDay(conversations: ConversationSummary[]): Array<[string, ConversationSummary[]]> {
  const groups = new Map<string, ConversationSummary[]>();
  for (const conversation of conversations) {
    const date = safeDate(conversation.started_at);
    const key = date ? localDayKey(date) : "unknown";
    const current = groups.get(key) ?? [];
    current.push(conversation);
    groups.set(key, current);
  }
  return [...groups.entries()];
}

function localDayKey(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function dayLabel(key: string) {
  if (key === "unknown") return "Unknown date";
  const date = new Date(`${key}T12:00:00`);
  const today = new Date();
  const yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  if (localDayKey(date) === localDayKey(today)) return "Today";
  if (localDayKey(date) === localDayKey(yesterday)) return "Yesterday";
  return date.toLocaleDateString(undefined, { weekday: "long" });
}

function longDate(key: string) {
  if (key === "unknown") return "";
  return new Date(`${key}T12:00:00`).toLocaleDateString(undefined, { month: "long", day: "numeric", year: "numeric" });
}

function timeLabel(value: string) {
  const date = safeDate(value);
  return date ? date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" }) : "—";
}

function formatDateTime(value: string) {
  const date = safeDate(value);
  return date ? date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) : value;
}

function safeDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

function shortPath(value: string | null) {
  if (!value) return "—";
  const home = "/Users/sebastian/";
  return value.startsWith(home) ? `~/${value.slice(home.length)}` : value;
}

function humanTokens(tokens: number) {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}m`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
  return String(tokens);
}

function bundleDocuments(documents: ConversationDocument[], mode: ExportMode) {
  if (documents.length === 1) return documents[0].markdown;
  return `# Conversation history bundle\n\nSessions: ${documents.length}\nMode: ${mode}\n\n${documents.map((document) => document.markdown).join("\n---\n\n")}`;
}
