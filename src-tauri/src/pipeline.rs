//! Pipeline de téléchargement : dédup → téléchargement → conversion ffmpeg → nommage final.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{bail, Context};
use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::ffmpeg;
use crate::queue::{DownloadedStore, JobHandle, MediaKind, MediaSaved};
use crate::sources::{JobLogger, MediaCandidate};

const MAX_NAME: usize = 120;

pub struct PipelineCfg {
    pub ffmpeg: PathBuf,
    pub dest_dir: PathBuf,
    pub tmp_dir: PathBuf,
    pub parallel: usize,
    pub optimize_png: bool,
    pub reencode_video: bool,
    pub video_crf: u32,
    pub limit: u32,
}

/// État partagé entre les téléchargements concurrents d'un job.
#[derive(Default)]
struct Shared {
    seen_urls: HashSet<String>,
    seen_hashes: HashSet<[u8; 32]>,
    /// Chemins finaux déjà attribués (minuscules, FS insensible à la casse)
    reserved: HashSet<String>,
    saved: u32,
}

struct Ctx {
    cfg: PipelineCfg,
    http: reqwest::Client,
    job: JobHandle,
    log: JobLogger,
    store: Arc<DownloadedStore>,
    shared: Mutex<Shared>,
}

impl Ctx {
    fn shared(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn limit_reached(&self, sh: &Shared) -> bool {
        self.cfg.limit > 0 && sh.saved >= self.cfg.limit
    }
}

enum Outcome {
    Saved,
    Skipped,
    /// Abandonné car la limite est atteinte
    Discarded,
}

/// Consomme les candidats avec `parallel` téléchargements simultanés.
/// Renvoie true si la limite a été atteinte (la source est alors annulée via `stop`).
pub async fn run(
    cfg: PipelineCfg,
    http: reqwest::Client,
    job: JobHandle,
    log: JobLogger,
    rx: mpsc::Receiver<MediaCandidate>,
    stop: CancellationToken,
) -> bool {
    let parallel = cfg.parallel.max(1);
    let ctx = Arc::new(Ctx {
        store: job.store(),
        cfg,
        http,
        job,
        log,
        shared: Mutex::new(Shared::default()),
    });

    let candidates = futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|c| (c, rx)) });
    let results = candidates
        .map(|c| {
            let ctx = ctx.clone();
            ctx.job.update(|i| i.found += 1);
            async move {
                let r = process(&ctx, &c).await;
                (c, r)
            }
        })
        .buffer_unordered(parallel);
    let mut results = std::pin::pin!(results);

    let mut limit_reached = false;
    loop {
        let item = tokio::select! {
            _ = stop.cancelled() => break,
            it = results.next() => it,
        };
        let Some((c, res)) = item else { break };
        match res {
            Ok(Outcome::Saved) | Ok(Outcome::Discarded) => {}
            Ok(Outcome::Skipped) => ctx.job.update(|i| i.skipped += 1),
            Err(e) => {
                ctx.job.update(|i| i.failed += 1);
                ctx.log.warn(format!("Échec {} : {e:#}", short(&c.url)));
            }
        }
        if ctx.limit_reached(&ctx.shared()) {
            limit_reached = true;
            ctx.log.info(format!("Limite de {} atteinte", ctx.cfg.limit));
            stop.cancel();
            break;
        }
    }
    // Au retour, le stream est détruit : drop du receiver + abandon des téléchargements en cours
    limit_reached
}

/// Supprime les fichiers temporaires d'un candidat, même si le futur est abandonné.
struct TmpFiles(Vec<PathBuf>);

