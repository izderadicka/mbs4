//! Server side for browser based [Piper](https://github.com/rhasspy/piper) TTS.
//!
//! Piper voices follow the rhasspy naming scheme `{lang}_{REGION}-{name}-{quality}`
//! (e.g. `en_US-amy-medium`) and come as a pair of files: the ONNX model
//! `{voiceId}.onnx` and its JSON config `{voiceId}.onnx.json`. Both live in the
//! configured voices directory (`AppConfig::voices_dir`).
//!
//! Endpoints (mounted under `/tts`):
//! - `GET /tts/piper/voices?language={bcp47}` – JSON listing consumed by the client
//! - `GET /tts/piper/voices/{id}.onnx` – the model file
//! - `GET /tts/piper/voices/{id}.onnx.json` – the config file
//!
//! The model/config routes support HTTP range requests, `ETag`/conditional
//! requests and long lived caching (models are 10–60 MB). When cross origin
//! isolation is enabled they also carry `Cross-Origin-Resource-Policy` (and the
//! matching COOP/COEP headers) so the cross-origin-isolated SPA can embed them.

use std::collections::BTreeMap;

use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::Response,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tower::ServiceExt as _;
use tower_http::services::ServeFile;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

const MIME_ONNX: &str = "application/octet-stream";
const MIME_JSON: &str = "application/json";
/// Models are versioned by their (immutable) voice id, so they can be cached
/// forever.
const CACHE_CONTROL_IMMUTABLE: &str = "public, max-age=31536000, immutable";

const NOSNIFF: HeaderName = HeaderName::from_static("x-content-type-options");
const COOP: HeaderName = HeaderName::from_static("cross-origin-opener-policy");
const COEP: HeaderName = HeaderName::from_static("cross-origin-embedder-policy");
const CORP: HeaderName = HeaderName::from_static("cross-origin-resource-policy");

/// Voice metadata returned by the listing endpoint. This is the typed shape the
/// client's `listVoices` consumes.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct VoiceInfo {
    /// Voice id, i.e. the file name without the `.onnx`/`.onnx.json` suffix,
    /// e.g. `en_US-amy-medium`.
    pub id: String,
    /// Human friendly voice name (the `{name}` part of the id, or the config's
    /// `dataset`).
    pub name: String,
    /// BCP47 language tag, e.g. `en-US`.
    pub lang: String,
    /// Voice quality, i.e. the `{quality}` part of the id (e.g. `medium`).
    pub quality: String,
    /// Number of speakers in the model (1 for single speaker voices).
    pub num_speakers: u32,
    /// Speaker names for multi speaker models, ordered by speaker id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speakers: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
pub struct VoicesQuery {
    /// BCP47 language tag. Filters voices by their primary language subtag
    /// (e.g. `en` or `en-US` both match `en_US-*` voices). Omit for all voices.
    pub language: Option<String>,
}

/// Subset of the Piper `.onnx.json` config we care about.
#[derive(Debug, Deserialize)]
struct PiperConfig {
    #[serde(default)]
    num_speakers: Option<u32>,
    #[serde(default)]
    dataset: Option<String>,
    #[serde(default)]
    language: Option<PiperLanguage>,
    #[serde(default)]
    speaker_id_map: Option<BTreeMap<String, i64>>,
}

#[derive(Debug, Deserialize)]
struct PiperLanguage {
    /// Language code such as `en_US`.
    #[serde(default)]
    code: Option<String>,
}

