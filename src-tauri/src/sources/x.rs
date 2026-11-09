//! Likes X du compte connecté — API GraphQL web de x.com, en Rust pur (reqwest).
//!
//! Auth : cookies de la session web (`auth_token`, `ct0`, `twid`…) + bearer public du
//! client web. Le queryId de l'opération `Likes` est découvert dans le bundle JS
//! `main.<hash>.js` ; à défaut on utilise une constante codée en dur (peut périmer).
//!
//! Limite connue : x.com peut exiger l'en-tête `x-client-transaction-id` (jeton signé
//! dérivé de la page d'accueil et d'animations SVG). Il n'est PAS implémenté ici : si
//! l'API répond 404/403 de façon systématique, c'est la cause probable.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use rand::Rng;
use regex::Regex;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, CONTENT_TYPE,
    COOKIE, ORIGIN, REFERER, SET_COOKIE, USER_AGENT,
};
use reqwest::StatusCode;
use serde_json::{json, Map, Value};
use tokio::sync::mpsc::Sender;

use super::{MediaCandidate, SourceCtx};

/// Bearer public du client web x.com (identique pour tous les utilisateurs).
const BEARER: &str = "Bearer AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";
const HOME_URL: &str = "https://x.com/";
const API_ROOT: &str = "https://x.com/i/api/graphql";
/// queryId de repli de l'opération `Likes` (relevé dans gallery-dl 1.32.6).
/// X le change régulièrement : il PEUT ÊTRE PÉRIMÉ, d'où la découverte dynamique.
const FALLBACK_LIKES_QUERY_ID: &str = "TGEKkJG_meudeaFcqaxM-Q";
const PAGE_SIZE: u32 = 100;
/// Nombre max de bundles JS secondaires inspectés si `main.*.js` ne contient pas `Likes`.
const MAX_EXTRA_BUNDLES: usize = 8;
const MAX_FEATURE_RETRIES: usize = 3;
const MAX_RATE_LIMIT_WAITS: usize = 10;
const MAX_TRANSIENT_RETRIES: usize = 3;

const SESSION_EXPIRED: &str = "Session X expirée : reconnecte ton compte";

/// Jeu de features courant du client web (utilisé si la découverte échoue, et comme
/// source des valeurs par défaut pour les features découvertes).
const DEFAULT_FEATURES: &[(&str, bool)] = &[
    ("rweb_video_screen_enabled", false),
    ("payments_enabled", false),
    ("rweb_xchat_enabled", false),
    ("profile_label_improvements_pcf_label_in_post_enabled", true),
    ("rweb_tipjar_consumption_enabled", true),
    ("verified_phone_label_enabled", false),
    ("creator_subscriptions_tweet_preview_api_enabled", true),
    ("responsive_web_graphql_timeline_navigation_enabled", true),
    ("responsive_web_graphql_skip_user_profile_image_extensions_enabled", false),
    ("premium_content_api_read_enabled", false),
    ("communities_web_enable_tweet_community_results_fetch", true),
    ("c9s_tweet_anatomy_moderator_badge_enabled", true),
    ("responsive_web_grok_analyze_button_fetch_trends_enabled", false),
    ("responsive_web_grok_analyze_post_followups_enabled", true),
    ("responsive_web_jetfuel_frame", true),
    ("responsive_web_grok_share_attachment_enabled", true),
    ("articles_preview_enabled", true),
    ("responsive_web_edit_tweet_api_enabled", true),
    ("graphql_is_translatable_rweb_tweet_is_translatable_enabled", true),
    ("view_counts_everywhere_api_enabled", true),
    ("longform_notetweets_consumption_enabled", true),
    ("responsive_web_twitter_article_tweet_consumption_enabled", true),
    ("tweet_awards_web_tipping_enabled", false),
    ("responsive_web_grok_show_grok_translated_post", false),
    ("responsive_web_grok_analysis_button_from_backend", true),
    ("creator_subscriptions_quote_tweet_preview_enabled", false),
    ("freedom_of_speech_not_reach_fetch_enabled", true),
    ("standardized_nudges_misinfo", true),
    ("tweet_with_visibility_results_prefer_gql_limited_actions_policy_enabled", true),
    ("longform_notetweets_rich_text_read_enabled", true),
    ("longform_notetweets_inline_media_enabled", true),
    ("responsive_web_grok_image_annotation_enabled", true),
    ("responsive_web_grok_imagine_annotation_enabled", true),
    ("responsive_web_grok_community_note_auto_translation_is_enabled", false),
    ("responsive_web_enhance_cards_enabled", false),
];

static RE_SCRIPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"https://abs\.twimg\.com/responsive-web/client-web(?:-legacy)?/[A-Za-z0-9_.~-]+\.js"#)
        .expect("regex script")
});
static RE_LIKES_QID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"queryId:\s*"([^"]+)"\s*,\s*operationName:\s*"Likes""#).expect("regex queryId")
});
static RE_FEATURE_SWITCHES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"featureSwitches:\s*\[([^\]]*)\]"#).expect("regex features"));
static RE_FIELD_TOGGLES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"fieldToggles:\s*\[([^\]]*)\]"#).expect("regex toggles"));
static RE_QUOTED_IDENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""([A-Za-z0-9_]+)""#).expect("regex ident"));
static RE_MISSING_FEATURES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)cannot be null:\s*([A-Za-z0-9_,\s]+)"#).expect("regex missing features")
});

// ───────────────────────────── Point d'entrée ─────────────────────────────

