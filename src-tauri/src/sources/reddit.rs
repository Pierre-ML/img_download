//! Source Reddit : posts enregistrés du compte connecté (cookies de session).

use std::time::Duration;

use anyhow::{bail, Result};
use reqwest::{RequestBuilder, Response, StatusCode};
use serde_json::Value;

use super::{MediaCandidate, SourceCtx};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36";
const PAGE_PAUSE: Duration = Duration::from_millis(500);
const MAX_RETRIES_429: u32 = 3;
const IMG_EXTS: [&str; 5] = [".jpg", ".jpeg", ".png", ".webp", ".gif"];

pub async fn collect_saved(
    ctx: &SourceCtx,
    tx: tokio::sync::mpsc::Sender<MediaCandidate>,
) -> Result<()> {
    let Some(session) = ctx.session.as_ref() else {
        bail!("Compte Reddit non connecté");
    };
    let cookie = session.cookie_header();
    let token = session.get("token_v2").map(str::to_string);

    let username = match session.username.clone().filter(|u| !u.is_empty()) {
        Some(u) => u,
        None => fetch_me(ctx, &cookie).await?,
    };
    let user_enc = urlencoding::encode(&username).into_owned();
    ctx.log
        .info(format!("Collecte des posts enregistrés de u/{username}…"));

    let mut redgifs_token: Option<String> = None;
    let mut use_oauth = false;
    let mut after: Option<String> = None;
    let mut page = 0u32;
    let mut total = 0usize;

    loop {
        if ctx.cancel.is_cancelled() {
            return Ok(());
        }
        page += 1;

        let (status, body) = {
            let r = fetch_page(
                ctx,
                &user_enc,
                after.as_deref(),
                &cookie,
                token.as_deref(),
                use_oauth,
            )
            .await?;
            if !use_oauth
                && matches!(r.0, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
                && token.is_some()
            {
                ctx.log
                    .warn("Accès web refusé, repli sur l'API OAuth (token_v2)");
                use_oauth = true;
                fetch_page(
                    ctx,
                    &user_enc,
                    after.as_deref(),
                    &cookie,
                    token.as_deref(),
                    true,
                )
                .await?
            } else {
                r
            }
        };
        if !status.is_success() {
            if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
                bail!("Reddit a refusé l'accès ({status}) : reconnecte ton compte Reddit");
            }
            bail!("Reddit a répondu {status}");
        }

        let children = body
            .pointer("/data/children")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if children.is_empty() {
            break;
        }

        let mut page_media = 0usize;
        for child in &children {
            if ctx.cancel.is_cancelled() {
                return Ok(());
            }
            if child.get("kind").and_then(Value::as_str) != Some("t3") {
                continue; // commentaire enregistré
            }
            let Some(post) = child.get("data") else {
                continue;
            };
            for cand in post_candidates(ctx, post, &mut redgifs_token).await {
                page_media += 1;
                if tx.send(cand).await.is_err() {
                    return Ok(());
                }
            }
        }
        total += page_media;
        ctx.log.info(format!(
            "Page {page} : {} posts, {page_media} médias ({total} au total)",
            children.len()
        ));

        after = body
            .pointer("/data/after")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if after.is_none() {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(PAGE_PAUSE) => {}
            _ = ctx.cancel.cancelled() => return Ok(()),
        }
    }
    ctx.log
        .info(format!("Collecte terminée : {total} médias trouvés"));
    Ok(())
}

// ── Requêtes ────────────────────────────────────────────────────────────────

/// Envoie la requête ; sur 429 attend (retry-after ou 5 s) et réessaie 3 fois.
async fn send_retry(ctx: &SourceCtx, build: impl Fn() -> RequestBuilder) -> Result<Response> {
    let mut attempt = 0;
    loop {
        let resp = build().send().await?;
        if resp.status() != StatusCode::TOO_MANY_REQUESTS || attempt >= MAX_RETRIES_429 {
            return Ok(resp);
        }
        attempt += 1;
        let wait = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(5)
            .clamp(1, 120);
        ctx.log
            .warn(format!("Reddit limite le débit (429), attente {wait} s…"));
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(wait)) => {}
            _ = ctx.cancel.cancelled() => bail!("Annulé"),
        }
    }
}