/// A voice id is a single path segment restricted to the rhasspy naming
/// alphabet. This also guards the file routes against path traversal.
fn is_valid_voice_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 255
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Primary language subtag, lowercased (e.g. `en` from `en-US` or `en_US-amy`).
fn primary_subtag(s: &str) -> String {
    s.split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn build_voice_info(id: &str, cfg: PiperConfig) -> VoiceInfo {
    let segments: Vec<&str> = id.split('-').collect();
    let lang_region = segments.first().copied().unwrap_or("");
    let quality = segments
        .last()
        .filter(|_| segments.len() >= 2)
        .map(|s| s.to_string())
        .unwrap_or_default();
    // The name is everything between the lang_region and the quality; fall back
    // to the config `dataset`.
    let parsed_name = if segments.len() >= 3 {
        segments[1..segments.len() - 1].join("-")
    } else {
        String::new()
    };
    let name = cfg
        .dataset
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or(parsed_name);

    let lang = cfg
        .language
        .as_ref()
        .and_then(|l| l.code.clone())
        .filter(|s| !s.is_empty())
        .map(|c| c.replace('_', "-"))
        .unwrap_or_else(|| lang_region.replace('_', "-"));

    let num_speakers = cfg.num_speakers.unwrap_or(1);
    let speakers = if num_speakers > 1 {
        cfg.speaker_id_map.as_ref().map(|map| {
            let mut entries: Vec<(&String, &i64)> = map.iter().collect();
            entries.sort_by_key(|(_, id)| **id);
            entries.into_iter().map(|(name, _)| name.clone()).collect()
        })
    } else {
        None
    };

    VoiceInfo {
        id: id.to_string(),
        name,
        lang,
        quality,
        num_speakers,
        speakers,
    }
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(get, path = "/piper/voices", tag = "TTS", operation_id = "listPiperVoices",
    params(VoicesQuery),
    responses(
        (status = StatusCode::OK, description = "Available Piper voices", body = Vec<VoiceInfo>),
    ))
)]
pub async fn list_voices(
    State(state): State<AppState>,
    Query(query): Query<VoicesQuery>,
) -> ApiResult<Json<Vec<VoiceInfo>>> {
    let dir = state
        .config()
        .voices_dir
        .as_ref()
        .ok_or_else(|| ApiError::ResourceNotFound("voices".into()))?;

    let filter = query.language.as_deref().map(primary_subtag);

    let mut read_dir = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| ApiError::InternalError(format!("Cannot read voices directory: {e}")))?;

    let mut voices = Vec::new();
    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(|e| ApiError::InternalError(format!("Cannot read voices directory: {e}")))?
    {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        // Listing a language = scanning `*.onnx.json`.
        let Some(id) = file_name.strip_suffix(".onnx.json") else {
            continue;
        };
        if !is_valid_voice_id(id) {
            continue;
        }
        if let Some(ref subtag) = filter {
            if &primary_subtag(id) != subtag {
                continue;
            }
        }
        // A voice is only usable if the model file exists too.
        let model = dir.join(format!("{id}.onnx"));
        if !tokio::fs::try_exists(&model).await.unwrap_or(false) {
            continue;
        }

        match tokio::fs::read(entry.path()).await {
            Ok(bytes) => match serde_json::from_slice::<PiperConfig>(&bytes) {
                Ok(cfg) => voices.push(build_voice_info(id, cfg)),
                Err(e) => tracing::warn!("Ignoring invalid Piper config {file_name}: {e}"),
            },
            Err(e) => tracing::warn!("Cannot read Piper config {file_name}: {e}"),
        }
    }

    voices.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(voices))
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(get, path = "/piper/voices/{id}.onnx", tag = "TTS", operation_id = "getPiperVoiceModel",
    params(("id" = String, Path, description = "Voice id, e.g. en_US-amy-medium")),
    responses(
        (status = StatusCode::OK, description = "Piper ONNX model file", content_type = "application/octet-stream"),
        (status = StatusCode::NOT_FOUND, description = "Voice not found"),
    ))
)]
async fn get_model(state: &AppState, id: &str, req: Request) -> ApiResult<Response> {
    serve_voice_file(state, id, &format!("{id}.onnx"), MIME_ONNX, req).await
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(get, path = "/piper/voices/{id}.onnx.json", tag = "TTS", operation_id = "getPiperVoiceConfig",
    params(("id" = String, Path, description = "Voice id, e.g. en_US-amy-medium")),
    responses(
        (status = StatusCode::OK, description = "Piper voice config file", content_type = "application/json"),
        (status = StatusCode::NOT_FOUND, description = "Voice not found"),
    ))
)]
async fn get_config(state: &AppState, id: &str, req: Request) -> ApiResult<Response> {
    serve_voice_file(state, id, &format!("{id}.onnx.json"), MIME_JSON, req).await
}

/// Single axum route for both file kinds; `matchit` cannot mix a capture with a
/// literal suffix in one segment, so we capture the whole file name and dispatch
/// on the suffix.
pub async fn get_voice_file(
    State(state): State<AppState>,
    AxumPath(file): AxumPath<String>,
    req: Request,
) -> ApiResult<Response> {
    if let Some(id) = file.strip_suffix(".onnx.json") {
        get_config(&state, id, req).await
    } else if let Some(id) = file.strip_suffix(".onnx") {
        get_model(&state, id, req).await
    } else {
        Err(ApiError::ResourceNotFound("voice file".into()))
    }
}

async fn serve_voice_file(
    state: &AppState,
    id: &str,
    file_name: &str,
    mime: &'static str,
    req: Request,
) -> ApiResult<Response> {
    let dir = state
        .config()
        .voices_dir
        .as_ref()
        .ok_or_else(|| ApiError::ResourceNotFound("voices".into()))?;
    if !is_valid_voice_id(id) {
        return Err(ApiError::InvalidRequest("Invalid voice id".into()));
    }

    let full_path = dir.join(file_name);
    let meta = match tokio::fs::metadata(&full_path).await {
        Ok(meta) if meta.is_file() => meta,
        _ => return Err(ApiError::ResourceNotFound("voice".into())),
    };
    let etag = make_etag(&meta);

    // Honour If-None-Match ourselves (ServeFile only understands
    // If-Modified-Since / Range).
    if let Some(if_none_match) = req.headers().get(header::IF_NONE_MATCH) {
        if if_none_match_matches(if_none_match, &etag) {
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::NOT_MODIFIED;
            add_asset_headers(state, response.headers_mut(), None, &etag);
            return Ok(response);
        }
    }

    // ServeFile handles content streaming, Range and conditional (Last-Modified)
    // requests; its error type is `Infallible`.
    let response = match ServeFile::new(&full_path).oneshot(req).await {
        Ok(response) => response,
        Err(never) => match never {},
    };
    let (mut parts, body) = response.into_parts();
    // Only assert our content type on a real body response.
    let mime = matches!(parts.status, StatusCode::OK | StatusCode::PARTIAL_CONTENT).then_some(mime);
    add_asset_headers(state, &mut parts.headers, mime, &etag);
    Ok(Response::from_parts(parts, Body::new(body)))
}

