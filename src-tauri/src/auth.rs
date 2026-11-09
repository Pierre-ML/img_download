//! Comptes X / Reddit : connexion via une fenêtre webview intégrée, cookies
//! stockés chiffrés (AES-256-GCM) dans `<app_data_dir>/sessions.bin`.
//! La clé est conservée dans le trousseau OS (repli : `session.key`).
//! Aucune valeur de cookie n'est jamais loguée.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const KEYRING_SERVICE: &str = "com.mediadownloader.app";
const KEYRING_USER: &str = "session-key";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36";
const X_BEARER: &str = "AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";
const POLL_INTERVAL: Duration = Duration::from_millis(1500);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

// ── Types publics ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    X,
    Reddit,
}

impl Provider {
    fn login_label(self) -> &'static str {
        match self {
            Provider::X => "login-x",
            Provider::Reddit => "login-reddit",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub cookies: Vec<SessionCookie>,
    pub username: Option<String>,
    pub user_id: Option<String>,
    pub saved_at: i64,
}

impl Session {
    /// Valeur du cookie `name`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.cookies
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.value.as_str())
    }

    /// En-tête Cookie : `a=1; b=2`.
    pub fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub connected: bool,
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub x: AccountInfo,
    pub reddit: AccountInfo,
}

// ── Stockage chiffré ────────────────────────────────────────────────────────

#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    x: Option<Session>,
    #[serde(default)]
    reddit: Option<Session>,
}

impl Store {
    fn get(&self, p: Provider) -> Option<&Session> {
        match p {
            Provider::X => self.x.as_ref(),
            Provider::Reddit => self.reddit.as_ref(),
        }
    }
    fn set(&mut self, p: Provider, s: Option<Session>) {
        match p {
            Provider::X => self.x = s,
            Provider::Reddit => self.reddit = s,
        }
    }
    fn status(&self) -> AccountStatus {
        fn info(s: Option<&Session>) -> AccountInfo {
            AccountInfo {
                connected: s.is_some(),
                username: s.and_then(|s| s.username.clone()),
            }
        }
        AccountStatus {
            x: info(self.x.as_ref()),
            reddit: info(self.reddit.as_ref()),
        }
    }
}

/// Sérialise les cycles lecture-modification-écriture du fichier.
static STORE_LOCK: Mutex<()> = Mutex::new(());

fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("dossier de données introuvable")?;
    std::fs::create_dir_all(&dir).context("création du dossier de données")?;
    Ok(dir)
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, data).context("écriture du fichier temporaire")?;
    std::fs::rename(&tmp, path).context("remplacement du fichier")?;
    Ok(())
}

fn decode_key(b64: &str) -> Option<[u8; 32]> {
    let bytes = B64.decode(b64.trim()).ok()?;
    <[u8; 32]>::try_from(bytes.as_slice()).ok()
}

fn new_key() -> [u8; 32] {
    let mut k = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut k);
    k
}

fn key_from_file(dir: &Path) -> Result<[u8; 32]> {
    let path = dir.join("session.key");
    if let Ok(s) = std::fs::read_to_string(&path) {
        if let Some(k) = decode_key(&s) {
            return Ok(k);
        }
    }
    let key = new_key();
    atomic_write(&path, B64.encode(key).as_bytes())?;
    Ok(key)
}

/// Clé AES : trousseau OS en priorité, sinon fichier `session.key`.
fn load_or_create_key(dir: &Path) -> Result<[u8; 32]> {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[auth] trousseau indisponible ({e}) : repli sur session.key");
            return key_from_file(dir);
        }
    };
    match entry.get_password() {
        Ok(s) => {
            if let Some(k) = decode_key(&s) {
                return Ok(k);
            }
            eprintln!("[auth] clé du trousseau invalide : régénération");
        }
        Err(keyring::Error::NoEntry) => {}
        Err(e) => {
            eprintln!("[auth] lecture du trousseau impossible ({e}) : repli sur session.key");
            return key_from_file(dir);
        }
    }
    let key = new_key();
    match entry.set_password(&B64.encode(key)) {
        Ok(()) => Ok(key),
        Err(e) => {
            eprintln!("[auth] écriture du trousseau impossible ({e}) : repli sur session.key");
            key_from_file(dir)
        }
    }
}

