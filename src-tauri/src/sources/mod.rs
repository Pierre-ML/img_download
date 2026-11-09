//! Interface partagée entre le core et les sources (Google, X, Reddit).

pub mod google;
pub mod reddit;
pub mod x;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone)]
pub struct MediaCandidate {
    /// Clé stable unique : "google:<url>", "x:<tweetId>:<index>", "reddit:<postId>:<index>"
    pub key: String,
    /// URL du média (image, gif, mp4)
    pub url: String,
    /// Piste audio séparée à fusionner (vidéos Reddit DASH)
    pub audio_url: Option<String>,
    /// Nom de fichier sans extension ; jetons "{w}" / "{h}" remplacés par le core
    pub name: String,
    pub referer: Option<String>,
}

pub struct SourceCtx {
    /// UA Chrome desktop, gzip/brotli ; pas de cookie store
    pub http: reqwest::Client,
    pub session: Option<crate::auth::Session>,
    /// 0 = illimité
    pub limit: u32,
    /// Dossier final du job (existe déjà)
    pub dest_dir: std::path::PathBuf,
    pub cancel: tokio_util::sync::CancellationToken,
    pub log: JobLogger,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct JobLog<'a> {
    job_id: &'a str,
    level: &'a str,
    message: String,
    ts: i64,
}

/// Émet les événements `job-log` d'un job.
#[derive(Clone)]
pub struct JobLogger {
    app: AppHandle,
    job_id: String,
}

impl JobLogger {
    pub fn new(app: AppHandle, job_id: impl Into<String>) -> Self {
        Self { app, job_id: job_id.into() }
    }

    fn emit(&self, level: &str, message: String) {
        let payload = JobLog { job_id: &self.job_id, level, message, ts: now_ms() };
        let _ = self.app.emit("job-log", payload);
    }

    pub fn info(&self, msg: impl Into<String>) {
        self.emit("info", msg.into());
    }

    pub fn warn(&self, msg: impl Into<String>) {
        self.emit("warn", msg.into());
    }

    pub fn error(&self, msg: impl Into<String>) {
        self.emit("error", msg.into());
    }
}

/// Horodatage ms epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
