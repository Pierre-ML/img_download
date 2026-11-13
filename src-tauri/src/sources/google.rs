//! Source Google Images : vraie recherche Google dans une fenêtre webview
//! hors écran (rendue mais invisible pour l'utilisateur).
//!
//! Déroulé : recherche du mot-clé → onglet Images → défilement ; à chaque tour,
//! un script synchrone extrait les URLs pleine résolution du DOM et des
//! réponses réseau interceptées. Consentement UE refusé automatiquement ;
//! en cas de captcha, la fenêtre est ramenée à l'écran pour l'utilisateur.
//!
//! Sur Windows, les API webview qui attendent une réponse (eval avec
//! callback, getters) ne doivent jamais être appelées depuis le thread
//! principal : `collect` tourne dans une tâche tokio du runner de file.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use tokio::sync::{mpsc::Sender, oneshot};

use super::{MediaCandidate, SourceCtx};

const REFERER: &str = "https://www.google.com/";
const WIN_W: f64 = 1280.0;
const WIN_H: f64 = 900.0;
/// Position hors écran : la fenêtre reste « visible » (WebView2 continue de
/// rendre la page et de faire tourner ses timers) sans apparaître.
const OFFSCREEN: f64 = -32000.0;
const CAPTCHA_TITLE: &str = "Google — vérification requise";

const POLL_INTERVAL: Duration = Duration::from_millis(1200);
const EVAL_TIMEOUT: Duration = Duration::from_secs(10);
const GLOBAL_TIMEOUT: Duration = Duration::from_secs(4 * 60);
const CAPTCHA_TIMEOUT: Duration = Duration::from_secs(3 * 60);
/// Délai après le clic sur l'onglet Images avant de naviguer directement.
const TAB_CLICK_WAIT: Duration = Duration::from_secs(6);
/// Délai max pour atteindre la page de résultats Images.
const IMAGES_PAGE_TIMEOUT: Duration = Duration::from_secs(25);
const MAX_IDLE_ROUNDS: u32 = 8;
const MAX_EVAL_FAILURES: u32 = 8;
const MAX_CONSENT_CLICKS: u32 = 5;
const UNLIMITED_MAX_CANDIDATES: usize = 400;

static WINDOW_SEQ: AtomicU64 = AtomicU64::new(0);

// ── Scripts injectés ────────────────────────────────────────────────────────

/// Exécuté au début de chaque document : intercepte `fetch` et
/// `XMLHttpRequest` (hôtes Google) et garde le texte des réponses dans
/// `window.__mdl` (borné à 20 Mo, vidé à chaque lecture par `EXTRACT_JS`).
const INIT_JS: &str = r#"
(function () {
  if (window.__mdl) return;
  var MAX = 20 * 1024 * 1024;
  var buf = { parts: [], size: 0 };
  window.__mdl = buf;

  function wanted(u) {
    try {
      var h = new URL(String(u), location.href).hostname;
      return /(^|\.)google(\.[a-z]+){1,2}$/.test(h);
    } catch (e) { return false; }
  }
  function keep(text) {
    if (typeof text !== 'string' || text.length > MAX || text.indexOf('http') < 0) return;
    while (buf.parts.length && buf.size + text.length > MAX) buf.size -= buf.parts.shift().length;
    buf.parts.push(text);
    buf.size += text.length;
  }

  var origFetch = window.fetch;
  if (typeof origFetch === 'function') {
    window.fetch = function (input) {
      var p = origFetch.apply(this, arguments);
      try {
        var u = typeof input === 'string' ? input : (input && input.url) || '';
        if (wanted(u)) {
          p.then(function (r) {
            try { r.clone().text().then(keep, function () {}); } catch (e) {}
          }, function () {});
        }
      } catch (e) {}
      return p;
    };
  }

  var proto = XMLHttpRequest.prototype;
  var origOpen = proto.open, origSend = proto.send;
  proto.open = function (method, url) {
    try { this.__mdlUrl = String(url); } catch (e) {}
    return origOpen.apply(this, arguments);
  };
  proto.send = function () {
    var xhr = this;
    try {
      if (wanted(xhr.__mdlUrl || '')) {
        xhr.addEventListener('load', function () {
          try {
            if (xhr.responseType === '' || xhr.responseType === 'text') keep(xhr.responseText);
          } catch (e) {}
        });
      }
    } catch (e) {}
    return origSend.apply(this, arguments);
  };
})();
"#;

