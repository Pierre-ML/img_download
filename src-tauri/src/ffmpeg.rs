//! Appels ffmpeg (sidecar à côté de l'exe, sinon PATH) + optimisation oxipng.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;

use anyhow::{bail, Context};
use regex::Regex;
use serde::Serialize;
use tokio::process::Command;

#[cfg(windows)]
const EXE_NAME: &str = "ffmpeg.exe";
#[cfg(not(windows))]
const EXE_NAME: &str = "ffmpeg";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegStatus {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

/// Cherche ffmpeg à côté de l'exécutable, puis dans le PATH.
pub fn resolve() -> Option<PathBuf> {
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        let p = dir.join(EXE_NAME);
        if p.is_file() {
            return Some(p);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(EXE_NAME))
        .find(|p| p.is_file())
}

/// Commande sans fenêtre console, tuée si le futur est abandonné.
fn command(bin: &Path) -> Command {
    let mut cmd = Command::new(bin);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

/// Lance ffmpeg ; en cas d'échec renvoie la dernière ligne d'erreur.
async fn run(bin: &Path, args: &[&std::ffi::OsStr]) -> anyhow::Result<()> {
    let out = command(bin).args(args).output().await.context("lancement de ffmpeg")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("erreur inconnue");
        bail!("ffmpeg : {}", last.trim());
    }
    Ok(())
}

#[tauri::command]
pub async fn ffmpeg_status() -> Result<FfmpegStatus, String> {
    let Some(bin) = resolve() else {
        return Ok(FfmpegStatus { available: false, path: None, version: None });
    };
    let mut cmd = command(&bin);
    cmd.stdout(Stdio::piped()).arg("-version");
    let version = match cmd.output().await {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines().next().map(|l| {
                l.strip_prefix("ffmpeg version ")
                    .and_then(|r| r.split_whitespace().next())
                    .unwrap_or(l)
                    .to_string()
            })
        }
        _ => None,
    };
    Ok(FfmpegStatus {
        available: version.is_some(),
        path: Some(bin.to_string_lossy().into_owned()),
        version,
    })
}

/// Image quelconque → PNG (1re frame, sans métadonnées), optimisé par oxipng si demandé.
/// Renvoie les dimensions lues dans l'en-tête IHDR.
pub async fn to_png(bin: &Path, input: &Path, output: &Path, optimize: bool) -> anyhow::Result<(u32, u32)> {
    let args: Vec<&std::ffi::OsStr> = vec![
        "-y".as_ref(), "-hide_banner".as_ref(), "-loglevel".as_ref(), "error".as_ref(),
        "-i".as_ref(), input.as_os_str(),
        "-frames:v".as_ref(), "1".as_ref(), "-map_metadata".as_ref(), "-1".as_ref(),
        "-c:v".as_ref(), "png".as_ref(), "-compression_level".as_ref(), "9".as_ref(),
        "-pred".as_ref(), "mixed".as_ref(),
        output.as_os_str(),
    ];
    run(bin, &args).await?;
    if optimize {
        let path = output.to_path_buf();
        // Un échec d'oxipng n'est pas bloquant : on garde le PNG de ffmpeg
        let _ = tokio::task::spawn_blocking(move || optimize_png(&path)).await;
    }
    png_dimensions(output).await
}

fn optimize_png(path: &Path) -> anyhow::Result<()> {
    let data = std::fs::read(path)?;
    let mut opts = oxipng::Options::from_preset(2);
    opts.strip = oxipng::StripChunks::Safe;
    let out = oxipng::optimize_from_memory(&data, &opts)?;
    if out.len() < data.len() {
        std::fs::write(path, out)?;
    }
    Ok(())
}

/// Largeur/hauteur depuis le chunk IHDR (octets 16..24).
async fn png_dimensions(path: &Path) -> anyhow::Result<(u32, u32)> {
    use tokio::io::AsyncReadExt;
    let mut head = [0u8; 24];
    let mut f = tokio::fs::File::open(path).await?;
    f.read_exact(&mut head).await.context("PNG tronqué")?;
    if &head[..8] != b"\x89PNG\r\n\x1a\n" || &head[12..16] != b"IHDR" {
        bail!("PNG invalide");
    }
    let w = u32::from_be_bytes([head[16], head[17], head[18], head[19]]);
    let h = u32::from_be_bytes([head[20], head[21], head[22], head[23]]);
    Ok((w, h))
}

/// Vidéo ou GIF animé → MP4 (H.264 + AAC, faststart).
/// `audio` : piste séparée ajoutée en 2e entrée. `force_reencode` : obligatoire pour les GIF.
pub async fn to_mp4(
    bin: &Path,
    input: &Path,
    audio: Option<&Path>,
    output: &Path,
    reencode: bool,
    crf: u32,
    force_reencode: bool,
) -> anyhow::Result<()> {
    let reencode = reencode || force_reencode;
    let res = mp4_pass(bin, input, audio, output, reencode, crf).await;
    match res {
        // Copie de flux impossible (codec incompatible MP4) → ré-encodage
        Err(_) if !reencode => mp4_pass(bin, input, audio, output, true, crf).await,
        other => other,
    }
}

async fn mp4_pass(
    bin: &Path,
    input: &Path,
    audio: Option<&Path>,
    output: &Path,
    reencode: bool,
    crf: u32,
) -> anyhow::Result<()> {
    let crf = crf.to_string();
    let mut args: Vec<&std::ffi::OsStr> = vec![
        "-y".as_ref(), "-hide_banner".as_ref(), "-loglevel".as_ref(), "error".as_ref(),
        "-i".as_ref(), input.as_os_str(),
    ];
    if let Some(a) = audio {
        args.extend::<[&std::ffi::OsStr; 8]>([
            "-i".as_ref(), a.as_os_str(),
            "-map".as_ref(), "0:v:0".as_ref(), "-map".as_ref(), "1:a:0".as_ref(),
            "-shortest".as_ref(), "-dn".as_ref(),
        ]);
    }
    if reencode {
        args.extend::<[&std::ffi::OsStr; 16]>([
            "-c:v".as_ref(), "libx264".as_ref(), "-crf".as_ref(), crf.as_ref(),
            "-preset".as_ref(), "medium".as_ref(), "-pix_fmt".as_ref(), "yuv420p".as_ref(),
            "-vf".as_ref(), "scale=trunc(iw/2)*2:trunc(ih/2)*2".as_ref(),
            "-c:a".as_ref(), "aac".as_ref(), "-b:a".as_ref(), "128k".as_ref(),
            "-movflags".as_ref(), "+faststart".as_ref(),
        ]);
    } else {
        args.extend::<[&std::ffi::OsStr; 4]>([
            "-c".as_ref(), "copy".as_ref(), "-movflags".as_ref(), "+faststart".as_ref(),
        ]);
    }
    args.push(output.as_os_str());
    run(bin, &args).await
}

/// Dimensions d'une vidéo, lues dans la sortie de `ffmpeg -i`.
pub async fn video_dimensions(bin: &Path, path: &Path) -> Option<(u32, u32)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"Video:.*?\b(\d{2,5})x(\d{2,5})\b").expect("regex valide")
    });
    // Sans fichier de sortie ffmpeg échoue, mais affiche les infos du flux
    let out = command(bin).arg("-hide_banner").arg("-i").arg(path).output().await.ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let caps = re.captures(&text)?;
    Some((caps[1].parse().ok()?, caps[2].parse().ok()?))
}