pub async fn collect_likes(ctx: &SourceCtx, tx: Sender<MediaCandidate>) -> Result<()> {
    let Some(session) = ctx.session.as_ref() else {
        bail!("Compte X non connecté");
    };

    let user_id = session
        .user_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| session.get("twid").and_then(user_id_from_twid))
        .ok_or_else(|| anyhow!("Identifiant du compte X introuvable : reconnecte ton compte"))?;

    let mut api = XApi {
        ctx,
        cookie_header: session.cookie_header(),
        ct0: session.get("ct0").filter(|v| !v.is_empty()).map(str::to_string),
    };

    match session.username.as_deref().filter(|u| !u.is_empty()) {
        Some(u) => ctx.log.info(format!("Récupération des likes de @{u}")),
        None => ctx.log.info("Récupération des likes du compte X connecté"),
    }

    let op = match discover_operation(&mut api).await {
        Ok(Some(op)) => {
            ctx.log.info(format!(
                "Opération Likes découverte ({} features)",
                op.features.len()
            ));
            op
        }
        Ok(None) => {
            ctx.log.warn("queryId Likes introuvable dans les scripts x.com : repli sur la valeur codée en dur");
            LikesOperation::fallback()
        }
        Err(e) => {
            if ctx.cancel.is_cancelled() {
                return Ok(());
            }
            ctx.log.warn(format!(
                "Découverte du queryId Likes impossible ({e}) : repli sur la valeur codée en dur"
            ));
            LikesOperation::fallback()
        }
    };
    if ctx.cancel.is_cancelled() {
        return Ok(());
    }
    if api.ct0.is_none() {
        bail!("Session X incomplète (jeton ct0 absent) : reconnecte ton compte");
    }

    let mut extra_features: HashMap<String, bool> = HashMap::new();
    let mut seen_entries: HashSet<String> = HashSet::new();
    let mut cursor: Option<String> = None;
    let mut page_no: u32 = 0;
    let mut total_tweets: usize = 0;
    let mut total_media: usize = 0;

    loop {
        if ctx.cancel.is_cancelled() {
            return Ok(());
        }
        page_no += 1;

        let Some(body) = api
            .fetch_likes(&op, &user_id, cursor.as_deref(), &mut extra_features)
            .await?
        else {
            return Ok(()); // annulé
        };

        let page = parse_page(&body);
        let mut new_tweets = 0usize;
        let mut batch: Vec<MediaCandidate> = Vec::new();
        for tweet in page.tweets {
            if seen_entries.insert(tweet.entry_id) {
                new_tweets += 1;
                batch.extend(tweet.media);
            }
        }
        total_tweets += new_tweets;
        total_media += batch.len();
        ctx.log.info(format!(
            "Page {page_no} : {new_tweets} tweets, {} médias",
            batch.len()
        ));

        for candidate in batch {
            tokio::select! {
                _ = ctx.cancel.cancelled() => return Ok(()),
                sent = tx.send(candidate) => {
                    if sent.is_err() {
                        return Ok(()); // récepteur fermé (limite atteinte / job annulé)
                    }
                }
            }
        }

        match page.cursor {
            Some(next) if new_tweets > 0 && cursor.as_deref() != Some(next.as_str()) => {
                cursor = Some(next);
            }
            _ => break,
        }

        if !sleep_or_cancel(ctx, Duration::from_millis(page_pause_ms())).await {
            return Ok(());
        }
    }

    ctx.log.info(format!(
        "Fin des likes X : {total_tweets} tweets parcourus, {total_media} médias trouvés"
    ));
    Ok(())
}

// ───────────────────────────── Client HTTP ─────────────────────────────

struct XApi<'a> {
    ctx: &'a SourceCtx,
    /// En-tête Cookie complet (jamais journalisé).
    cookie_header: String,
    /// Jeton CSRF = cookie `ct0` (jamais journalisé).
    ct0: Option<String>,
}