impl Drop for TmpFiles {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

enum Kind {
    Image,
    /// `forced` : GIF / WebP animé, ré-encodage obligatoire
    Video { forced: bool },
}

async fn process(ctx: &Ctx, c: &MediaCandidate) -> anyhow::Result<Outcome> {
    if ctx.store.contains(&c.key) {
        return Ok(Outcome::Skipped);
    }
    {
        let mut sh = ctx.shared();
        if ctx.limit_reached(&sh) {
            return Ok(Outcome::Discarded);
        }
        if !sh.seen_urls.insert(c.url.clone()) {
            return Ok(Outcome::Skipped);
        }
    }

    let id = uuid::Uuid::new_v4().simple().to_string();
    let tmp = &ctx.cfg.tmp_dir;
    let raw = tmp.join(format!("{id}.src"));
    let audio_path = tmp.join(format!("{id}.audio"));
    let png = tmp.join(format!("{id}.png"));
    let mp4 = tmp.join(format!("{id}.mp4"));
    let _guard = TmpFiles(vec![raw.clone(), audio_path.clone(), png.clone(), mp4.clone()]);

    download(&ctx.http, &c.url, c.referer.as_deref(), &raw).await?;
    let ff = &ctx.cfg.ffmpeg;

    let (out, kind, dims) = match classify(&raw).await? {
        Kind::Image => {
            let d = ffmpeg::to_png(ff, &raw, &png, ctx.cfg.optimize_png).await?;
            (png.clone(), MediaKind::Image, Some(d))
        }
        Kind::Video { forced } => {
            let audio = match &c.audio_url {
                Some(a) => match download(&ctx.http, a, c.referer.as_deref(), &audio_path).await {
                    Ok(_) => Some(audio_path.as_path()),
                    Err(e) => {
                        ctx.log.warn(format!("Audio indisponible pour {} : {e:#}", short(&c.url)));
                        None
                    }
                },
                None => None,
            };
            let res = ffmpeg::to_mp4(ff, &raw, audio, &mp4, ctx.cfg.reencode_video, ctx.cfg.video_crf, forced).await;
            match res {
                Ok(()) => {
                    let d = ffmpeg::video_dimensions(ff, &mp4).await;
                    (mp4.clone(), MediaKind::Video, d)
                }
                // Animation non décodable : on garde la 1re image en PNG
                Err(_) if forced => {
                    let d = ffmpeg::to_png(ff, &raw, &png, ctx.cfg.optimize_png).await?;
                    (png.clone(), MediaKind::Image, Some(d))
                }
                Err(e) => return Err(e),
            }
        }
    };

    let hash = sha256_file(&out).await?;
    let bytes = tokio::fs::metadata(&out).await?.len();
    let ext = if kind == MediaKind::Image { "png" } else { "mp4" };

    // Réservation du nom final et du compteur sous verrou
    let reserved = {
        let mut sh = ctx.shared();
        if ctx.limit_reached(&sh) {
            return Ok(Outcome::Discarded);
        }
        if sh.seen_hashes.insert(hash) {
            let stem = final_stem(&c.name, kind, dims);
            let path = unique_path(&ctx.cfg.dest_dir, &stem, ext, &sh.reserved);
            sh.reserved.insert(path.to_string_lossy().to_lowercase());
            sh.saved += 1;
            Some(path)
        } else {
            None
        }
    };
    let Some(final_path) = reserved else {
        // Contenu identique déjà enregistré dans ce job
        remember(ctx, &c.key).await;
        return Ok(Outcome::Skipped);
    };
    ctx.job.update(|i| i.saved += 1);

    if let Err(e) = move_file(&out, &final_path).await {
        ctx.shared().saved -= 1;
        ctx.job.update(|i| i.saved = i.saved.saturating_sub(1));
        return Err(e).context("écriture du fichier final");
    }
    remember(ctx, &c.key).await;

    ctx.job.media_saved(MediaSaved {
        job_id: ctx.job.id.clone(),
        path: final_path.to_string_lossy().into_owned(),
        kind,
        width: dims.map(|d| d.0),
        height: dims.map(|d| d.1),
        bytes,
    });
    Ok(Outcome::Saved)
}

/// Mémorise la clé dans downloaded.json (écriture groupée).
async fn remember(ctx: &Ctx, key: &str) {
    if ctx.store.insert(key.to_string()) {
        let store = ctx.store.clone();
        let _ = tokio::task::spawn_blocking(move || store.flush()).await;
    }
}

async fn download(http: &reqwest::Client, url: &str, referer: Option<&str>, dest: &Path) -> anyhow::Result<u64> {
    let mut req = http.get(url);
    if let Some(r) = referer {
        req = req.header(reqwest::header::REFERER, r);
    }
    let mut resp = req.send().await?.error_for_status()?;
    let is_html = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/html"));
    if is_html {
        bail!("page HTML reçue au lieu d'un média");
    }
    let mut f = tokio::fs::File::create(dest).await?;
    let mut n = 0u64;
    while let Some(chunk) = resp.chunk().await? {
        f.write_all(&chunk).await?;
        n += chunk.len() as u64;
    }
    f.flush().await?;
    if n == 0 {
        bail!("fichier vide");
    }
    Ok(n)
}

/// Type réel d'après les magic bytes.
async fn classify(path: &Path) -> anyhow::Result<Kind> {
    let mut head = Vec::with_capacity(8192);
    tokio::fs::File::open(path).await?.take(8192).read_to_end(&mut head).await?;
    let Some(t) = infer::get(&head) else {
        bail!("format non reconnu");
    };
    let mime = t.mime_type();
    if mime == "image/gif" {
        let data = tokio::fs::read(path).await?;
        return Ok(if gif_is_animated(&data) { Kind::Video { forced: true } } else { Kind::Image });
    }
    if mime == "image/webp" && webp_is_animated(&head) {
        return Ok(Kind::Video { forced: true });
    }
    if mime.starts_with("image/") {
        return Ok(Kind::Image);
    }
    if mime.starts_with("video/") {
        return Ok(Kind::Video { forced: false });
    }
    bail!("type non pris en charge : {mime}")
}

/// Vrai si le GIF contient au moins deux images.
fn gif_is_animated(d: &[u8]) -> bool {
    fn skip_blocks(d: &[u8], mut i: usize) -> usize {
        while i < d.len() {
            let n = d[i] as usize;
            i += 1;
            if n == 0 {
                break;
            }
            i += n;
        }
        i
    }
    if d.len() < 13 {
        return false;
    }
    let mut i = 13;
    if d[10] & 0x80 != 0 {
        i += 3 * (1usize << ((d[10] & 7) + 1));
    }
    let mut frames = 0;
    while i < d.len() {
        match d[i] {
            0x21 => i = skip_blocks(d, i + 2),
            0x2C => {
                if i + 10 > d.len() {
                    break;
                }
                let packed = d[i + 9];
                i += 10;
                if packed & 0x80 != 0 {
                    i += 3 * (1usize << ((packed & 7) + 1));
                }
                i = skip_blocks(d, i + 1); // +1 : taille de code LZW
                frames += 1;
                if frames > 1 {
                    return true;
                }
            }
            _ => break, // 0x3B (fin) ou données invalides
        }
    }
    false
}

/// WebP étendu (VP8X) avec le drapeau d'animation.
fn webp_is_animated(d: &[u8]) -> bool {
    d.len() > 20 && &d[0..4] == b"RIFF" && &d[8..12] == b"WEBP" && &d[12..16] == b"VP8X" && d[20] & 0x02 != 0
}

async fn sha256_file(path: &Path) -> anyhow::Result<[u8; 32]> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> anyhow::Result<[u8; 32]> {
        use std::io::Read;
        let mut f = std::fs::File::open(&path)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(hasher.finalize().into())
    })
    .await?
}