/// Fonctions communes, préfixées à chaque script évalué (voir `page_script`).
const HELPERS_JS: &str = r#"
function visible(el) {
  return !!(el && (el.offsetWidth || el.offsetHeight || el.getClientRects().length));
}
function label(el) {
  return String(el.innerText || el.value || el.getAttribute('aria-label') || '')
    .replace(/\s+/g, ' ').trim().toLowerCase();
}
function findByText(selector, texts) {
  var els = document.querySelectorAll(selector);
  for (var t = 0; t < texts.length; t++) {
    for (var i = 0; i < els.length; i++) {
      if (visible(els[i]) && label(els[i]) === texts[t]) return els[i];
    }
  }
  return null;
}
function consentButton() {
  var sel = 'button, input[type="submit"], input[type="button"], [role="button"]';
  return findByText(sel, ['tout refuser', 'refuser tout', 'reject all'])
    || findByText(sel, ['tout accepter', 'accepter tout', 'accept all', "j'accepte", 'j’accepte', 'i agree']);
}
function isConsent() {
  return /^consent\./.test(location.hostname) || !!consentButton();
}
function isCaptcha() {
  return location.pathname.indexOf('/sorry/') === 0
    || !!document.querySelector('#captcha-form, form[action*="/sorry/"], iframe[src*="recaptcha"]');
}
function isImagesPage() {
  return /[?&](udm=2|tbm=isch)(&|$)/.test(location.search);
}
function pageState() {
  return {
    url: location.href,
    ready: document.readyState !== 'loading',
    captcha: isCaptcha(),
    consent: isConsent()
  };
}
"#;

/// Extraction des URLs pleine résolution (nouvelles depuis le dernier appel)
/// et état de la page. Sources : HTML courant, réponses réseau interceptées,
/// liens `a[href*="imgurl="]`. Motif principal : `["https://…",<h>,<w>]`,
/// éventuellement échappé (`\"`, `=`, `\/`…) dans du JSON imbriqué.
const EXTRACT_JS: &str = r#"
var state = pageState();
var seen = window.__mdlSeen || (window.__mdlSeen = new Set());
var urls = [];

