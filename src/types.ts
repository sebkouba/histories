export type SourceKind = "codex" | "pi" | "chatgpt";
export type ExportMode = "clean" | "full";

export type SearchRequest = {
  query: string | null;
  sources: SourceKind[];
  since: string | null;
  until: string | null;
  cwd: string | null;
  limit: number;
  offset: number;
};

export type ConversationSummary = {
  id: number;
  source: SourceKind;
  external_id: string;
  title: string;
  cwd: string | null;
  started_at: string;
  updated_at: string;
  approx_clean_tokens: number;
  approx_full_tokens: number;
  message_count: number;
  user_message_count: number;
  assistant_message_count: number;
  clean_markdown_path: string;
  full_markdown_path: string;
  source_path: string;
};

export type ConversationDocument = ConversationSummary & {
  markdown: string;
};

export type SourceReport = {
  source: SourceKind;
  configured: boolean;
  available: boolean;
  scanned: number;
  imported: number;
};

export type RefreshReport = {
  scanned: number;
  imported: number;
  unchanged: number;
  removed: number;
  failures: Array<{ path: string; error: string }>;
  sources: SourceReport[];
};

export type AppPaths = {
  data_dir: string;
  database_path: string;
  exports_dir: string;
  config_path: string;
};

export type SourceConfig = {
  codex_roots: string[];
  pi_roots: string[];
  chatgpt_exports: string[];
};

export type BootstrapResponse = {
  paths: AppPaths;
  config: SourceConfig;
  refresh: RefreshReport;
  conversations: ConversationSummary[];
};