async fn fetch_me(ctx: &SourceCtx, cookie: &str) -> Result<String> {
    let resp = send_retry(ctx, || {
        ctx.http
            .get("https://www.reddit.com/api/me.json")
            .header("user-agent", UA)
            .header("cookie", cookie)
            .header("accept", "application/json")
    })
    .await?;
    if !resp.status().is_success() {
        bail!(
            "Impossible de déterminer le nom d'utilisateur Reddit ({})",
            resp.status()
        );
    }
    let v: Value = resp.json().await?;
    match v
        .pointer("/data/name")
        .or_else(|| v.get("name"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        Some(n) => Ok(n.to_string()),
        None => bail!("Nom d'utilisateur Reddit introuvable : reconnecte ton compte"),
    }
}

async fn fetch_page(
    ctx: &SourceCtx,
    user_enc: &str,
    after: Option<&str>,
    cookie: &str,
    token: Option<&str>,
    oauth: bool,
) -> Result<(StatusCode, Value)> {
    let mut url = if oauth {
        format!("https://oauth.reddit.com/user/{user_enc}/saved?limit=100&raw_json=1")
    } else {
        format!("https://www.reddit.com/user/{user_enc}/saved.json?limit=100&raw_json=1")
    };
    if let Some(a) = after {
        url.push_str("&after=");
        url.push_str(&urlencoding::encode(a));
    }
    let resp = send_retry(ctx, || {
        let rb = ctx
            .http
            .get(&url)
            .header("user-agent", UA)
            .header("accept", "application/json");
        match (oauth, token) {
            (true, Some(t)) => rb.header("authorization", format!("Bearer {t}")),
            _ => rb.header("cookie", cookie),
        }
    })
    .await?;
    let status = resp.status();
    if !status.is_success() {
        return Ok((status, Value::Null));
    }
    let body: Value = resp.json().await?;
    Ok((status, body))
}

// ── Extraction des médias ───────────────────────────────────────────────────

struct Media {
    url: String,
    audio: Option<String>,
}

fn s<'a>(v: &'a Value, ptr: &str) -> Option<&'a str> {
    v.pointer(ptr)
        .and_then(Value::as_str)
        .filter(|x| !x.is_empty())
}

fn unescape(u: &str) -> String {
    u.replace("&amp;", "&")
}

fn strip_query(u: &str) -> &str {
    u.split(['?', '#']).next().unwrap_or(u)
}

fn has_img_ext(u: &str) -> bool {
    let p = strip_query(u).to_lowercase();
    IMG_EXTS.iter().any(|e| p.ends_with(e))
}

fn host_of(u: &str) -> String {
    url::Url::parse(u)
        .ok()
        .and_then(|x| x.host_str().map(str::to_lowercase))
        .unwrap_or_default()
}

async fn post_candidates(
    ctx: &SourceCtx,
    post: &Value,
    redgifs_token: &mut Option<String>,
) -> Vec<MediaCandidate> {
    let Some(id) = s(post, "/id") else {
        return vec![];
    };
    let title = clean_title(s(post, "/title").unwrap_or(""), id);
    // Crosspost : les médias sont portés par le post d'origine.
    let src = post
        .pointer("/crosspost_parent_list/0")
        .filter(|v| v.is_object())
        .unwrap_or(post);

    let medias = extract_media(ctx, src, redgifs_token).await;
    let multi = medias.len() > 1;
    medias
        .into_iter()
        .enumerate()
        .map(|(i, m)| MediaCandidate {
            key: format!("reddit:{id}:{i}"),
            url: m.url,
            audio_url: m.audio,
            name: if multi {
                format!("{title}_{:02}", i + 1)
            } else {
                title.clone()
            },
            referer: Some("https://www.reddit.com/".to_string()),
        })
        .collect()
}

fn clean_title(raw: &str, id: &str) -> String {
    let t: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let t: String = t.chars().take(100).collect();
    if t.trim().is_empty() {
        format!("reddit_{id}")
    } else {
        t
    }
}

