use crate::{Result, error::IoContext};
use directories::{ProjectDirs, UserDirs};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPaths {
    pub home_dir: PathBuf,
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub exports_dir: PathBuf,
    pub config_path: PathBuf,
}

impl AppPaths {
    pub fn discover(override_dir: Option<PathBuf>) -> Result<Self> {
        let home_dir = UserDirs::new()
            .map(|dirs| dirs.home_dir().to_owned())
            .unwrap_or_else(|| PathBuf::from("."));
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
        secure_directory(&data_dir)?;
        let exports_dir = data_dir.join("exports");
        fs::create_dir_all(&exports_dir).at(&exports_dir)?;
        secure_directory(&exports_dir)?;

        Ok(Self {
            home_dir,
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
        secure_directory(parent)?;
    }
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("file"),
        Uuid::new_v4()
    ));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary).at(&temporary)?;
        file.write_all(content).at(&temporary)?;
        file.sync_all().at(&temporary)?;
        fs::rename(&temporary, path).at(path)?;
        secure_file(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options.open(path).at(path)?;
    secure_file(path)?;
    Ok(file)
}

pub(crate) fn secure_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).at(path)?;
    Ok(())
}

fn secure_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).at(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn app_data_and_atomic_files_are_private() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = AppPaths::discover(Some(temporary.path().join("data"))).unwrap();
        atomic_write(&paths.config_path, b"{}\n").unwrap();

        assert_eq!(
            fs::metadata(&paths.data_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&paths.config_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn concurrent_atomic_writes_do_not_share_temporary_paths() {
        let temporary = tempfile::tempdir().unwrap();
        let output = temporary.path().join("output.md");
        let threads = (0..8)
            .map(|index| {
                let output = output.clone();
                std::thread::spawn(move || atomic_write(&output, index.to_string().as_bytes()))
            })
            .collect::<Vec<_>>();

        for thread in threads {
            thread.join().unwrap().unwrap();
        }
        assert!(output.exists());
        assert_eq!(
            fs::read_dir(temporary.path()).unwrap().count(),
            1,
            "temporary files should be cleaned up"
        );
    }
}