/// Weak-ish validator derived from file size and mtime.
fn make_etag(meta: &std::fs::Metadata) -> String {
    let len = meta.len();
    let modified = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("\"{len:x}-{modified:x}\"")
}

fn if_none_match_matches(header: &HeaderValue, etag: &str) -> bool {
    match header.to_str() {
        Ok(value) => {
            let value = value.trim();
            value == "*"
                || value.split(',').any(|tag| {
                    let tag = tag.trim();
                    tag.strip_prefix("W/").unwrap_or(tag) == etag
                })
        }
        Err(_) => false,
    }
}

fn coep_header_value(mode: &str) -> HeaderValue {
    match mode {
        "credentialless" => HeaderValue::from_static("credentialless"),
        _ => HeaderValue::from_static("require-corp"),
    }
}

fn add_asset_headers(
    state: &AppState,
    headers: &mut HeaderMap,
    mime: Option<&'static str>,
    etag: &str,
) {
    if let Some(mime) = mime {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CACHE_CONTROL_IMMUTABLE),
    );
    if let Ok(value) = HeaderValue::from_str(etag) {
        headers.insert(header::ETAG, value);
    }
    headers.insert(NOSNIFF, HeaderValue::from_static("nosniff"));

    // Cross-origin isolation: CORP lets the isolated SPA embed these assets, the
    // COOP/COEP pair keeps the responses consistent with the app document.
    if state.config().cross_origin_isolation {
        headers.insert(CORP, HeaderValue::from_static("same-origin"));
        headers.insert(COEP, coep_header_value(&state.config().coep_mode));
        headers.insert(COOP, HeaderValue::from_static("same-origin"));
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/piper/voices", get(list_voices))
        .route("/piper/voices/{file}", get(get_voice_file))
}

#[cfg(feature = "openapi")]
pub fn api_docs() -> utoipa::openapi::OpenApi {
    use utoipa::OpenApi as _;
    #[derive(utoipa::OpenApi)]
    #[openapi(paths(list_voices, get_model, get_config))]
    struct ApiDoc;
    ApiDoc::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_voice_ids() {
        assert!(is_valid_voice_id("en_US-amy-medium"));
        assert!(is_valid_voice_id("cs_CZ-jirka-low"));
        assert!(!is_valid_voice_id(""));
        assert!(!is_valid_voice_id("../etc/passwd"));
        assert!(!is_valid_voice_id("foo/bar"));
        assert!(!is_valid_voice_id("foo.bar"));
    }

    #[test]
    fn primary_subtags() {
        assert_eq!(primary_subtag("en-US"), "en");
        assert_eq!(primary_subtag("en_US"), "en");
        assert_eq!(primary_subtag("EN"), "en");
        assert_eq!(primary_subtag("en_US-amy-medium"), "en");
    }

    #[test]
    fn build_single_speaker() {
        let cfg = PiperConfig {
            num_speakers: Some(1),
            dataset: Some("amy".to_string()),
            language: Some(PiperLanguage {
                code: Some("en_US".to_string()),
            }),
            speaker_id_map: None,
        };
        let info = build_voice_info("en_US-amy-medium", cfg);
        assert_eq!(info.id, "en_US-amy-medium");
        assert_eq!(info.name, "amy");
        assert_eq!(info.lang, "en-US");
        assert_eq!(info.quality, "medium");
        assert_eq!(info.num_speakers, 1);
        assert!(info.speakers.is_none());
    }

    #[test]
    fn build_multi_speaker_sorts_by_id() {
        let mut map = BTreeMap::new();
        map.insert("bravo".to_string(), 1);
        map.insert("alpha".to_string(), 0);
        let cfg = PiperConfig {
            num_speakers: Some(2),
            dataset: None,
            language: None,
            speaker_id_map: Some(map),
        };
        let info = build_voice_info("en_GB-vctk-medium", cfg);
        assert_eq!(info.name, "vctk");
        assert_eq!(info.lang, "en-GB");
        assert_eq!(info.num_speakers, 2);
        assert_eq!(
            info.speakers,
            Some(vec!["alpha".to_string(), "bravo".to_string()])
        );
    }

    #[test]
    fn if_none_match() {
        let etag = "\"a-b\"";
        assert!(if_none_match_matches(
            &HeaderValue::from_static("\"a-b\""),
            etag
        ));
        assert!(if_none_match_matches(&HeaderValue::from_static("*"), etag));
        assert!(if_none_match_matches(
            &HeaderValue::from_static("W/\"a-b\""),
            etag
        ));
        assert!(if_none_match_matches(
            &HeaderValue::from_static("\"x\", \"a-b\""),
            etag
        ));
        assert!(!if_none_match_matches(
            &HeaderValue::from_static("\"other\""),
            etag
        ));
    }
}
