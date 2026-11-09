//! Source Google Images : requêtes HTTP (sans navigateur), extraction des URLs
//! pleine résolution embarquées dans le HTML/JS de la page de résultats.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{bail, Result};
use regex::Regex;

use super::{MediaCandidate, SourceCtx};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36";
const REFERER: &str = "https://www.google.com/";
const MAX_PAGES: u32 = 30;
const DEFAULT_MAX_CANDIDATES: usize = 300;

fn re_array() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"\["(https?://(?:[^"\\]|\\.)*)",\s*(\d+),\s*(\d+)\]"#).expect("regex tableau")
    })
}

fn re_ou() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#""ou":"((?:[^"\\]|\\.)*)""#).expect("regex ou"))
}

pub async fn collect(
    ctx: &SourceCtx,
    query: &str,
    tx: tokio::sync::mpsc::Sender<MediaCandidate>,
) -> Result<()> {
    let keyword = clean_keyword(query);
    let counter = AtomicU32::new(max_existing_number(&ctx.dest_dir, &keyword));
    let max_candidates = if ctx.limit == 0 {
        DEFAULT_MAX_CANDIDATES
    } else {
        (ctx.limit as usize).saturating_mul(2)
    };
    let q = urlencoding::encode(query.trim()).into_owned();
    ctx.log
        .info(format!("Recherche Google Images : « {} »", query.trim()));

    let mut seen: HashSet<String> = HashSet::new();
    let mut sent = 0usize;

    for page in 0..MAX_PAGES {
        if ctx.cancel.is_cancelled() {
            return Ok(());
        }
        let url = format!(
            "https://www.google.com/search?q={q}&tbm=isch&hl=fr&ijn={page}&start={}",
            page * 100
        );
        let resp = ctx
            .http
            .get(&url)
            .header("user-agent", UA)
            .header(
                "accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("accept-language", "fr-FR,fr;q=0.9,en;q=0.8")
            .header("cookie", "CONSENT=YES+")
            .send()
            .await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) if page > 0 => {
                ctx.log.warn(format!(
                    "Page {page} : erreur réseau ({e}), arrêt de la collecte"
                ));
                break;
            }
            Err(e) => bail!("Requête Google impossible : {e}"),
        };
        let status = resp.status();
        if !status.is_success() {
            if page == 0 {
                bail!("Google a répondu {status} (captcha ou limitation ? réessaie plus tard)");
            }
            ctx.log.warn(format!(
                "Page {page} : Google a répondu {status}, arrêt de la collecte"
            ));
            break;
        }
        let html = match resp.text().await {
            Ok(t) => t,
            Err(e) if page > 0 => {
                ctx.log
                    .warn(format!("Page {page} : lecture impossible ({e})"));
                break;
            }
            Err(e) => bail!("Lecture de la réponse Google impossible : {e}"),
        };

        let found = extract_urls(&html);
        let mut new_count = 0usize;
        for u in found {
            if !seen.insert(u.clone()) {
                continue;
            }
            new_count += 1;
            let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
            let cand = MediaCandidate {
                key: format!("google:{u}"),
                url: u,
                audio_url: None,
                name: format!("{keyword}_{n:03}_{{w}}x{{h}}"),
                referer: Some(REFERER.to_string()),
            };
            sent += 1;
            if tx.send(cand).await.is_err() {
                return Ok(());
            }
            if sent >= max_candidates {
                break;
            }
        }
        ctx.log.info(format!(
            "Page {page} : {new_count} nouvelles images ({sent} au total)"
        ));

        if page == 0 && new_count == 0 {
            ctx.log.warn(
                "Aucune image trouvée sur la première page : Google a peut-être renvoyé une page de \
                 consentement ou un captcha (ou a changé son format).",
            );
        }
        if new_count == 0 || sent >= max_candidates {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(600)) => {}
            _ = ctx.cancel.cancelled() => return Ok(()),
        }
    }
    Ok(())
}

// ── Extraction ──────────────────────────────────────────────────────────────

fn extract_urls(html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut push = |raw: &str| {
        let u = unescape_js(raw);
        if is_candidate(&u) && seen.insert(u.clone()) {
            out.push(u);
        }
    };
    for c in re_array().captures_iter(html) {
        let h: u32 = c[2].parse().unwrap_or(0);
        let w: u32 = c[3].parse().unwrap_or(0);
        if h < 80 && w < 80 {
            continue; // vignette / icône
        }
        push(&c[1]);
    }
    for c in re_ou().captures_iter(html) {
        push(&c[1]);
    }
    out
}

fn is_candidate(u: &str) -> bool {
    let Ok(parsed) = url::Url::parse(u) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_lowercase();
    !(host.contains("gstatic") || host.contains("google") || host.starts_with("encrypted-tbn"))
}

/// Décode les échappements JS : `\uXXXX`, `\xXX`, `\/`, `\\`, `\"`.
fn unescape_js(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '\\' || i + 1 >= chars.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let n = chars[i + 1];
        match n {
            'u' if i + 6 <= chars.len() => {
                let hex: String = chars[i + 2..i + 6].iter().collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                    i += 6;
                } else {
                    out.push(c);
                    i += 1;
                }
            }
            'x' if i + 4 <= chars.len() => {
                let hex: String = chars[i + 2..i + 4].iter().collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                    i += 4;
                } else {
                    out.push(c);
                    i += 1;
                }
            }
            '/' | '\\' | '"' | '\'' => {
                out.push(n);
                i += 2;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
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
