//! Appels ffmpeg (sidecar à côté de l'exe, sinon PATH) + optimisation oxipng.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;

use anyhow::{bail, Context};
use regex::Regex;
use serde::Serialize;
use tokio::process::Command;

use crate::settings::{ImageFormat, VideoFormat};

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
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
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
async fn run(bin: &Path, args: &[OsString]) -> anyhow::Result<()> {
    let out = command(bin)
        .args(args)
        .output()
        .await
        .context("lancement de ffmpeg")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("erreur inconnue");
        bail!("ffmpeg : {}", last.trim());
    }
    Ok(())
}

#[tauri::command]
pub async fn ffmpeg_status() -> Result<FfmpegStatus, String> {
    let Some(bin) = resolve() else {
        return Ok(FfmpegStatus {
            available: false,
            path: None,
            version: None,
        });
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

/// Extension finale d'une image.
pub fn image_ext(f: ImageFormat) -> &'static str {
    match f {
        ImageFormat::Png => "png",
        ImageFormat::Jpg => "jpg",
        ImageFormat::Webp => "webp",
        ImageFormat::Avif => "avif",
    }
}

/// Extension finale d'une vidéo.
pub fn video_ext(f: VideoFormat) -> &'static str {
    match f {
        VideoFormat::H264 | VideoFormat::H265 => "mp4",
        VideoFormat::Vp9 | VideoFormat::Av1 => "webm",
    }
}

/// Qualité 50..=100 → `-q:v` mjpeg (2 = meilleur, 31 = pire).
pub fn jpg_qscale(quality: u32) -> u32 {
    let q = quality.clamp(1, 100) as f64;
    ((31.0 - (q - 1.0) * 29.0 / 99.0).round() as u32).clamp(2, 31)
}

/// Qualité 50..=100 → CRF libaom (0 = meilleur, 63 = pire).
pub fn avif_crf(quality: u32) -> u32 {
    ((63.0 - quality as f64 * 0.55).round().max(0.0) as u32).min(63)
}

/// CRF de base (x264) → CRF de l'encodeur du format, borné à sa plage valide.
pub fn video_crf_for(format: VideoFormat, crf: u32) -> u32 {
    match format {
        VideoFormat::H264 => crf.min(51),
        VideoFormat::H265 => (crf + 5).min(51),
        VideoFormat::Vp9 => (crf + 10).min(63),
        VideoFormat::Av1 => (crf + 12).min(63),
    }
}