function decode(s) {
  return s
    .replace(/\\+u([0-9a-fA-F]{4})/g, function (_, h) { return String.fromCharCode(parseInt(h, 16)); })
    .replace(/\\+x([0-9a-fA-F]{2})/g, function (_, h) { return String.fromCharCode(parseInt(h, 16)); })
    .replace(/\\+(.)/g, '$1');
}
function keepUrl(u) {
  if (!/^https?:\/\//i.test(u)) return false;
  var h;
  try { h = new URL(u).hostname.toLowerCase(); } catch (e) { return false; }
  return !(/(^|\.)gstatic\.com$/.test(h)
    || /(^|\.)googleusercontent\.com$/.test(h)
    || /^encrypted-tbn/.test(h)
    || /(^|\.)google(\.[a-z]+){1,2}$/.test(h));
}
function add(raw) {
  var u = decode(raw);
  if (!keepUrl(u) || seen.has(u)) return;
  seen.add(u);
  urls.push(u);
}
function scan(text) {
  if (!text) return;
  var m;
  var arr = /\[\\*"(https?:(?:[^"\\]|\\+[^"\\])*)\\*",\s*(\d+),\s*(\d+)\]/g;
  while ((m = arr.exec(text))) {
    if (+m[2] < 80 && +m[3] < 80) continue; // vignette / icône
    add(m[1]);
  }
  var ou = /\\*"ou\\*":\s*\\*"(https?:(?:[^"\\]|\\+[^"\\])*)\\*"/g;
  while ((m = ou.exec(text))) add(m[1]);
}

var buf = window.__mdl;
var parts = buf ? buf.parts : [];
if (buf) { buf.parts = []; buf.size = 0; }

if (state.ready && isImagesPage()) {
  scan(document.documentElement ? document.documentElement.innerHTML : '');
  for (var i = 0; i < parts.length; i++) scan(parts[i]);
  var links = document.querySelectorAll('a[href*="imgurl="]');
  for (var j = 0; j < links.length; j++) {
    try {
      var v = new URL(links[j].href).searchParams.get('imgurl');
      if (v) add(v);
    } catch (e) {}
  }
}
state.urls = urls;
return state;
"#;

const STATE_JS: &str = "return pageState();";

const CONSENT_JS: &str = r#"
var b = consentButton();
if (b) b.click();
return { clicked: !!b };
"#;

/// Clic sur l'onglet « Images » de la page de résultats.
const IMAGES_TAB_JS: &str = r#"
var links = document.querySelectorAll('a[href]');
var pick = null;
for (var i = 0; i < links.length; i++) {
  var h = links[i].getAttribute('href') || '';
  if (/[?&](udm=2|tbm=isch)(&|$)/.test(h)) {
    if (visible(links[i])) { pick = links[i]; break; }
    if (!pick) pick = links[i];
  }
}
if (!pick) pick = findByText('a[href]', ['images']);
if (pick) pick.click();
return { clicked: !!pick };
"#;

/// Défilement en bas de page + clic sur « Plus de résultats » s'il est visible.
const SCROLL_JS: &str = r#"
var el = document.scrollingElement || document.documentElement;
window.scrollTo(0, el.scrollHeight);
var more = findByText('a, button, [role="button"], input[type="button"]', [
  'plus de résultats', 'afficher plus de résultats', 'voir plus de résultats',
  'more results', 'show more results'
]);
if (more) more.click();
return { clicked: !!more };
"#;

/// Script synchrone autonome : helpers + corps, exceptions capturées.
fn page_script(body: &str) -> String {
    format!(
        "(function () {{\n{HELPERS_JS}\ntry {{\n{body}\n}} catch (e) {{\n  return {{ error: String((e && e.message) || e) }};\n}}\n}})()"
    )
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PageState {
    url: String,
    ready: bool,
    captcha: bool,
    consent: bool,
    urls: Vec<String>,
    error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Clicked {
    clicked: bool,
}

// ── Collecte ────────────────────────────────────────────────────────────────

enum TabStep {
    Search,
    Clicked(Instant),
    Navigated(Instant),
}

pub async fn collect(ctx: &SourceCtx, query: &str, tx: Sender<MediaCandidate>) -> Result<()> {
    let query = query.trim();
    if query.is_empty() {
        bail!("Mot-clé de recherche vide");
    }
    let keyword = clean_keyword(query);
    let mut counter = max_existing_number(&ctx.dest_dir, &keyword);
    let max = max_candidates(ctx.limit);
    let search = search_url(query, false)?;
    let images = search_url(query, true)?;

    let extract_js = page_script(EXTRACT_JS);
    let state_js = page_script(STATE_JS);
    let consent_js = page_script(CONSENT_JS);
    let tab_js = page_script(IMAGES_TAB_JS);
    let scroll_js = page_script(SCROLL_JS);

    ctx.log
        .info(format!("Recherche Google Images : « {query} »"));
    let guard = WindowGuard(open_window(&ctx.app, search)?);
    let win = &guard.0;

    let mut deadline = Instant::now() + GLOBAL_TIMEOUT;
    let mut seen: HashSet<String> = HashSet::new();
    let mut sent = 0usize;
    let mut idle = 0u32;
    let mut failures = 0u32;
    let mut consent_clicks = 0u32;
    let mut tab = TabStep::Search;

    loop {
        if !pause(ctx, POLL_INTERVAL).await {
            return Ok(());
        }
        if Instant::now() >= deadline {
            ctx.log
                .info("Délai de recherche atteint (4 min) : fin de la collecte");
            break;
        }

        let state = match eval_json::<PageState>(win, &extract_js).await {
            Ok(s) if s.error.is_none() => s,
            res => {
                failures += 1;
                if failures >= MAX_EVAL_FAILURES {
                    let why = match res {
                        Ok(s) => s.error.unwrap_or_default(),
                        Err(e) => format!("{e:#}"),
                    };
                    bail!("La page Google ne répond plus : {why}");
                }
                continue;
            }
        };
        failures = 0;

        if state.captcha {
            deadline += solve_captcha(ctx, win, &state_js).await?;
            continue;
        }
        if state.consent {
            consent_clicks += 1;
            if consent_clicks > MAX_CONSENT_CLICKS {
                bail!("Impossible de passer la page de consentement de Google");
            }
            if consent_clicks == 1 {
                ctx.log.info("Consentement Google : refus des cookies");
            }
            let _ = eval_json::<Clicked>(win, &consent_js).await;
            continue;
        }
        if !state.ready {
            continue;
        }

        if !is_images_url(&state.url) {
            match tab {
                TabStep::Search => {
                    ctx.log.info("Ouverture de l'onglet Images");
                    let clicked = eval_json::<Clicked>(win, &tab_js)
                        .await
                        .is_ok_and(|c| c.clicked);
                    tab = if clicked {
                        TabStep::Clicked(Instant::now())
                    } else {
                        navigate(win, &images)?;
                        TabStep::Navigated(Instant::now())
                    };
                }
                TabStep::Clicked(t) if t.elapsed() >= TAB_CLICK_WAIT => {
                    navigate(win, &images)?;
                    tab = TabStep::Navigated(Instant::now());
                }
                TabStep::Navigated(t) if t.elapsed() >= IMAGES_PAGE_TIMEOUT => {
                    bail!(
                        "Impossible d'ouvrir Google Images (connexion réseau ?) — page : {}",
                        state.url
                    );
                }
                _ => {}
            }
            continue;
        }

        let mut new_count = 0usize;
        for u in state.urls {
            if sent >= max {
                break;
            }
            if !is_candidate(&u) || !seen.insert(u.clone()) {
                continue;
            }
            counter += 1;
            new_count += 1;
            sent += 1;
            let cand = MediaCandidate {
                key: format!("google:{u}"),
                url: u,
                audio_url: None,
                name: format!("{keyword}_{counter:03}_{{w}}x{{h}}"),
                referer: Some(REFERER.to_string()),
            };
            if tx.send(cand).await.is_err() {
                return Ok(());
            }
        }

        if new_count > 0 {
            idle = 0;
            ctx.log.info(format!("{sent} images trouvées"));
        } else {
            idle += 1;
        }
        if sent >= max || idle >= MAX_IDLE_ROUNDS {
            break;
        }
        let _ = eval_json::<Clicked>(win, &scroll_js).await;
    }

    if sent == 0 {
        ctx.log.warn(
            "Aucune image trouvée sur Google Images (aucun résultat, ou format de page changé)",
        );
    } else {
        ctx.log.info(format!(
            "Recherche Google terminée : {sent} images trouvées"
        ));
    }
    Ok(())
}

/// Attend que l'utilisateur résolve le captcha dans la fenêtre ramenée à
/// l'écran. Renvoie le temps passé (non décompté du délai global).
async fn solve_captcha(ctx: &SourceCtx, win: &WebviewWindow, state_js: &str) -> Result<Duration> {
    let started = Instant::now();
    ctx.log
        .warn("Google demande une vérification : résous-la dans la fenêtre ouverte");
    show_for_user(win);

    let outcome = loop {
        if !pause(ctx, Duration::from_millis(1500)).await {
            break Ok(());
        }
        if win.app_handle().get_webview_window(win.label()).is_none() {
            break Err(anyhow!(
                "Fenêtre de vérification Google fermée : recherche abandonnée"
            ));
        }
        if started.elapsed() >= CAPTCHA_TIMEOUT {
            break Err(anyhow!(
                "Vérification Google non résolue en 3 min : réessaie plus tard"
            ));
        }
        if let Ok(s) = eval_json::<PageState>(win, state_js).await {
            if s.error.is_none() && s.ready && !s.captcha {
                break Ok(());
            }
        }
    };

    hide_offscreen(win);
    outcome?;
    if !ctx.cancel.is_cancelled() {
        ctx.log
            .info("Vérification terminée : reprise de la recherche");
    }
    Ok(started.elapsed())
}

/// Attend `d` ; `false` si le job est annulé entre-temps.
async fn pause(ctx: &SourceCtx, d: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(d) => true,
        _ = ctx.cancel.cancelled() => false,
    }
}

// ── Fenêtre webview ─────────────────────────────────────────────────────────

/// Ferme la fenêtre Google dans tous les cas (fin, erreur, annulation :
/// le futur de la source est abandonné par `select!`).
struct WindowGuard(WebviewWindow);

impl Drop for WindowGuard {
    fn drop(&mut self) {
        let _ = self.0.destroy();
    }
}

fn open_window(app: &AppHandle, url: Url) -> Result<WebviewWindow> {
    let label = format!("google-{}", WINDOW_SEQ.fetch_add(1, Ordering::Relaxed) + 1);
    WebviewWindowBuilder::new(app, label, WebviewUrl::External(url))
        .title("Lucascrap — Google Images")
        .inner_size(WIN_W, WIN_H)
        .position(OFFSCREEN, OFFSCREEN)
        .decorations(false)
        .skip_taskbar(true)
        .focused(false)
        .visible(true)
        .initialization_script(INIT_JS)
        .build()
        .context("Ouverture de la fenêtre Google impossible")
}

fn navigate(win: &WebviewWindow, url: &Url) -> Result<()> {
    win.navigate(url.clone())
        .context("Navigation vers Google Images impossible")
}

fn show_for_user(win: &WebviewWindow) {
    let _ = win.set_title(CAPTCHA_TITLE);
    let _ = win.set_decorations(true);
    let _ = win.set_skip_taskbar(false);
    let _ = win.set_size(LogicalSize::new(1000.0, 780.0));
    let _ = win.center();
    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_always_on_top(true);
    let _ = win.set_focus();
}

fn hide_offscreen(win: &WebviewWindow) {
    let _ = win.set_always_on_top(false);
    let _ = win.set_decorations(false);
    let _ = win.set_skip_taskbar(true);
    let _ = win.set_size(LogicalSize::new(WIN_W, WIN_H));
    let _ = win.set_position(LogicalPosition::new(OFFSCREEN, OFFSCREEN));
}

/// Évalue un script synchrone et désérialise son résultat (JSON).
async fn eval_json<T: DeserializeOwned>(win: &WebviewWindow, js: &str) -> Result<T> {
    let (tx, rx) = oneshot::channel::<String>();
    let slot = Mutex::new(Some(tx));
    win.eval_with_callback(js, move |res| {
        if let Some(tx) = slot.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = tx.send(res);
        }
    })
    .context("exécution du script impossible")?;
    let raw = tokio::time::timeout(EVAL_TIMEOUT, rx)
        .await
        .map_err(|_| anyhow!("la page ne répond pas"))?
        .map_err(|_| anyhow!("script interrompu (navigation en cours ?)"))?;
    serde_json::from_str(&raw).context("réponse du script illisible")
}

// ── URLs ────────────────────────────────────────────────────────────────────

fn search_url(query: &str, images: bool) -> Result<Url> {
    let mut s = format!(
        "https://www.google.com/search?q={}&hl=fr",
        urlencoding::encode(query)
    );
    if images {
        s.push_str("&udm=2");
    }
    Url::parse(&s).context("URL de recherche invalide")
}

/// Page de résultats Images (`udm=2` ou ancien `tbm=isch`).
fn is_images_url(u: &str) -> bool {
    Url::parse(u).is_ok_and(|p| {
        p.query_pairs()
            .any(|(k, v)| (k == "udm" && v == "2") || (k == "tbm" && v == "isch"))
    })
}

/// Hôtes Google (`google.com`, `www.google.fr`, `google.co.uk`…).
fn is_google_host(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    labels
        .iter()
        .position(|l| *l == "google")
        .is_some_and(|i| (1..=2).contains(&(labels.len() - i - 1)))
}

/// Validation finale côté Rust (le script filtre déjà de la même façon).
fn is_candidate(u: &str) -> bool {
    let Ok(parsed) = Url::parse(u) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    let under = |d: &str| host == d || host.ends_with(&format!(".{d}"));
    !(under("gstatic.com")
        || under("googleusercontent.com")
        || host.starts_with("encrypted-tbn")
        || is_google_host(&host))
}

/// Nombre de candidats à collecter : ~2× la limite (marge pour les échecs).
fn max_candidates(limit: u32) -> usize {
    if limit == 0 {
        UNLIMITED_MAX_CANDIDATES
    } else {
        let l = limit as usize;
        l.saturating_mul(2).max(l + 10)
    }
}

// ── Nommage ─────────────────────────────────────────────────────────────────

/// minuscules, espaces → `_`, caractères non alphanumériques retirés (accents conservés).
fn clean_keyword(q: &str) -> String {
    let mut out = String::new();
    for c in q.trim().chars() {
        if c.is_whitespace() || c == '_' {
            if !out.ends_with('_') {
                out.push('_');
            }
        } else if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "image".to_string()
    } else {
        out
    }
}

/// Plus grand NNN existant dans des fichiers `<motclé>_<NNN>_…` du dossier.
fn max_existing_number(dir: &std::path::Path, keyword: &str) -> u32 {
    let prefix = format!("{keyword}_");
    let mut max = 0u32;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() || !rest[digits.len()..].starts_with('_') {
            continue;
        }
        if let Ok(n) = digits.parse::<u32>() {
            max = max.max(n);
        }
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_is_cleaned() {
        assert_eq!(clean_keyword("  Chat  Noir! "), "chat_noir");
        assert_eq!(clean_keyword("Été_à  Paris"), "été_à_paris");
        assert_eq!(clean_keyword("?!"), "image");
    }

    #[test]
    fn numbering_continues_from_existing_files() {
        let dir = std::env::temp_dir().join(format!("mdl-google-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in [
            "chat_007_800x600.jpg",
            "chat_012_1024x768.png",
            "chat_noir_099_10x10.jpg",
            "chat_abc_1x1.jpg",
            "chat_3.jpg",
        ] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        assert_eq!(max_existing_number(&dir, "chat"), 12);
        assert_eq!(max_existing_number(&dir, "chat_noir"), 99);
        assert_eq!(max_existing_number(&dir, "chien"), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn google_hosts_are_rejected() {
        assert!(is_candidate("https://example.com/a.jpg"));
        assert!(is_candidate("http://cdn.site.fr/img/b.png?x=1"));
        assert!(is_candidate("https://storage.googleapis.com/bucket/c.jpg"));
        assert!(!is_candidate(
            "https://encrypted-tbn0.gstatic.com/images?q=tbn"
        ));
        assert!(!is_candidate("https://www.gstatic.com/a.png"));
        assert!(!is_candidate("https://lh3.googleusercontent.com/a.jpg"));
        assert!(!is_candidate("https://www.google.com/logo.png"));
        assert!(!is_candidate("https://google.co.uk/x.jpg"));
        assert!(!is_candidate("data:image/png;base64,AAAA"));
        assert!(!is_candidate("not a url"));
    }

    #[test]
    fn images_page_detection() {
        assert!(is_images_url("https://www.google.com/search?q=chat&udm=2"));
        assert!(is_images_url(
            "https://www.google.fr/search?tbm=isch&q=chat"
        ));
        assert!(!is_images_url("https://www.google.com/search?q=chat&hl=fr"));
        assert!(!is_images_url(
            "https://consent.google.com/m?continue=x%26udm%3D2"
        ));
    }

    #[test]
    fn search_url_is_encoded() {
        let u = search_url("chat & chien", true).unwrap();
        assert_eq!(
            u.as_str(),
            "https://www.google.com/search?q=chat%20%26%20chien&hl=fr&udm=2"
        );
        assert!(is_images_url(u.as_str()));
        assert!(!is_images_url(search_url("chat", false).unwrap().as_str()));
    }

    #[test]
    fn candidate_budget() {
        assert_eq!(max_candidates(0), UNLIMITED_MAX_CANDIDATES);
        assert_eq!(max_candidates(1), 11);
        assert_eq!(max_candidates(50), 100);
    }

    #[test]
    fn scripts_are_wrapped() {
        let s = page_script(EXTRACT_JS);
        assert!(s.starts_with("(function () {"));
        assert!(s.ends_with("})()"));
        assert!(s.contains("function consentButton()"));
    }
}
