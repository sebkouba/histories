use histories_core::{
    AppPaths, Catalog, ConversationDocument, ConversationSummary, ExportMode, RefreshReport,
    Refresher, SearchRequest, SourceConfig,
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Default)]
struct AppState {
    data_dir: Option<PathBuf>,
}

#[derive(Serialize)]
struct BootstrapResponse {
    paths: AppPaths,
    config: SourceConfig,
    refresh: RefreshReport,
    conversations: Vec<ConversationSummary>,
}

#[tauri::command]
async fn bootstrap(
    state: tauri::State<'_, AppState>,
    request: SearchRequest,
) -> Result<BootstrapResponse, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let refresher = Refresher::discover(data_dir).map_err(error_string)?;
        let refresh = refresher.refresh().map_err(error_string)?;
        let catalog = Catalog::open(&refresher.paths.database_path).map_err(error_string)?;
        let conversations = catalog.search(&request).map_err(error_string)?;
        Ok(BootstrapResponse {
            paths: refresher.paths,
            config: refresher.config,
            refresh,
            conversations,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn search_conversations(
    state: tauri::State<'_, AppState>,
    request: SearchRequest,
) -> Result<Vec<ConversationSummary>, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let paths = AppPaths::discover(data_dir).map_err(error_string)?;
        Catalog::open(&paths.database_path)
            .and_then(|catalog| catalog.search(&request))
            .map_err(error_string)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn refresh_catalog(state: tauri::State<'_, AppState>) -> Result<RefreshReport, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Refresher::discover(data_dir)
            .and_then(|refresher| refresher.refresh())
            .map_err(error_string)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn get_documents(
    state: tauri::State<'_, AppState>,
    ids: Vec<i64>,
    mode: ExportMode,
) -> Result<Vec<ConversationDocument>, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Refresher::discover(data_dir)
            .and_then(|refresher| refresher.documents(&ids, mode))
            .map_err(error_string)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn register_chatgpt_export(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<RefreshReport, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(path)
            .canonicalize()
            .map_err(|error| format!("Could not access ChatGPT export: {error}"))?;
        let paths = AppPaths::discover(data_dir).map_err(error_string)?;
        let mut config = SourceConfig::load(&paths).map_err(error_string)?;
        config.add_chatgpt_export(path);
        config.save(&paths).map_err(error_string)?;
        Refresher { paths, config }.refresh().map_err(error_string)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn error_string(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            search_conversations,
            refresh_catalog,
            get_documents,
            register_chatgpt_export,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Histories");
}