impl XApi<'_> {
    fn cookie_value(&self) -> Result<HeaderValue> {
        let mut v = HeaderValue::from_str(&self.cookie_header)
            .map_err(|_| anyhow!("Cookies de session X invalides : reconnecte ton compte"))?;
        v.set_sensitive(true);
        Ok(v)
    }

    fn page_headers(&self) -> Result<HeaderMap> {
        let mut h = HeaderMap::new();
        h.insert(USER_AGENT, HeaderValue::from_static(UA));
        h.insert(
            ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
        );
        h.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("fr-FR,fr;q=0.9,en;q=0.8"));
        h.insert(COOKIE, self.cookie_value()?);
        Ok(h)
    }

    fn api_headers(&self) -> Result<HeaderMap> {
        let ct0 = self
            .ct0
            .as_deref()
            .ok_or_else(|| anyhow!("Session X incomplète (jeton ct0 absent) : reconnecte ton compte"))?;
        let mut csrf = HeaderValue::from_str(ct0)
            .map_err(|_| anyhow!("Jeton ct0 invalide : reconnecte ton compte"))?;
        csrf.set_sensitive(true);
        let mut auth = HeaderValue::from_static(BEARER);
        auth.set_sensitive(true);

        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, auth);
        h.insert(HeaderName::from_static("x-csrf-token"), csrf);
        h.insert(
            HeaderName::from_static("x-twitter-auth-type"),
            HeaderValue::from_static("OAuth2Session"),
        );
        h.insert(HeaderName::from_static("x-twitter-active-user"), HeaderValue::from_static("yes"));
        h.insert(
            HeaderName::from_static("x-twitter-client-language"),
            HeaderValue::from_static("fr"),
        );
        h.insert(COOKIE, self.cookie_value()?);
        h.insert(REFERER, HeaderValue::from_static(HOME_URL));
        h.insert(ORIGIN, HeaderValue::from_static("https://x.com"));
        h.insert(USER_AGENT, HeaderValue::from_static(UA));
        h.insert(ACCEPT, HeaderValue::from_static("*/*"));
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        h.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("fr-FR,fr;q=0.9,en;q=0.8"));
        Ok(h)
    }

    /// Met à jour `ct0` (et l'en-tête Cookie) si la réponse en pose un nouveau.
    /// Renvoie `true` si le jeton a changé.
    fn absorb_set_cookie(&mut self, headers: &HeaderMap) -> bool {
        let Some(new_ct0) = set_cookie_value(headers, "ct0") else {
            return false;
        };
        if self.ct0.as_deref() == Some(new_ct0.as_str()) {
            return false;
        }
        self.cookie_header = replace_cookie(&self.cookie_header, "ct0", &new_ct0);
        self.ct0 = Some(new_ct0);
        true
    }

    /// Envoie une requête en respectant l'annulation. `Ok(None)` = annulé.
    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Option<reqwest::Response>, reqwest::Error> {
        tokio::select! {
            _ = self.ctx.cancel.cancelled() => Ok(None),
            r = req.send() => r.map(Some),
        }
    }

    /// Une page de la timeline Likes, avec gestion des erreurs/rejeux. `Ok(None)` = annulé.
    async fn fetch_likes(
        &mut self,
        op: &LikesOperation,
        user_id: &str,
        cursor: Option<&str>,
        extra_features: &mut HashMap<String, bool>,
    ) -> Result<Option<Value>> {
        let mut feature_retries = 0usize;
        let mut rate_limit_waits = 0usize;
        let mut transient_retries = 0usize;
        let mut csrf_retried = false;
        let mut not_found_retried = false;

        loop {
            if self.ctx.cancel.is_cancelled() {
                return Ok(None);
            }
            let url = likes_url(op, user_id, cursor, extra_features)?;
            let req = self
                .ctx
                .http
                .get(url)
                .headers(self.api_headers()?)
                .timeout(Duration::from_secs(30));

            let resp = match self.send(req).await {
                Ok(Some(r)) => r,
                Ok(None) => return Ok(None),
                Err(e) => {
                    transient_retries += 1;
                    if transient_retries > MAX_TRANSIENT_RETRIES {
                        bail!("Erreur réseau vers x.com : {e}");
                    }
                    self.ctx.log.warn(format!("Erreur réseau vers x.com, nouvel essai ({transient_retries}/{MAX_TRANSIENT_RETRIES})"));
                    if !sleep_or_cancel(self.ctx, Duration::from_secs(2 * transient_retries as u64)).await {
                        return Ok(None);
                    }
                    continue;
                }
            };

            let ct0_changed = self.absorb_set_cookie(resp.headers());
            let status = resp.status();
            let headers = resp.headers().clone();
            let body = tokio::select! {
                _ = self.ctx.cancel.cancelled() => return Ok(None),
                b = resp.text() => b.unwrap_or_default(),
            };

            match status {
                s if s.is_success() => {
                    let v: Value = serde_json::from_str(&body)
                        .map_err(|_| anyhow!("Réponse Likes illisible (JSON invalide) : {}", truncate(&body, 200)))?;
                    if v.pointer("/data/user/result").is_none() {
                        if v.get("errors").is_some() {
                            bail!("X a renvoyé une erreur : {}", api_error_message(&body));
                        }
                        self.ctx.log.warn("Réponse Likes inattendue (pas de timeline utilisateur)");
                    }
                    return Ok(Some(v));
                }
                StatusCode::BAD_REQUEST => {
                    let missing = parse_missing_features(&body);
                    let fresh: Vec<String> = missing
                        .into_iter()
                        .filter(|f| !extra_features.contains_key(f))
                        .collect();
                    if !fresh.is_empty() && feature_retries < MAX_FEATURE_RETRIES {
                        feature_retries += 1;
                        self.ctx.log.warn(format!(
                            "X exige de nouvelles features ({}) : nouvel essai",
                            fresh.join(", ")
                        ));
                        for f in fresh {
                            extra_features.insert(f, true);
                        }
                        continue;
                    }
                    bail!("Requête Likes refusée par X (400) : {}", api_error_message(&body));
                }
                StatusCode::TOO_MANY_REQUESTS => {
                    rate_limit_waits += 1;
                    if rate_limit_waits > MAX_RATE_LIMIT_WAITS {
                        bail!("Limite de requêtes X atteinte trop de fois d'affilée, réessaie plus tard");
                    }
                    let wait = rate_limit_wait_secs(&headers, now_secs());
                    self.ctx.log.info(format!("Limite X atteinte, reprise dans {wait} s"));
                    if !sleep_or_cancel(self.ctx, Duration::from_secs(wait)).await {
                        return Ok(None);
                    }
                    continue;
                }
                StatusCode::UNAUTHORIZED => bail!(SESSION_EXPIRED),
                StatusCode::FORBIDDEN => {
                    // 353 = jeton CSRF (ct0) périmé : X en renvoie souvent un nouveau.
                    if ct0_changed && !csrf_retried && has_error_code(&body, 353) {
                        csrf_retried = true;
                        continue;
                    }
                    if has_error_code(&body, 32) || has_error_code(&body, 89) || has_error_code(&body, 353) {
                        bail!(SESSION_EXPIRED);
                    }
                    bail!(
                        "{SESSION_EXPIRED} (403 : {}). Si l'erreur persiste après reconnexion, X exige \
                         probablement l'en-tête x-client-transaction-id, non pris en charge.",
                        api_error_message(&body)
                    );
                }
                StatusCode::NOT_FOUND => {
                    if !not_found_retried {
                        not_found_retried = true;
                        if !sleep_or_cancel(self.ctx, Duration::from_secs(2)).await {
                            return Ok(None);
                        }
                        continue;
                    }
                    bail!(
                        "L'API X répond 404 de façon systématique (queryId {} {}). Cause probable : \
                         X exige l'en-tête x-client-transaction-id (non pris en charge) ou le queryId est périmé.",
                        op.query_id,
                        if op.discovered { "découvert" } else { "de repli" }
                    );
                }
                s if s.is_server_error() => {
                    transient_retries += 1;
                    if transient_retries > MAX_TRANSIENT_RETRIES {
                        bail!("x.com indisponible ({s})");
                    }
                    self.ctx.log.warn(format!("x.com renvoie {s}, nouvel essai ({transient_retries}/{MAX_TRANSIENT_RETRIES})"));
                    if !sleep_or_cancel(self.ctx, Duration::from_secs(3 * transient_retries as u64)).await {
                        return Ok(None);
                    }
                    continue;
                }
                s => bail!("Réponse inattendue de x.com ({s}) : {}", api_error_message(&body)),
            }
        }
    }
}

