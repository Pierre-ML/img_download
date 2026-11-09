//! Réglages utilisateur : <app_config_dir>/settings.json

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub dest_dir: String,
    pub parallel: u32,
    pub optimize_png: bool,
    pub reencode_video: bool,
    pub video_crf: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dest_dir: default_dest_dir().to_string_lossy().into_owned(),
            parallel: 4,
            optimize_png: true,
            reencode_video: true,
            video_crf: 23,
        }
    }
}

impl Settings {
    /// Borne les valeurs dans les plages autorisées.
    fn normalized(mut self) -> Self {
        self.dest_dir = self.dest_dir.trim().to_string();
        if self.dest_dir.is_empty() {
            self.dest_dir = default_dest_dir().to_string_lossy().into_owned();
        }
        self.parallel = self.parallel.clamp(1, 16);
        self.video_crf = self.video_crf.clamp(18, 35);
        self
    }
}

pub struct SettingsState(Mutex<Settings>);

fn default_dest_dir() -> PathBuf {
    dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("MediaDownloader")
}

fn settings_path(app: &AppHandle) -> anyhow::Result<PathBuf> {
    Ok(app.path().app_config_dir()?.join("settings.json"))
}

fn load(app: &AppHandle) -> Settings {
    let Ok(path) = settings_path(app) else {
        return Settings::default();
    };
    std::fs::read(&path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<Settings>(&raw).ok())
        .unwrap_or_default()
        .normalized()
}

/// Écriture atomique (fichier temporaire puis renommage).
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

/// Prépare le dossier de destination et l'autorise pour le protocole asset.
pub fn allow_dest(app: &AppHandle, dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    if let Err(e) = app.asset_protocol_scope().allow_directory(dir, true) {
        eprintln!("scope asset refusé pour {} : {e}", dir.display());
    }
}

/// Chargement au démarrage.
pub fn init(app: &AppHandle) -> anyhow::Result<()> {
    let s = load(app);
    allow_dest(app, Path::new(&s.dest_dir));
    app.manage(SettingsState(Mutex::new(s)));
    Ok(())
}

/// Copie des réglages courants.
pub fn current(app: &AppHandle) -> Settings {
    let state = app.state::<SettingsState>();
    let guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
    guard.clone()
}

#[tauri::command]
pub async fn get_settings(state: State<'_, SettingsState>) -> Result<Settings, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    Ok(guard.clone())
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, SettingsState>,
    settings: Settings,
) -> Result<Settings, String> {
    let s = settings.normalized();
    let dest = PathBuf::from(&s.dest_dir);
    std::fs::create_dir_all(&dest).map_err(|e| format!("Dossier de destination invalide : {e}"))?;
    let path = settings_path(&app).map_err(|e| e.to_string())?;
    let raw = serde_json::to_vec_pretty(&s).map_err(|e| e.to_string())?;
    write_atomic(&path, &raw).map_err(|e| format!("Enregistrement impossible : {e}"))?;
    allow_dest(&app, &dest);
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    *guard = s.clone();
    Ok(s)
}