fn read_store(app: &AppHandle) -> Result<Store> {
    let dir = data_dir(app)?;
    let path = dir.join("sessions.bin");
    let blob = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
        Err(e) => return Err(e).context("lecture de sessions.bin"),
    };
    if blob.len() < 13 {
        eprintln!("[auth] sessions.bin corrompu : ignoré");
        return Ok(Store::default());
    }
    let key = load_or_create_key(&dir)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow!("clé invalide : {e}"))?;
    let (nonce, ct) = blob.split_at(12);
    match cipher.decrypt(Nonce::from_slice(nonce), ct) {
        Ok(plain) => match serde_json::from_slice::<Store>(&plain) {
            Ok(s) => Ok(s),
            Err(e) => {
                eprintln!("[auth] sessions.bin illisible ({e}) : ignoré");
                Ok(Store::default())
            }
        },
        Err(_) => {
            eprintln!("[auth] déchiffrement impossible (clé changée ?) : sessions ignorées");
            Ok(Store::default())
        }
    }
}

fn write_store(app: &AppHandle, store: &Store) -> Result<()> {
    let dir = data_dir(app)?;
    let key = load_or_create_key(&dir)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow!("clé invalide : {e}"))?;
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let plain = serde_json::to_vec(store)?;
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
        .map_err(|_| anyhow!("échec du chiffrement"))?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    atomic_write(&dir.join("sessions.bin"), &out)
}

fn update_store(app: &AppHandle, f: impl FnOnce(&mut Store)) -> Result<Store> {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store = read_store(app)?;
    f(&mut store);
    write_store(app, &store)?;
    Ok(store)
}

pub fn load_session(app: &AppHandle, provider: Provider) -> Result<Option<Session>> {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let store = read_store(app)?;
    Ok(store.get(provider).cloned())
}

fn current_status(app: &AppHandle) -> AccountStatus {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    match read_store(app) {
        Ok(s) => s.status(),
        Err(e) => {
            eprintln!("[auth] lecture des sessions impossible : {e}");
            Store::default().status()
        }
    }
}

// ── Commandes ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn account_status(app: AppHandle) -> Result<AccountStatus, String> {
    Ok(current_status(&app))
}

#[tauri::command]
pub async fn account_login(app: AppHandle, provider: Provider) -> Result<AccountStatus, String> {
    let (title, login_url) = match provider {
        Provider::X => ("Connexion X", "https://x.com/i/flow/login"),
        Provider::Reddit => ("Connexion Reddit", "https://www.reddit.com/login/"),
    };
    let label = provider.login_label();

    if let Some(w) = app.get_webview_window(label) {
        let _ = w.show();
        let _ = w.set_focus();
    } else {
        let url: tauri::Url = login_url
            .parse()
            .map_err(|e| format!("URL invalide : {e}"))?;
        WebviewWindowBuilder::new(&app, label, WebviewUrl::External(url))
            .title(title)
            .inner_size(520.0, 760.0)
            .build()
            .map_err(|e| format!("Impossible d'ouvrir la fenêtre de connexion : {e}"))?;
    }

    // Les cookies sont lus depuis une tâche séparée : sur Windows,
    // `cookies_for_url` se bloque s'il est appelé depuis un handler synchrone.
    let app2 = app.clone();
    let handle = tauri::async_runtime::spawn(async move { poll_login(app2, provider).await });
    match handle.await {
        Ok(r) => r,
        Err(e) => Err(format!("Erreur interne : {e}")),
    }
}

#[tauri::command]
pub async fn account_logout(app: AppHandle, provider: Provider) -> Result<AccountStatus, String> {
    let store = update_store(&app, |s| s.set(provider, None)).map_err(|e| format!("{e:#}"))?;

    // Best effort : vider les cookies du webview pour qu'une reconnexion
    // ne soit pas instantanée avec l'ancienne session.
    if let Some(w) = app.get_webview_window(provider.login_label()) {
        let _ = w.close();
    }
    clear_webview_data(&app).await;

    let status = store.status();
    let _ = app.emit("account-changed", &status);
    Ok(status)
}