// ───────────────────────────── Découverte du queryId ─────────────────────────────

#[derive(Debug, Clone)]
struct LikesOperation {
    query_id: String,
    features: Vec<String>,
    field_toggles: Vec<String>,
    discovered: bool,
}

impl LikesOperation {
    fn fallback() -> Self {
        Self {
            query_id: FALLBACK_LIKES_QUERY_ID.to_string(),
            features: Vec::new(),
            field_toggles: Vec::new(),
            discovered: false,
        }
    }
}

/// GET x.com → bundles JS → queryId/features de `Likes`. `Ok(None)` = introuvable.
async fn discover_operation(api: &mut XApi<'_>) -> Result<Option<LikesOperation>> {
    let req = api
        .ctx
        .http
        .get(HOME_URL)
        .headers(api.page_headers()?)
        .timeout(Duration::from_secs(30));
    let Some(resp) = api.send(req).await? else {
        bail!("annulé");
    };
    api.absorb_set_cookie(resp.headers());
    let status = resp.status();
    if !status.is_success() {
        bail!("page d'accueil x.com : {status}");
    }
    let html = resp.text().await?;
    let scripts = script_urls(&html);
    if scripts.is_empty() {
        return Ok(None);
    }

    for (i, url) in scripts.iter().enumerate() {
        if i > MAX_EXTRA_BUNDLES {
            break;
        }
        if api.ctx.cancel.is_cancelled() {
            bail!("annulé");
        }
        let req = api
            .ctx
            .http
            .get(url.as_str())
            .header(USER_AGENT, UA)
            .header(REFERER, HOME_URL)
            .timeout(Duration::from_secs(60));
        let Some(resp) = api.send(req).await? else {
            bail!("annulé");
        };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(js) = resp.text().await else { continue };
        if let Some(op) = find_likes_operation(&js) {
            return Ok(Some(op));
        }
    }
    Ok(None)
}

/// URLs des bundles JS du client web, `main.*.js` en premier, sans doublon.
fn script_urls(html: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut urls: Vec<String> = RE_SCRIPT
        .find_iter(html)
        .map(|m| m.as_str().to_string())
        .filter(|u| seen.insert(u.clone()))
        .collect();
    urls.sort_by_key(|u| {
        let file = u.rsplit('/').next().unwrap_or("");
        if file.starts_with("main.") {
            0
        } else if file.contains("main") {
            1
        } else {
            2
        }
    });
    urls
}