async fn extract_media(
    ctx: &SourceCtx,
    post: &Value,
    redgifs_token: &mut Option<String>,
) -> Vec<Media> {
    // 1. Galerie
    if post.get("is_gallery").and_then(Value::as_bool) == Some(true) {
        if let Some(meta) = post.get("media_metadata").and_then(Value::as_object) {
            let ids: Vec<String> = match post
                .pointer("/gallery_data/items")
                .and_then(Value::as_array)
            {
                Some(items) => items
                    .iter()
                    .filter_map(|it| {
                        it.get("media_id")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .collect(),
                None => meta.keys().cloned().collect(),
            };
            let out: Vec<Media> = ids
                .iter()
                .filter_map(|mid| {
                    let m = meta.get(mid)?;
                    if let Some(st) = m.get("status").and_then(Value::as_str) {
                        if st != "valid" {
                            return None;
                        }
                    }
                    // Animés : mp4 > gif ; statiques : u
                    let u = s(m, "/s/mp4")
                        .or_else(|| s(m, "/s/u"))
                        .or_else(|| s(m, "/s/gif"))?;
                    Some(Media {
                        url: unescape(u),
                        audio: None,
                    })
                })
                .collect();
            if !out.is_empty() {
                return out;
            }
        }
    }

    let url = s(post, "/url").map(unescape).unwrap_or_default();
    let host = host_of(&url);

    // 2. Vidéo Reddit native
    let rv = post
        .pointer("/secure_media/reddit_video")
        .or_else(|| post.pointer("/media/reddit_video"))
        .filter(|v| v.is_object());
    if let Some(rv) = rv {
        if let Some(fb) = s(rv, "/fallback_url") {
            let video = strip_query(fb).to_string();
            let is_gif = rv.get("is_gif").and_then(Value::as_bool) == Some(true);
            let audio = if is_gif {
                None
            } else {
                find_audio(ctx, &video).await
            };
            return vec![Media { url: video, audio }];
        }
    }

    // 3. Imgur .gifv → .mp4
    if host.ends_with("imgur.com") && strip_query(&url).to_lowercase().ends_with(".gifv") {
        let p = strip_query(&url);
        return vec![Media {
            url: format!("{}mp4", &p[..p.len() - 4]),
            audio: None,
        }];
    }

    // 4. Image directe
    if host == "i.redd.it" || (host == "i.imgur.com" && has_img_ext(&url)) || has_img_ext(&url) {
        return vec![Media { url, audio: None }];
    }

    // 5. Redgifs
    if host.ends_with("redgifs.com") {
        let id = strip_query(&url)
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .split('.')
            .next()
            .unwrap_or("")
            .to_lowercase();
        if !id.is_empty() {
            match redgifs_url(ctx, redgifs_token, &id).await {
                Ok(u) => {
                    return vec![Media {
                        url: u,
                        audio: None,
                    }]
                }
                Err(e) => ctx.log.warn(format!("Redgifs {id} ignoré : {e}")),
            }
        }
        return vec![];
    }

    // 6. Aperçu vidéo
    if let Some(fb) = s(post, "/preview/reddit_video_preview/fallback_url") {
        return vec![Media {
            url: strip_query(fb).to_string(),
            audio: None,
        }];
    }

    // 7. Aperçu image
    if s(post, "/post_hint") == Some("image") {
        if let Some(u) = s(post, "/preview/images/0/source/url") {
            return vec![Media {
                url: unescape(u),
                audio: None,
            }];
        }
    }
    vec![]
}

/// Cherche la piste audio DASH à côté de la vidéo (HEAD, 200 requis).
async fn find_audio(ctx: &SourceCtx, video_url: &str) -> Option<String> {
    let base = video_url.rsplit_once('/')?.0;
    for name in ["DASH_AUDIO_128.mp4", "DASH_audio.mp4"] {
        let cand = format!("{base}/{name}");
        let ok = ctx
            .http
            .head(&cand)
            .header("user-agent", UA)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map(|r| r.status() == StatusCode::OK)
            .unwrap_or(false);
        if ok {
            return Some(cand);
        }
    }
    None
}

// ── Redgifs ─────────────────────────────────────────────────────────────────

async fn redgifs_token_fetch(ctx: &SourceCtx) -> Result<String> {
    let resp = ctx
        .http
        .get("https://api.redgifs.com/v2/auth/temporary")
        .header("user-agent", UA)
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("auth redgifs {}", resp.status());
    }
    let v: Value = resp.json().await?;
    match v.get("token").and_then(Value::as_str) {
        Some(t) if !t.is_empty() => Ok(t.to_string()),
        _ => bail!("token redgifs absent"),
    }
}

async fn redgifs_url(ctx: &SourceCtx, cache: &mut Option<String>, id: &str) -> Result<String> {
    for attempt in 0..2 {
        if cache.is_none() {
            *cache = Some(redgifs_token_fetch(ctx).await?);
        }
        let tok = cache.clone().unwrap_or_default();
        let resp = ctx
            .http
            .get(format!("https://api.redgifs.com/v2/gifs/{id}"))
            .header("user-agent", UA)
            .header("authorization", format!("Bearer {tok}"))
            .send()
            .await?;
        let st = resp.status();
        if (st == StatusCode::UNAUTHORIZED || st == StatusCode::FORBIDDEN) && attempt == 0 {
            *cache = None;
            continue;
        }
        if !st.is_success() {
            bail!("API redgifs {st}");
        }
        let v: Value = resp.json().await?;
        return match s(&v, "/gif/urls/hd").or_else(|| s(&v, "/gif/urls/sd")) {
            Some(u) => Ok(u.to_string()),
            None => bail!("aucune URL vidéo"),
        };
    }
    bail!("authentification redgifs refusée")
}