/// Vide les données de navigation du profil webview via une fenêtre invisible.
/// Si l'API échoue, on ignore (la session locale est déjà supprimée).
async fn clear_webview_data(app: &AppHandle) {
    const LABEL: &str = "clear-data";
    let window = match app.get_webview_window(LABEL) {
        Some(w) => w,
        None => {
            let url: tauri::Url = match "about:blank".parse() {
                Ok(u) => u,
                Err(_) => return,
            };
            match WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url))
                .visible(false)
                .build()
            {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("[auth] nettoyage du webview ignoré : {e}");
                    return;
                }
            }
        }
    };
    tokio::time::sleep(Duration::from_millis(400)).await;
    let w2 = window.clone();
    let res = tauri::async_runtime::spawn_blocking(move || w2.clear_all_browsing_data()).await;
    match res {
        Ok(Ok(())) => {}
        Ok(Err(e)) => eprintln!("[auth] clear_all_browsing_data a échoué : {e}"),
        Err(e) => eprintln!("[auth] clear_all_browsing_data interrompu : {e}"),
    }
    let _ = window.close();
}

// ── Boucle de connexion ─────────────────────────────────────────────────────

async fn poll_login(app: AppHandle, provider: Provider) -> Result<AccountStatus, String> {
    let label = provider.login_label();
    let (cookie_url, session_ok): (&str, fn(&[SessionCookie]) -> bool) = match provider {
        Provider::X => ("https://x.com/", |c| has(c, "auth_token") && has(c, "ct0")),
        Provider::Reddit => ("https://www.reddit.com/", |c| has(c, "reddit_session")),
    };
    let url: tauri::Url = cookie_url
        .parse()
        .map_err(|e| format!("URL invalide : {e}"))?;
    let deadline = Instant::now() + LOGIN_TIMEOUT;

    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        let Some(window) = app.get_webview_window(label) else {
            return Err("Connexion annulée".into());
        };
        if Instant::now() >= deadline {
            let _ = window.close();
            return Err("Délai de connexion dépassé".into());
        }

        let cookies: Vec<SessionCookie> = match window.cookies_for_url(url.clone()) {
            Ok(list) => list
                .iter()
                .map(|c| SessionCookie {
                    name: c.name().to_string(),
                    value: c.value().to_string(),
                    domain: c.domain().unwrap_or("").to_string(),
                })
                .collect(),
            Err(_) => continue,
        };
        if !session_ok(&cookies) {
            continue;
        }

        let mut session = Session {
            cookies,
            username: None,
            user_id: None,
            saved_at: now_secs(),
        };
        if provider == Provider::X {
            session.user_id = session.get("twid").and_then(parse_twid);
        }
        session.username = fetch_username(provider, &session).await;

        let store = update_store(&app, |s| s.set(provider, Some(session)))
            .map_err(|e| format!("Sauvegarde de la session impossible : {e:#}"))?;
        let _ = window.close();
        let status = store.status();
        let _ = app.emit("account-changed", &status);
        return Ok(status);
    }
}

fn has(cookies: &[SessionCookie], name: &str) -> bool {
    cookies
        .iter()
        .any(|c| c.name == name && !c.value.is_empty())
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `u%3D123` ou `"u=123"` → `123`.
fn parse_twid(raw: &str) -> Option<String> {
    let decoded = urlencoding::decode(raw).ok()?.into_owned();
    let trimmed = decoded.trim().trim_matches('"');
    let id = trimmed.strip_prefix("u=").unwrap_or(trimmed);
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
        Some(id.to_string())
    } else {
        None
    }
}

async fn fetch_username(provider: Provider, session: &Session) -> Option<String> {
    let client = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(15))
        .build()
        .ok()?;
    let cookie = session.cookie_header();
    match provider {
        Provider::X => {
            let ct0 = session.get("ct0")?;
            let resp = client
                .get("https://api.x.com/1.1/account/settings.json")
                .header("authorization", format!("Bearer {X_BEARER}"))
                .header("x-csrf-token", ct0)
                .header("x-twitter-auth-type", "OAuth2Session")
                .header("cookie", cookie)
                .send()
                .await
                .ok()?;
            if !resp.status().is_success() {
                return None;
            }
            let v: serde_json::Value = resp.json().await.ok()?;
            v.get("screen_name")?.as_str().map(str::to_string)
        }
        Provider::Reddit => {
            let resp = client
                .get("https://www.reddit.com/api/me.json")
                .header("cookie", cookie)
                .header("accept", "application/json")
                .send()
                .await
                .ok()?;
            if !resp.status().is_success() {
                return None;
            }
            let v: serde_json::Value = resp.json().await.ok()?;
            v.pointer("/data/name")
                .or_else(|| v.get("name"))?
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        }
    }
}