/// Cherche `queryId:"…",operationName:"Likes"` et les listes featureSwitches/fieldToggles
/// associées (dans le bloc de l'opération, avant la prochaine opération).
fn find_likes_operation(js: &str) -> Option<LikesOperation> {
    let caps = RE_LIKES_QID.captures(js)?;
    let query_id = caps.get(1)?.as_str().to_string();
    let end = caps.get(0)?.end();
    let tail = js.get(end..)?;
    let mut window_end = tail.find("queryId:").unwrap_or(tail.len()).min(6000);
    while window_end > 0 && !tail.is_char_boundary(window_end) {
        window_end -= 1;
    }
    let window = &tail[..window_end];

    let idents = |re: &Regex| -> Vec<String> {
        re.captures(window)
            .and_then(|c| c.get(1))
            .map(|m| {
                RE_QUOTED_IDENT
                    .captures_iter(m.as_str())
                    .filter_map(|c| c.get(1).map(|x| x.as_str().to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };

    Some(LikesOperation {
        query_id,
        features: idents(&RE_FEATURE_SWITCHES),
        field_toggles: idents(&RE_FIELD_TOGGLES),
        discovered: true,
    })
}

// ───────────────────────────── Construction de la requête ─────────────────────────────

fn build_features(discovered: &[String], extra: &HashMap<String, bool>) -> Map<String, Value> {
    let defaults: HashMap<&str, bool> = DEFAULT_FEATURES.iter().copied().collect();
    let mut map = Map::new();
    if discovered.is_empty() {
        for (k, v) in DEFAULT_FEATURES {
            map.insert((*k).to_string(), Value::Bool(*v));
        }
    } else {
        for name in discovered {
            let v = defaults.get(name.as_str()).copied().unwrap_or(true);
            map.insert(name.clone(), Value::Bool(v));
        }
    }
    for (k, v) in extra {
        map.insert(k.clone(), Value::Bool(*v));
    }
    map
}

fn build_field_toggles(discovered: &[String]) -> Map<String, Value> {
    let mut map = Map::new();
    if discovered.is_empty() {
        map.insert("withArticlePlainText".into(), Value::Bool(false));
    } else {
        for name in discovered {
            map.insert(name.clone(), Value::Bool(false));
        }
    }
    map
}

fn likes_url(
    op: &LikesOperation,
    user_id: &str,
    cursor: Option<&str>,
    extra: &HashMap<String, bool>,
) -> Result<url::Url> {
    let mut variables = json!({
        "userId": user_id,
        "count": PAGE_SIZE,
        "includePromotedContent": false,
        "withClientEventToken": false,
        "withBirdwatchNotes": false,
        "withVoice": true,
    });
    if let (Some(c), Some(obj)) = (cursor, variables.as_object_mut()) {
        obj.insert("cursor".into(), Value::String(c.to_string()));
    }
    let features = Value::Object(build_features(&op.features, extra));
    let toggles = Value::Object(build_field_toggles(&op.field_toggles));

    let mut url = url::Url::parse(&format!("{API_ROOT}/{}/Likes", op.query_id))?;
    url.query_pairs_mut()
        .append_pair("variables", &variables.to_string())
        .append_pair("features", &features.to_string())
        .append_pair("fieldToggles", &toggles.to_string());
    Ok(url)
}

// ───────────────────────────── Parsing (fonctions pures) ─────────────────────────────

#[derive(Debug, Default)]
struct ParsedTweet {
    /// `entryId` de la timeline (dédoublonnage entre pages).
    entry_id: String,
    media: Vec<MediaCandidate>,
}

#[derive(Debug, Default)]
struct LikesPage {
    tweets: Vec<ParsedTweet>,
    cursor: Option<String>,
}

/// Médias d'une page de Likes + curseur suivant (`cursor-bottom-*`).
#[allow(dead_code)] // façade pure de `parse_page`, utilisée par les tests
fn parse_likes_page(v: &Value) -> (Vec<MediaCandidate>, Option<String>) {
    let page = parse_page(v);
    let media = page.tweets.into_iter().flat_map(|t| t.media).collect();
    (media, page.cursor)
}

fn parse_page(v: &Value) -> LikesPage {
    let mut page = LikesPage::default();
    let Some(user) = v.pointer("/data/user/result") else {
        return page;
    };
    let Some(instructions) = user
        .get("timeline_v2")
        .or_else(|| user.get("timeline"))
        .and_then(|t| t.pointer("/timeline/instructions"))
        .and_then(Value::as_array)
    else {
        return page;
    };

    for instr in instructions {
        let kind = instr.get("type").and_then(Value::as_str).unwrap_or("");
        let entries: Vec<&Value> = match kind {
            "TimelineAddEntries" => instr
                .get("entries")
                .and_then(Value::as_array)
                .map(|a| a.iter().collect())
                .unwrap_or_default(),
            "TimelineReplaceEntry" => instr.get("entry").into_iter().collect(),
            _ => continue,
        };
        for entry in entries {
            parse_entry(entry, &mut page);
        }
    }
    page
}

fn parse_entry(entry: &Value, page: &mut LikesPage) {
    let entry_id = entry.get("entryId").and_then(Value::as_str).unwrap_or("");
    let Some(content) = entry.get("content") else {
        return;
    };

    let cursor_type = content
        .get("cursorType")
        .and_then(Value::as_str)
        .unwrap_or("");
    if entry_id.starts_with("cursor-bottom") || cursor_type == "Bottom" {
        if let Some(val) = content.get("value").and_then(Value::as_str) {
            if !val.is_empty() {
                page.cursor = Some(val.to_string());
            }
        }
        return;
    }

    if entry_id.starts_with("tweet-") {
        if let Some(result) = content.pointer("/itemContent/tweet_results/result") {
            page.tweets.push(parse_tweet(entry_id, result));
        } else {
            page.tweets.push(ParsedTweet { entry_id: entry_id.to_string(), media: Vec::new() });
        }
        return;
    }

    // Modules (rares dans Likes) : content.items[].item.itemContent.tweet_results.result
    if let Some(items) = content.get("items").and_then(Value::as_array) {
        for item in items {
            let item_id = item.get("entryId").and_then(Value::as_str).unwrap_or(entry_id);
            if let Some(result) = item.pointer("/item/itemContent/tweet_results/result") {
                page.tweets.push(parse_tweet(item_id, result));
            }
        }
    }
}

/// Déballe `TweetWithVisibilityResults` ; ignore tombstones/indisponibles.
fn unwrap_tweet(result: &Value) -> Option<&Value> {
    match result.get("__typename").and_then(Value::as_str) {
        Some("Tweet") => Some(result),
        Some("TweetWithVisibilityResults") => result.get("tweet"),
        Some(_) => None,
        None => result
            .get("tweet")
            .or_else(|| result.get("legacy").map(|_| result)),
    }
}

fn parse_tweet(entry_id: &str, result: &Value) -> ParsedTweet {
    let mut parsed = ParsedTweet { entry_id: entry_id.to_string(), media: Vec::new() };
    let Some(tweet) = unwrap_tweet(result) else {
        return parsed;
    };
    let Some(legacy) = tweet.get("legacy") else {
        return parsed;
    };

    let tweet_id = tweet
        .get("rest_id")
        .and_then(Value::as_str)
        .or_else(|| legacy.get("id_str").and_then(Value::as_str))
        .or_else(|| entry_id.strip_prefix("tweet-"))
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()));
    let Some(tweet_id) = tweet_id else {
        return parsed;
    };

    let user = tweet.pointer("/core/user_results/result");
    let author = user
        .and_then(|u| u.pointer("/core/screen_name"))
        .or_else(|| user.and_then(|u| u.pointer("/legacy/screen_name")))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("x");

    let media = legacy
        .pointer("/extended_entities/media")
        .or_else(|| legacy.pointer("/entities/media"))
        .and_then(Value::as_array);
    let Some(media) = media else {
        return parsed;
    };

    for (i, m) in media.iter().enumerate() {
        let index = i + 1;
        let kind = m.get("type").and_then(Value::as_str).unwrap_or("");
        let url = match kind {
            "photo" => m
                .get("media_url_https")
                .or_else(|| m.get("media_url"))
                .and_then(Value::as_str)
                .and_then(photo_orig_url),
            "video" | "animated_gif" => best_mp4_variant(m),
            _ => None,
        };
        let Some(url) = url else { continue };
        parsed.media.push(MediaCandidate {
            key: format!("x:{tweet_id}:{index}"),
            url,
            audio_url: None,
            name: format!("{author}_{tweet_id}_{index:02}"),
            referer: Some(HOME_URL.to_string()),
        });
    }
    parsed
}

/// `https://pbs.twimg.com/media/ABC.jpg` → `https://pbs.twimg.com/media/ABC?format=jpg&name=orig`.
fn photo_orig_url(u: &str) -> Option<String> {
    let (base, query) = match u.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (u, None),
    };
    let base = base.split('#').next().unwrap_or(base);
    let (dir, file) = base.rsplit_once('/')?;
    if file.is_empty() || !dir.starts_with("http") {
        return None;
    }
    let (stem, ext) = match file.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s, e.to_ascii_lowercase()),
        _ => {
            let fmt = query
                .into_iter()
                .flat_map(|q| q.split('&'))
                .find_map(|kv| kv.strip_prefix("format="))
                .filter(|f| !f.is_empty())
                .unwrap_or("jpg");
            (file, fmt.to_ascii_lowercase())
        }
    };
    Some(format!("{dir}/{stem}?format={ext}&name=orig"))
}

