use crate::{Result, error::IoContext};
use directories::{ProjectDirs, UserDirs};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub exports_dir: PathBuf,
    pub config_path: PathBuf,
}

impl AppPaths {
    pub fn discover(override_dir: Option<PathBuf>) -> Result<Self> {
        let data_dir = override_dir
            .or_else(|| env::var_os("HISTORIES_DATA_DIR").map(PathBuf::from))
            .or_else(|| {
                ProjectDirs::from("de", "sebastian", "Histories")
                    .map(|dirs| dirs.data_dir().to_owned())
            })
            .ok_or_else(|| {
                crate::HistoriesError::Message(
                    "could not determine the Histories data directory".to_owned(),
                )
            })?;
        fs::create_dir_all(&data_dir).at(&data_dir)?;
        let exports_dir = data_dir.join("exports");
        fs::create_dir_all(&exports_dir).at(&exports_dir)?;

        Ok(Self {
            database_path: data_dir.join("histories.sqlite3"),
            config_path: data_dir.join("config.json"),
            exports_dir,
            data_dir,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceConfig {
    pub codex_roots: Vec<PathBuf>,
    pub pi_roots: Vec<PathBuf>,
    pub chatgpt_exports: Vec<PathBuf>,
}

impl Default for SourceConfig {
    fn default() -> Self {
        let home = UserDirs::new()
            .map(|dirs| dirs.home_dir().to_owned())
            .unwrap_or_else(|| PathBuf::from("."));
        let mut chatgpt_exports = Vec::new();
        if let Some(path) = env::var_os("HISTORIES_CHATGPT_EXPORT") {
            chatgpt_exports.push(PathBuf::from(path));
        }
        Self {
            codex_roots: vec![
                home.join(".codex/sessions"),
                home.join(".codex/archived_sessions"),
            ],
            pi_roots: vec![home.join(".pi/agent/sessions")],
            chatgpt_exports,
        }
    }
}

impl SourceConfig {
    pub fn load(paths: &AppPaths) -> Result<Self> {
        if !paths.config_path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&paths.config_path).at(&paths.config_path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save(&self, paths: &AppPaths) -> Result<()> {
        let raw = serde_json::to_string_pretty(self)? + "\n";
        atomic_write(&paths.config_path, raw.as_bytes())
    }

    pub fn add_chatgpt_export(&mut self, path: impl AsRef<Path>) {
        let path = path.as_ref().to_owned();
        if !self.chatgpt_exports.contains(&path) {
            self.chatgpt_exports.push(path);
        }
    }
}

pub(crate) fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).at(parent)?;
    }
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("file")
    ));
    fs::write(&temporary, content).at(&temporary)?;
    fs::rename(&temporary, path).at(path)?;
    Ok(())
}