/// Image quelconque → format choisi (1re frame, sans métadonnées).
/// PNG : optimisé par oxipng si demandé. Renvoie les dimensions de l'image produite.
pub async fn to_image(
    bin: &Path,
    input: &Path,
    output: &Path,
    format: ImageFormat,
    quality: u32,
    optimize: bool,
) -> anyhow::Result<(u32, u32)> {
    let quality = quality.clamp(50, 100);
    let mut args: Vec<OsString> = ["-y", "-hide_banner", "-loglevel", "error", "-i"]
        .map(OsString::from)
        .into();
    args.push(input.into());
    args.extend(["-frames:v", "1", "-map_metadata", "-1"].map(OsString::from));
    let codec: Vec<String> = match format {
        ImageFormat::Png => ["-c:v", "png", "-compression_level", "9", "-pred", "mixed"]
            .map(String::from)
            .into(),
        ImageFormat::Jpg => vec![
            "-c:v".into(),
            "mjpeg".into(),
            "-q:v".into(),
            jpg_qscale(quality).to_string(),
            "-pix_fmt".into(),
            "yuvj420p".into(),
        ],
        ImageFormat::Webp => vec![
            "-c:v".into(),
            "libwebp".into(),
            "-quality".into(),
            quality.to_string(),
            "-compression_level".into(),
            "6".into(),
        ],
        ImageFormat::Avif => vec![
            "-c:v".into(),
            "libaom-av1".into(),
            "-still-picture".into(),
            "1".into(),
            "-crf".into(),
            avif_crf(quality).to_string(),
            "-b:v".into(),
            "0".into(),
            "-cpu-used".into(),
            "6".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
        ],
    };
    args.extend(codec.into_iter().map(OsString::from));
    args.push(output.into());
    run(bin, &args).await?;
    if format == ImageFormat::Png {
        if optimize {
            let path = output.to_path_buf();
            // Un échec d'oxipng n'est pas bloquant : on garde le PNG de ffmpeg
            let _ = tokio::task::spawn_blocking(move || optimize_png(&path)).await;
        }
        png_dimensions(output).await
    } else {
        video_dimensions(bin, output)
            .await
            .context("dimensions de l'image illisibles")
    }
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

/// Vidéo ou GIF animé → format vidéo choisi.
/// `audio` : piste séparée ajoutée en 2e entrée. `force_reencode` : obligatoire pour les GIF.
/// `reencode` ne concerne que H.264 (copie de flux possible) ; les autres formats sont toujours ré-encodés.
#[allow(clippy::too_many_arguments)]
pub async fn to_video(
    bin: &Path,
    input: &Path,
    audio: Option<&Path>,
    output: &Path,
    format: VideoFormat,
    reencode: bool,
    crf: u32,
    force_reencode: bool,
) -> anyhow::Result<()> {
    match format {
        VideoFormat::H264 => {
            let reencode = reencode || force_reencode;
            let res = video_pass(bin, input, audio, output, format, reencode, crf, false).await;
            match res {
                // Copie de flux impossible (codec incompatible MP4) → ré-encodage
                Err(_) if !reencode => {
                    video_pass(bin, input, audio, output, format, true, crf, false).await
                }
                other => other,
            }
        }
        VideoFormat::Av1 => {
            let res = video_pass(bin, input, audio, output, format, true, crf, false).await;
            match res {
                // libsvtav1 absent ou en échec → libaom-av1
                Err(_) => video_pass(bin, input, audio, output, format, true, crf, true).await,
                other => other,
            }
        }
        _ => video_pass(bin, input, audio, output, format, true, crf, false).await,
    }
}

/// Arguments d'encodage vidéo/audio d'un format (ré-encodage). `aom` : AV1 via libaom.
fn encode_args(format: VideoFormat, crf: u32, aom: bool) -> Vec<String> {
    let c = video_crf_for(format, crf).to_string();
    let mut a: Vec<String> = Vec::new();
    let mut push = |xs: &[&str]| a.extend(xs.iter().map(|x| x.to_string()));
    match format {
        VideoFormat::H264 => push(&["-c:v", "libx264", "-crf", &c, "-preset", "medium"]),
        VideoFormat::H265 => push(&[
            "-c:v", "libx265", "-crf", &c, "-preset", "medium", "-tag:v", "hvc1",
        ]),
        VideoFormat::Vp9 => push(&[
            "-c:v",
            "libvpx-vp9",
            "-crf",
            &c,
            "-b:v",
            "0",
            "-row-mt",
            "1",
            "-deadline",
            "good",
            "-cpu-used",
            "4",
        ]),
        VideoFormat::Av1 if aom => push(&[
            "-c:v",
            "libaom-av1",
            "-crf",
            &c,
            "-b:v",
            "0",
            "-cpu-used",
            "6",
            "-row-mt",
            "1",
        ]),
        VideoFormat::Av1 => push(&["-c:v", "libsvtav1", "-crf", &c, "-preset", "8"]),
    }
    push(&[
        "-pix_fmt",
        "yuv420p",
        "-vf",
        "scale=trunc(iw/2)*2:trunc(ih/2)*2",
    ]);
    match format {
        VideoFormat::H264 | VideoFormat::H265 => {
            push(&["-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"])
        }
        VideoFormat::Vp9 | VideoFormat::Av1 => push(&["-c:a", "libopus", "-b:a", "128k"]),
    }
    a
}

#[allow(clippy::too_many_arguments)]
async fn video_pass(
    bin: &Path,
    input: &Path,
    audio: Option<&Path>,
    output: &Path,
    format: VideoFormat,
    reencode: bool,
    crf: u32,
    aom: bool,
) -> anyhow::Result<()> {
    let mut args: Vec<OsString> = ["-y", "-hide_banner", "-loglevel", "error", "-i"]
        .map(OsString::from)
        .into();
    args.push(input.into());
    if let Some(a) = audio {
        args.push("-i".into());
        args.push(a.into());
        args.extend(["-map", "0:v:0", "-map", "1:a:0", "-shortest", "-dn"].map(OsString::from));
    }
    if reencode {
        args.extend(encode_args(format, crf, aom).into_iter().map(OsString::from));
    } else {
        args.extend(["-c", "copy", "-movflags", "+faststart"].map(OsString::from));
    }
    args.push(output.into());
    run(bin, &args).await
}

/// Dimensions d'une vidéo, lues dans la sortie de `ffmpeg -i`.
pub async fn video_dimensions(bin: &Path, path: &Path) -> Option<(u32, u32)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r"Video:.*?\b(\d{2,5})x(\d{2,5})\b").expect("regex valide"));
    // Sans fichier de sortie ffmpeg échoue, mais affiche les infos du flux
    let out = command(bin)
        .arg("-hide_banner")
        .arg("-i")
        .arg(path)
        .output()
        .await
        .ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let caps = re.captures(&text)?;
    Some((caps[1].parse().ok()?, caps[2].parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions() {
        assert_eq!(image_ext(ImageFormat::Png), "png");
        assert_eq!(image_ext(ImageFormat::Jpg), "jpg");
        assert_eq!(image_ext(ImageFormat::Webp), "webp");
        assert_eq!(image_ext(ImageFormat::Avif), "avif");
        assert_eq!(video_ext(VideoFormat::H264), "mp4");
        assert_eq!(video_ext(VideoFormat::H265), "mp4");
        assert_eq!(video_ext(VideoFormat::Vp9), "webm");
        assert_eq!(video_ext(VideoFormat::Av1), "webm");
    }

    #[test]
    fn image_quality_mapping() {
        assert_eq!(jpg_qscale(100), 2);
        assert_eq!(jpg_qscale(50), 17);
        assert_eq!(jpg_qscale(0), 31);
        assert_eq!(avif_crf(100), 8);
        assert_eq!(avif_crf(85), 16);
        assert_eq!(avif_crf(50), 36);
    }

    #[test]
    fn crf_per_codec() {
        assert_eq!(video_crf_for(VideoFormat::H264, 23), 23);
        assert_eq!(video_crf_for(VideoFormat::H265, 23), 28);
        assert_eq!(video_crf_for(VideoFormat::H265, 35), 40);
        assert_eq!(video_crf_for(VideoFormat::Vp9, 23), 33);
        assert_eq!(video_crf_for(VideoFormat::Av1, 23), 35);
        assert_eq!(video_crf_for(VideoFormat::Av1, 60), 63);
        assert_eq!(video_crf_for(VideoFormat::H265, 60), 51);
    }

    #[test]
    fn webm_has_no_faststart() {
        assert!(!encode_args(VideoFormat::Vp9, 23, false).contains(&"+faststart".to_string()));
        assert!(encode_args(VideoFormat::H265, 23, false).contains(&"hvc1".to_string()));
        assert!(encode_args(VideoFormat::Av1, 23, true).contains(&"libaom-av1".to_string()));
    }
}