/// Variante `video/mp4` au plus grand bitrate.
fn best_mp4_variant(m: &Value) -> Option<String> {
    m.pointer("/video_info/variants")
        .and_then(Value::as_array)?
        .iter()
        .filter(|v| v.get("content_type").and_then(Value::as_str) == Some("video/mp4"))
        .filter_map(|v| {
            let url = v.get("url").and_then(Value::as_str)?;
            let bitrate = v.get("bitrate").and_then(Value::as_u64).unwrap_or(0);
            Some((bitrate, url))
        })
        .max_by_key(|(b, _)| *b)
        .map(|(_, u)| u.to_string())
}

/// Cookie `twid` (`u%3D123` ou `"u=123"`) → `123`.
fn user_id_from_twid(raw: &str) -> Option<String> {
    let decoded = urlencoding::decode(raw.trim().trim_matches('"')).ok()?;
    let id = decoded.trim().trim_matches('"').strip_prefix("u=")?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit())).then(|| id.to_string())
}

/// Noms extraits de « The following features cannot be null: a, b, c ».
fn parse_missing_features(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for caps in RE_MISSING_FEATURES.captures_iter(body) {
        let Some(list) = caps.get(1) else { continue };
        for name in list.as_str().split(',') {
            let name = name.trim();
            if !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !out.iter().any(|n| n == name)
            {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// Messages `errors[].message` de l'API, sinon début du corps.
fn api_error_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        if let Some(errors) = v.get("errors").and_then(Value::as_array) {
            let msgs: Vec<&str> = errors
                .iter()
                .filter_map(|e| e.get("message").and_then(Value::as_str))
                .collect();
            if !msgs.is_empty() {
                return truncate(&msgs.join(" ; "), 300);
            }
        }
    }
    let t = body.trim();
    if t.is_empty() {
        "réponse vide".into()
    } else {
        truncate(t, 200)
    }
}

fn has_error_code(body: &str, code: u64) -> bool {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("errors").and_then(Value::as_array).cloned())
        .map(|errs| errs.iter().any(|e| e.get("code").and_then(Value::as_u64) == Some(code)))
        .unwrap_or(false)
}

/// Valeur d'un cookie posé par `Set-Cookie` (ignore les suppressions à valeur vide).
fn set_cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .filter_map(|s| s.split(';').next())
        .filter_map(|kv| kv.split_once('='))
        .filter(|(n, v)| n.trim() == name && !v.trim().is_empty())
        .map(|(_, v)| v.trim().to_string())
        .next_back()
}

/// Remplace (ou ajoute) `name=value` dans un en-tête Cookie `a=1; b=2`.
fn replace_cookie(header: &str, name: &str, value: &str) -> String {
    let mut parts: Vec<String> = header
        .split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .filter(|p| p.split_once('=').map(|(n, _)| n.trim()) != Some(name))
        .map(str::to_string)
        .collect();
    parts.push(format!("{name}={value}"));
    parts.join("; ")
}

