//! File d'attente : un seul job à la fois, événements `job-update` throttlés.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{mpsc, Notify};
use tokio_util::sync::CancellationToken;

use crate::auth::{self, Provider};
use crate::pipeline::{self, sanitize_filename};
use crate::settings;
use crate::sources::{self, now_ms, JobLogger, SourceCtx};

const RECENT_MAX: usize = 500;
const CHANNEL_CAP: usize = 32;
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
(KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36";

// ── Types exposés au frontend ──────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum JobRequest {
    Google { query: String, limit: u32 },
    XLikes { limit: u32 },
    RedditSaved { limit: u32 },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    Google,
    XLikes,
    RedditSaved,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Error,
    Cancelled,
}

impl JobStatus {
    fn finished(self) -> bool {
        matches!(self, Self::Done | Self::Error | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: String,
    pub kind: JobKind,
    pub label: String,
    pub status: JobStatus,
    pub found: u32,
    pub saved: u32,
    pub skipped: u32,
    pub failed: u32,
    pub limit: u32,
    pub error: Option<String>,
    pub dest_dir: String,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Video,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSaved {
    pub job_id: String,
    pub path: String,
    pub kind: MediaKind,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bytes: u64,
}

// ── État interne ───────────────────────────────────────────────────────────

struct JobEntry {
    info: JobInfo,
    request: JobRequest,
    cancel: CancellationToken,
    dirty: bool,
}

pub struct Inner {
    jobs: Mutex<Vec<JobEntry>>,
    recent: Mutex<VecDeque<MediaSaved>>,
    wake: Notify,
    http: reqwest::Client,
    pub store: Arc<DownloadedStore>,
}

#[derive(Clone)]
pub struct QueueState(Arc<Inner>);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Clés déjà téléchargées, persistées dans <app_data_dir>/downloaded.json.
pub struct DownloadedStore {
    path: PathBuf,
    inner: Mutex<StoreInner>,
}

struct StoreInner {
    keys: HashSet<String>,
    unsaved: usize,
}

impl DownloadedStore {
    fn load(path: PathBuf) -> Self {
        let keys = std::fs::read(&path)
            .ok()
            .and_then(|raw| serde_json::from_slice::<HashSet<String>>(&raw).ok())
            .unwrap_or_default();
        Self {
            path,
            inner: Mutex::new(StoreInner { keys, unsaved: 0 }),
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        lock(&self.inner).keys.contains(key)
    }

    /// Ajoute une clé ; renvoie true s'il est temps de persister.
    pub fn insert(&self, key: String) -> bool {
        let mut g = lock(&self.inner);
        if g.keys.insert(key) {
            g.unsaved += 1;
        }
        g.unsaved >= 25
    }

    /// Écriture atomique, sous le verrou pour sérialiser les écrivains.
    pub fn flush(&self) {
        let mut g = lock(&self.inner);
        if g.unsaved == 0 {
            return;
        }
        let mut keys: Vec<&String> = g.keys.iter().collect();
        keys.sort();
        match serde_json::to_vec(&keys) {
            Ok(raw) => match settings::write_atomic(&self.path, &raw) {
                Ok(()) => g.unsaved = 0,
                Err(e) => eprintln!("downloaded.json : {e}"),
            },
            Err(e) => eprintln!("downloaded.json : {e}"),
        }
    }
}

/// Accès d'un job en cours à son état (compteurs, médias).
#[derive(Clone)]
pub struct JobHandle {
    app: AppHandle,
    q: Arc<Inner>,
    pub id: String,
}

impl JobHandle {
    /// Modifie le JobInfo ; l'émission est faite par le ticker (≤ 10/s).
    pub fn update(&self, f: impl FnOnce(&mut JobInfo)) {
        let mut jobs = lock(&self.q.jobs);
        if let Some(e) = jobs.iter_mut().find(|e| e.info.id == self.id) {
            f(&mut e.info);
            e.dirty = true;
        }
    }

    pub fn media_saved(&self, m: MediaSaved) {
        {
            let mut r = lock(&self.q.recent);
            r.push_front(m.clone());
            r.truncate(RECENT_MAX);
        }
        let _ = self.app.emit("media-saved", m);
    }

    pub fn store(&self) -> Arc<DownloadedStore> {
        self.q.store.clone()
    }

    /// Émet si le job a changé depuis la dernière émission.
    fn flush(&self) {
        let info = {
            let mut jobs = lock(&self.q.jobs);
            match jobs.iter_mut().find(|e| e.info.id == self.id) {
                Some(e) if e.dirty => {
                    e.dirty = false;
                    e.info.clone()
                }
                _ => return,
            }
        };
        let _ = self.app.emit("job-update", info);
    }
}

// ── Initialisation et runner ───────────────────────────────────────────────

pub fn init(app: &AppHandle) -> anyhow::Result<()> {
    let http = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .gzip(true)
        .brotli(true)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()?;
    let data_dir = app.path().app_data_dir()?;
    let store = Arc::new(DownloadedStore::load(data_dir.join("downloaded.json")));
    let inner = Arc::new(Inner {
        jobs: Mutex::new(Vec::new()),
        recent: Mutex::new(VecDeque::new()),
        wake: Notify::new(),
        http,
        store,
    });
    app.manage(QueueState(inner.clone()));
    tauri::async_runtime::spawn(runner(app.clone(), inner));
    Ok(())
}

fn emit_job(app: &AppHandle, info: &JobInfo) {
    let _ = app.emit("job-update", info.clone());
}

/// Boucle principale : prend le premier job en attente et l'exécute.
async fn runner(app: AppHandle, q: Arc<Inner>) {
    loop {
        let next = {
            let mut jobs = lock(&q.jobs);
            jobs.iter_mut()
                .find(|e| e.info.status == JobStatus::Queued)
                .map(|e| {
                    e.info.status = JobStatus::Running;
                    e.info.started_at = Some(now_ms());
                    e.dirty = false;
                    (e.info.clone(), e.request.clone(), e.cancel.clone())
                })
        };
        let Some((info, request, cancel)) = next else {
            q.wake.notified().await;
            continue;
        };
        emit_job(&app, &info);

        let handle = JobHandle {
            app: app.clone(),
            q: q.clone(),
            id: info.id.clone(),
        };
        let log = JobLogger::new(app.clone(), info.id.clone());
        log.info(format!("Démarrage : {}", info.label));

        // Ticker d'émission throttlée des compteurs
        let tick_stop = CancellationToken::new();
        let ticker = {
            let h = handle.clone();
            let stop = tick_stop.clone();
            tauri::async_runtime::spawn(async move {
                let mut iv = tokio::time::interval(Duration::from_millis(100));
                loop {
                    tokio::select! {
                        _ = stop.cancelled() => break,
                        _ = iv.tick() => h.flush(),
                    }
                }
            })
        };

        let result = run_job(
            &app,
            &q,
            &handle,
            &info,
            request,
            cancel.clone(),
            log.clone(),
        )
        .await;
        tick_stop.cancel();
        let _ = ticker.await;
        let store = q.store.clone();
        let _ = tokio::task::spawn_blocking(move || store.flush()).await;

        let final_info = {
            let mut jobs = lock(&q.jobs);
            jobs.iter_mut().find(|e| e.info.id == info.id).map(|e| {
                e.info.finished_at = Some(now_ms());
                e.dirty = false;
                if cancel.is_cancelled() {
                    e.info.status = JobStatus::Cancelled;
                } else if let Err(err) = &result {
                    e.info.status = JobStatus::Error;
                    e.info.error = Some(format!("{err:#}"));
                } else {
                    e.info.status = JobStatus::Done;
                }
                e.info.clone()
            })
        };
        if let Some(fi) = final_info {
            match fi.status {
                JobStatus::Cancelled => log.warn("Job annulé"),
                JobStatus::Error => log.error(fi.error.clone().unwrap_or_default()),
                _ => log.info(format!(
                    "Terminé : {} enregistré(s), {} ignoré(s), {} échec(s)",
                    fi.saved, fi.skipped, fi.failed
                )),
            }
            emit_job(&app, &fi);
        }
    }
}

/// Exécute un job : source (producteur) + pipeline (consommateur) en parallèle.
async fn run_job(
    app: &AppHandle,
    q: &Arc<Inner>,
    handle: &JobHandle,
    info: &JobInfo,
    request: JobRequest,
    cancel: CancellationToken,
    log: JobLogger,
) -> anyhow::Result<()> {
    let s = settings::current(app);
    let ffmpeg = crate::ffmpeg::resolve().ok_or_else(|| {
        anyhow::anyhow!("ffmpeg introuvable (ni à côté de l'application, ni dans le PATH)")
    })?;

    let dest_dir = PathBuf::from(&info.dest_dir);
    std::fs::create_dir_all(&dest_dir).map_err(|e| {
        anyhow::anyhow!(
            "Création du dossier {} impossible : {e}",
            dest_dir.display()
        )
    })?;
    settings::allow_dest(app, &dest_dir);

    let tmp_dir = app.path().app_cache_dir()?.join("jobs").join(&info.id);
    std::fs::create_dir_all(&tmp_dir)?;

    let session = match &request {
        JobRequest::Google { .. } => None,
        JobRequest::XLikes { .. } => auth::load_session(app, Provider::X)?,
        JobRequest::RedditSaved { .. } => auth::load_session(app, Provider::Reddit)?,
    };

    // Annulé par l'utilisateur (parent) ou par le pipeline quand la limite est atteinte
    let source_cancel = cancel.child_token();
    let ctx = SourceCtx {
        app: app.clone(),
        http: q.http.clone(),
        session,
        limit: info.limit,
        dest_dir: dest_dir.clone(),
        cancel: source_cancel.clone(),
        log: log.clone(),
    };
    let (tx, rx) = mpsc::channel(CHANNEL_CAP);

    let source = async {
        let fut = async {
            match &request {
                JobRequest::Google { query, .. } => sources::google::collect(&ctx, query, tx).await,
                JobRequest::XLikes { .. } => sources::x::collect_likes(&ctx, tx).await,
                JobRequest::RedditSaved { .. } => sources::reddit::collect_saved(&ctx, tx).await,
            }
        };
        tokio::select! {
            r = fut => r,
            _ = source_cancel.cancelled() => Ok(()),
        }
    };
    let cfg = pipeline::PipelineCfg {
        ffmpeg,
        dest_dir,
        tmp_dir: tmp_dir.clone(),
        parallel: s.parallel as usize,
        optimize_png: s.optimize_png,
        reencode_video: s.reencode_video,
        video_crf: s.video_crf,
        limit: info.limit,
    };
    let pipe = pipeline::run(
        cfg,
        q.http.clone(),
        handle.clone(),
        log.clone(),
        rx,
        source_cancel.clone(),
    );

    let (src_res, limit_reached) = tokio::join!(source, pipe);
    let _ = tokio::fs::remove_dir_all(&tmp_dir).await;

    match src_res {
        Err(e) if !limit_reached && !cancel.is_cancelled() => Err(e),
        _ => Ok(()),
    }
}

fn job_folder(dest: &Path, req: &JobRequest) -> PathBuf {
    match req {
        JobRequest::Google { query, .. } => dest
            .join("Google")
            .join(sanitize_filename(query, "recherche")),
        JobRequest::XLikes { .. } => dest.join("X").join("Likes"),
        JobRequest::RedditSaved { .. } => dest.join("Reddit").join("Saved"),
    }
}

// ── Commandes ──────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn queue_add(
    app: AppHandle,
    state: State<'_, QueueState>,
    request: JobRequest,
) -> Result<JobInfo, String> {
    let request = match request {
        JobRequest::Google { query, limit } => {
            let query = query.trim().to_string();
            if query.is_empty() {
                return Err("Mot-clé vide".into());
            }
            JobRequest::Google { query, limit }
        }
        other => other,
    };
    let (kind, label, limit) = match &request {
        JobRequest::Google { query, limit } => {
            (JobKind::Google, format!("Google : {query}"), *limit)
        }
        JobRequest::XLikes { limit } => (JobKind::XLikes, "Likes X".to_string(), *limit),
        JobRequest::RedditSaved { limit } => (
            JobKind::RedditSaved,
            "Reddit enregistrés".to_string(),
            *limit,
        ),
    };
    let s = settings::current(&app);
    let dest = job_folder(Path::new(&s.dest_dir), &request);
    let info = JobInfo {
        id: uuid::Uuid::new_v4().to_string(),
        kind,
        label,
        status: JobStatus::Queued,
        found: 0,
        saved: 0,
        skipped: 0,
        failed: 0,
        limit,
        error: None,
        dest_dir: dest.to_string_lossy().into_owned(),
        started_at: None,
        finished_at: None,
    };
    lock(&state.0.jobs).push(JobEntry {
        info: info.clone(),
        request,
        cancel: CancellationToken::new(),
        dirty: false,
    });
    emit_job(&app, &info);
    state.0.wake.notify_one();
    Ok(info)
}

#[tauri::command]
pub async fn queue_list(state: State<'_, QueueState>) -> Result<Vec<JobInfo>, String> {
    Ok(lock(&state.0.jobs).iter().map(|e| e.info.clone()).collect())
}

#[tauri::command]
pub async fn queue_cancel(
    app: AppHandle,
    state: State<'_, QueueState>,
    id: String,
) -> Result<(), String> {
    let changed = {
        let mut jobs = lock(&state.0.jobs);
        let Some(e) = jobs.iter_mut().find(|e| e.info.id == id) else {
            return Err("Job introuvable".into());
        };
        match e.info.status {
            JobStatus::Queued => {
                e.cancel.cancel();
                e.info.status = JobStatus::Cancelled;
                e.info.finished_at = Some(now_ms());
                Some(e.info.clone())
            }
            // Le runner passera le statut à "cancelled" à la fin
            JobStatus::Running => {
                e.cancel.cancel();
                None
            }
            _ => None,
        }
    };
    if let Some(info) = changed {
        emit_job(&app, &info);
    }
    Ok(())
}

#[tauri::command]
pub async fn queue_clear_finished(state: State<'_, QueueState>) -> Result<(), String> {
    lock(&state.0.jobs).retain(|e| !e.info.status.finished());
    Ok(())
}

#[tauri::command]
pub async fn recent_media(
    state: State<'_, QueueState>,
    limit: u32,
) -> Result<Vec<MediaSaved>, String> {
    let r = lock(&state.0.recent);
    let n = if limit == 0 {
        r.len()
    } else {
        (limit as usize).min(r.len())
    };
    Ok(r.iter().take(n).cloned().collect())
}

/// Annule tous les jobs (en attente et en cours) puis attend, au plus `timeout`, que plus rien ne
/// tourne : les ffmpeg lancés par les jobs sont tués à l'abandon des futurs (`kill_on_drop`).
/// Utilisé avant l'installation d'une mise à jour (ffmpeg.exe verrouille le dossier d'installation).
pub async fn cancel_all(app: &AppHandle, timeout: Duration) {
    let Some(state) = app.try_state::<QueueState>() else {
        return;
    };
    let q = state.0.clone();
    let changed: Vec<JobInfo> = {
        let mut jobs = lock(&q.jobs);
        let mut out = Vec::new();
        for e in jobs.iter_mut() {
            match e.info.status {
                JobStatus::Queued => {
                    e.cancel.cancel();
                    e.info.status = JobStatus::Cancelled;
                    e.info.finished_at = Some(now_ms());
                    out.push(e.info.clone());
                }
                JobStatus::Running => e.cancel.cancel(),
                _ => {}
            }
        }
        out
    };
    for info in &changed {
        emit_job(app, info);
    }
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let running = lock(&q.jobs)
            .iter()
            .any(|e| e.info.status == JobStatus::Running);
        if !running {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