async fn move_file(from: &Path, to: &Path) -> anyhow::Result<()> {
    if tokio::fs::rename(from, to).await.is_ok() {
        return Ok(());
    }
    // Volumes différents : copie puis suppression
    tokio::fs::copy(from, to).await?;
    let _ = tokio::fs::remove_file(from).await;
    Ok(())
}

/// Remplace les jetons {w}/{h} (retire "_{w}x{h}" pour une vidéo) puis nettoie.
fn final_stem(name: &str, kind: MediaKind, dims: Option<(u32, u32)>) -> String {
    let mut s = name.to_string();
    if kind == MediaKind::Video {
        s = s.replace("_{w}x{h}", "").replace("{w}x{h}", "");
    }
    let (w, h) = dims.unwrap_or((0, 0));
    s = s.replace("{w}", &w.to_string()).replace("{h}", &h.to_string());
    sanitize_filename(&s, "media")
}

/// Nom de fichier sûr sous Windows : caractères interdits, espaces, longueur, noms réservés.
pub fn sanitize_filename(name: &str, fallback: &str) -> String {
    const FORBIDDEN: &str = "\\/:*?\"<>|";
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || FORBIDDEN.contains(c) { '_' } else { c })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated: String = collapsed.chars().take(MAX_NAME).collect();
    let trimmed = truncated.trim_end_matches([' ', '.', '_']);
    if trimmed.is_empty() {
        return fallback.to_string();
    }
    let base = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((base.starts_with("COM") || base.starts_with("LPT"))
            && base.len() == 4
            && base.as_bytes()[3].is_ascii_digit());
    if reserved {
        format!("_{trimmed}")
    } else {
        trimmed.to_string()
    }
}

/// Chemin libre : stem.ext, stem_2.ext, stem_3.ext…
fn unique_path(dir: &Path, stem: &str, ext: &str, reserved: &HashSet<String>) -> PathBuf {
    let taken = |p: &Path| p.exists() || reserved.contains(&p.to_string_lossy().to_lowercase());
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while taken(&p) {
        p = dir.join(format!("{stem}_{n}.{ext}"));
        n += 1;
    }
    p
}

fn short(url: &str) -> String {
    if url.chars().count() > 90 {
        format!("{}…", url.chars().take(90).collect::<String>())
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_filename("a/b:c?  d. ", "x"), "a_b_c_ d");
        assert_eq!(sanitize_filename("  ...", "x"), "x");
        assert_eq!(sanitize_filename("con", "x"), "_con");
        assert_eq!(sanitize_filename(&"é".repeat(200), "x").chars().count(), 120);
    }

    #[test]
    fn stems() {
        let n = "chat_001_{w}x{h}";
        assert_eq!(final_stem(n, MediaKind::Image, Some((640, 480))), "chat_001_640x480");
        assert_eq!(final_stem(n, MediaKind::Video, Some((640, 480))), "chat_001");
    }
}