/// Attente avant reprise après un 429 (`x-rate-limit-reset` = epoch en secondes).
fn rate_limit_wait_secs(headers: &HeaderMap, now: u64) -> u64 {
    headers
        .get("x-rate-limit-reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|reset| reset.saturating_sub(now) + 2)
        .unwrap_or(60)
        .clamp(5, 900)
}

fn truncate(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn page_pause_ms() -> u64 {
    rand::thread_rng().gen_range(500..=900)
}

/// Attend `dur` ; `false` si le job a été annulé entre-temps.
async fn sleep_or_cancel(ctx: &SourceCtx, dur: Duration) -> bool {
    tokio::select! {
        _ = ctx.cancel.cancelled() => false,
        _ = tokio::time::sleep(dur) => true,
    }
}

// ───────────────────────────── Tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_page() -> Value {
        serde_json::from_str(r#"{
          "data": { "user": { "result": {
            "__typename": "User",
            "timeline": { "timeline": { "instructions": [
              { "type": "TimelineClearCache" },
              { "type": "TimelineAddEntries", "entries": [
                { "entryId": "tweet-1850000000000000001", "sortIndex": "1",
                  "content": { "entryType": "TimelineTimelineItem", "__typename": "TimelineTimelineItem",
                    "itemContent": { "itemType": "TimelineTweet", "__typename": "TimelineTweet",
                      "tweet_results": { "result": {
                        "__typename": "Tweet",
                        "rest_id": "1850000000000000001",
                        "core": { "user_results": { "result": {
                          "__typename": "User", "rest_id": "11",
                          "core": { "screen_name": "alice_art", "name": "Alice" },
                          "legacy": {}
                        } } },
                        "legacy": {
                          "id_str": "1850000000000000001",
                          "full_text": "deux photos https://t.co/x",
                          "entities": { "media": [ { "type": "photo",
                            "media_url_https": "https://pbs.twimg.com/media/GaAAAAAXcAA1.jpg" } ] },
                          "extended_entities": { "media": [
                            { "type": "photo", "id_str": "1",
                              "media_url_https": "https://pbs.twimg.com/media/GaAAAAAXcAA1.jpg" },
                            { "type": "photo", "id_str": "2",
                              "media_url_https": "https://pbs.twimg.com/media/GaBBBBBWkAA2.png" }
                          ] }
                        }
                      } }
                    } } },
                { "entryId": "tweet-1850000000000000002", "sortIndex": "2",
                  "content": { "entryType": "TimelineTimelineItem",
                    "itemContent": { "itemType": "TimelineTweet",
                      "tweet_results": { "result": {
                        "__typename": "TweetWithVisibilityResults",
                        "tweet": {
                          "rest_id": "1850000000000000002",
                          "core": { "user_results": { "result": {
                            "__typename": "User",
                            "legacy": { "screen_name": "bob_vids" }
                          } } },
                          "legacy": {
                            "id_str": "1850000000000000002",
                            "extended_entities": { "media": [
                              { "type": "video",
                                "media_url_https": "https://pbs.twimg.com/ext_tw_video_thumb/1/pu/img/t.jpg",
                                "video_info": { "variants": [
                                  { "content_type": "application/x-mpegURL",
                                    "url": "https://video.twimg.com/ext_tw_video/1/pu/pl/a.m3u8" },
                                  { "content_type": "video/mp4", "bitrate": 832000,
                                    "url": "https://video.twimg.com/ext_tw_video/1/pu/vid/640x360/low.mp4" },
                                  { "content_type": "video/mp4", "bitrate": 2176000,
                                    "url": "https://video.twimg.com/ext_tw_video/1/pu/vid/1280x720/high.mp4" }
                                ] } }
                            ] }
                          }
                        },
                        "limitedActionResults": {}
                      } }
                    } } },
                { "entryId": "tweet-1850000000000000003",
                  "content": { "itemContent": { "tweet_results": { "result": {
                    "__typename": "TweetTombstone", "tombstone": {} } } } } },
                { "entryId": "tweet-1850000000000000004",
                  "content": { "itemContent": { "tweet_results": { "result": {
                    "__typename": "Tweet", "rest_id": "1850000000000000004",
                    "core": { "user_results": { "result": { "core": { "screen_name": "gif_lover" } } } },
                    "legacy": { "extended_entities": { "media": [
                      { "type": "animated_gif",
                        "video_info": { "variants": [
                          { "content_type": "video/mp4", "bitrate": 0,
                            "url": "https://video.twimg.com/tweet_video/GaGIF.mp4" } ] } }
                    ] } }
                  } } } } },
                { "entryId": "cursor-top-1850000000000000001",
                  "content": { "entryType": "TimelineTimelineCursor", "value": "TOPCURSOR", "cursorType": "Top" } },
                { "entryId": "cursor-bottom-1850000000000000004",
                  "content": { "entryType": "TimelineTimelineCursor", "value": "HBaAgLydNEXT==", "cursorType": "Bottom" } }
              ] }
            ] } }
          } } }
        }"#)
        .expect("JSON de test valide")
    }

    #[test]
    fn parses_photos_videos_and_cursor() {
        let (media, cursor) = parse_likes_page(&sample_page());
        assert_eq!(cursor.as_deref(), Some("HBaAgLydNEXT=="));
        assert_eq!(media.len(), 4, "{media:#?}");

        assert_eq!(media[0].key, "x:1850000000000000001:1");
        assert_eq!(media[0].name, "alice_art_1850000000000000001_01");
        assert_eq!(media[0].url, "https://pbs.twimg.com/media/GaAAAAAXcAA1?format=jpg&name=orig");
        assert_eq!(media[0].referer.as_deref(), Some("https://x.com/"));
        assert!(media[0].audio_url.is_none());

        assert_eq!(media[1].key, "x:1850000000000000001:2");
        assert_eq!(media[1].name, "alice_art_1850000000000000001_02");
        assert_eq!(media[1].url, "https://pbs.twimg.com/media/GaBBBBBWkAA2?format=png&name=orig");

        assert_eq!(media[2].key, "x:1850000000000000002:1");
        assert_eq!(media[2].name, "bob_vids_1850000000000000002_01");
        assert_eq!(media[2].url, "https://video.twimg.com/ext_tw_video/1/pu/vid/1280x720/high.mp4");

        assert_eq!(media[3].name, "gif_lover_1850000000000000004_01");
        assert_eq!(media[3].url, "https://video.twimg.com/tweet_video/GaGIF.mp4");
    }

    #[test]
    fn counts_tweet_entries_including_tombstones() {
        let page = parse_page(&sample_page());
        assert_eq!(page.tweets.len(), 4);
        assert!(page.tweets[2].media.is_empty());
    }

    #[test]
    fn handles_timeline_v2_and_empty_page() {
        let v = json!({ "data": { "user": { "result": { "timeline_v2": { "timeline": { "instructions": [
            { "type": "TimelineAddEntries", "entries": [
                { "entryId": "cursor-bottom-0", "content": { "value": "SAME", "cursorType": "Bottom" } }
            ] }
        ] } } } } } });
        let page = parse_page(&v);
        assert!(page.tweets.is_empty());
        assert_eq!(page.cursor.as_deref(), Some("SAME"));
        let (media, cursor) = parse_likes_page(&json!({ "data": {} }));
        assert!(media.is_empty() && cursor.is_none());
    }

    #[test]
    fn photo_url_variants() {
        assert_eq!(
            photo_orig_url("https://pbs.twimg.com/media/ABC.JPG").as_deref(),
            Some("https://pbs.twimg.com/media/ABC?format=jpg&name=orig")
        );
        assert_eq!(
            photo_orig_url("https://pbs.twimg.com/media/ABC?format=webp&name=small").as_deref(),
            Some("https://pbs.twimg.com/media/ABC?format=webp&name=orig")
        );
        assert_eq!(photo_orig_url("not a url"), None);
    }

    #[test]
    fn twid_decoding() {
        assert_eq!(user_id_from_twid("u%3D123456").as_deref(), Some("123456"));
        assert_eq!(user_id_from_twid("\"u=42\"").as_deref(), Some("42"));
        assert_eq!(user_id_from_twid("garbage"), None);
    }

    #[test]
    fn missing_features_parsing() {
        let body = r#"{"errors":[{"message":"The following features cannot be null: rweb_new_thing, other_flag_enabled","code":336}]}"#;
        assert_eq!(parse_missing_features(body), vec!["rweb_new_thing", "other_flag_enabled"]);
        assert!(parse_missing_features(r#"{"errors":[{"message":"Bad"}]}"#).is_empty());
    }

    #[test]
    fn finds_likes_operation_in_bundle() {
        let js = r#"e.exports={queryId:"AbC-123_x",operationName:"Bookmarks",operationType:"query",metadata:{featureSwitches:["bm_only"],fieldToggles:[]}}},
            1234:e=>{e.exports={queryId:"LiKeS_qid-9",operationName:"Likes",operationType:"query",metadata:{featureSwitches:["rweb_tipjar_consumption_enabled","brand_new_flag"],fieldToggles:["withArticlePlainText"]}}},
            5678:e=>{e.exports={queryId:"zzz",operationName:"UserTweets",operationType:"query",metadata:{featureSwitches:["other"],fieldToggles:[]}}}"#;
        let op = find_likes_operation(js).expect("opération trouvée");
        assert_eq!(op.query_id, "LiKeS_qid-9");
        assert_eq!(op.features, vec!["rweb_tipjar_consumption_enabled", "brand_new_flag"]);
        assert_eq!(op.field_toggles, vec!["withArticlePlainText"]);

        let feats = build_features(&op.features, &HashMap::from([("late".to_string(), true)]));
        assert_eq!(feats.get("rweb_tipjar_consumption_enabled"), Some(&Value::Bool(true)));
        assert_eq!(feats.get("brand_new_flag"), Some(&Value::Bool(true)));
        assert_eq!(feats.get("late"), Some(&Value::Bool(true)));
        assert!(!feats.contains_key("payments_enabled"));
        assert!(find_likes_operation("nothing here").is_none());
    }

    #[test]
    fn script_urls_main_first() {
        let html = r#"<script src="https://abs.twimg.com/responsive-web/client-web/vendor.aa11.js"></script>
            <script src="https://abs.twimg.com/responsive-web/client-web/main.bb22.js"></script>
            <link href="https://abs.twimg.com/responsive-web/client-web/main.bb22.js">"#;
        let urls = script_urls(html);
        assert_eq!(urls.len(), 2);
        assert!(urls[0].ends_with("/main.bb22.js"));
    }

    #[test]
    fn cookie_helpers() {
        assert_eq!(
            replace_cookie("auth_token=a; ct0=old; twid=u%3D1", "ct0", "new"),
            "auth_token=a; twid=u%3D1; ct0=new"
        );
        let mut h = HeaderMap::new();
        h.append(SET_COOKIE, HeaderValue::from_static("guest_id=v1; Path=/"));
        h.append(SET_COOKIE, HeaderValue::from_static("ct0=fresh; Domain=.x.com; Secure"));
        assert_eq!(set_cookie_value(&h, "ct0").as_deref(), Some("fresh"));
        assert_eq!(set_cookie_value(&h, "auth_token"), None);

        let mut r = HeaderMap::new();
        r.insert("x-rate-limit-reset", HeaderValue::from_static("1000100"));
        assert_eq!(rate_limit_wait_secs(&r, 1_000_000), 102);
        assert_eq!(rate_limit_wait_secs(&HeaderMap::new(), 0), 60);
    }

    #[test]
    fn likes_url_contains_params() {
        let url = likes_url(&LikesOperation::fallback(), "42", Some("CUR"), &HashMap::new())
            .expect("url");
        assert!(url.path().ends_with(&format!("/{FALLBACK_LIKES_QUERY_ID}/Likes")));
        let pairs: HashMap<String, String> = url.query_pairs().into_owned().collect();
        let vars: Value = serde_json::from_str(&pairs["variables"]).expect("variables");
        assert_eq!(vars["userId"], "42");
        assert_eq!(vars["cursor"], "CUR");
        assert_eq!(vars["count"], 100);
        let feats: Value = serde_json::from_str(&pairs["features"]).expect("features");
        assert_eq!(feats["payments_enabled"], false);
        let toggles: Value = serde_json::from_str(&pairs["fieldToggles"]).expect("toggles");
        assert_eq!(toggles["withArticlePlainText"], false);
    }
}
