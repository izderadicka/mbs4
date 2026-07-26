use mbs4_e2e_tests::{TestUser, launch_env, prepare_env};
use serde_json::Value;
use tracing::info;
use tracing_test::traced_test;

async fn write_voice(dir: &std::path::Path, id: &str, model_size: usize, config: &str) {
    tokio::fs::write(dir.join(format!("{id}.onnx")), vec![0u8; model_size])
        .await
        .unwrap();
    tokio::fs::write(dir.join(format!("{id}.onnx.json")), config)
        .await
        .unwrap();
}

#[tokio::test]
#[traced_test]
async fn test_piper_voices() {
    let (mut args, mut guard) = prepare_env("test_tts").await.unwrap();

    // Populate a voices directory next to the (temp) data dir.
    let voices_dir = guard.path().join("voices");
    tokio::fs::create_dir_all(&voices_dir).await.unwrap();

    write_voice(
        &voices_dir,
        "en_US-amy-medium",
        5000,
        r#"{"num_speakers":1,"dataset":"amy","language":{"code":"en_US","family":"en","region":"US"}}"#,
    )
    .await;
    write_voice(
        &voices_dir,
        "cs_CZ-jirka-low",
        3000,
        r#"{"num_speakers":1,"language":{"code":"cs_CZ"}}"#,
    )
    .await;
    write_voice(
        &voices_dir,
        "en_GB-vctk-medium",
        4000,
        r#"{"num_speakers":3,"language":{"code":"en_GB"},"speaker_id_map":{"charlie":2,"alice":0,"bob":1}}"#,
    )
    .await;
    // Config without a paired model file must be skipped in the listing.
    tokio::fs::write(
        voices_dir.join("de_DE-thorsten-medium.onnx.json"),
        r#"{"num_speakers":1,"language":{"code":"de_DE"}}"#,
    )
    .await
    .unwrap();

    args.voices_dir = Some(voices_dir);
    assert!(args.cross_origin_isolation, "isolation on by default");

    let base_url = args.base_url.clone();
    let (client, _state) = launch_env(args, TestUser::User, &mut guard).await.unwrap();

    // --- voice endpoints require authentication ---
    let anonymous = reqwest::Client::new();
    for path in [
        "tts/piper/voices",
        "tts/piper/voices/en_US-amy-medium.onnx",
        "tts/piper/voices/en_US-amy-medium.onnx.json",
    ] {
        let response = anonymous
            .get(base_url.join(path).unwrap())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 401, "{path} must require auth");
    }

    // --- listing (all) ---
    let url = base_url.join("tts/piper/voices").unwrap();
    let response = client.get(url.clone()).send().await.unwrap();
    assert!(response.status().is_success());
    let voices: Vec<Value> = response.json().await.unwrap();
    info!("voices: {voices:#?}");
    // 3 usable voices (orphan de_DE skipped), sorted by id.
    let ids: Vec<&str> = voices.iter().map(|v| v["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        ["cs_CZ-jirka-low", "en_GB-vctk-medium", "en_US-amy-medium"]
    );

    let amy = voices
        .iter()
        .find(|v| v["id"] == "en_US-amy-medium")
        .unwrap();
    assert_eq!(amy["name"], "amy");
    assert_eq!(amy["lang"], "en-US");
    assert_eq!(amy["quality"], "medium");
    assert_eq!(amy["num_speakers"], 1);
    assert!(
        amy.get("speakers").is_none(),
        "single speaker has no speakers"
    );

    let vctk = voices
        .iter()
        .find(|v| v["id"] == "en_GB-vctk-medium")
        .unwrap();
    assert_eq!(vctk["num_speakers"], 3);
    // Speakers ordered by speaker id: alice(0), bob(1), charlie(2).
    assert_eq!(
        vctk["speakers"],
        serde_json::json!(["alice", "bob", "charlie"])
    );

    // --- listing filtered by primary subtag ---
    let response = client
        .get(url.clone())
        .query(&[("language", "en")])
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let en_voices: Vec<Value> = response.json().await.unwrap();
    let en_ids: Vec<&str> = en_voices
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    assert_eq!(en_ids, ["en_GB-vctk-medium", "en_US-amy-medium"]);

    // A full BCP47 tag filters by its primary subtag too.
    let response = client
        .get(url.clone())
        .query(&[("language", "cs-CZ")])
        .send()
        .await
        .unwrap();
    let cs_voices: Vec<Value> = response.json().await.unwrap();
    assert_eq!(cs_voices.len(), 1);
    assert_eq!(cs_voices[0]["id"], "cs_CZ-jirka-low");

    // --- model file ---
    let model_url = base_url
        .join("tts/piper/voices/en_US-amy-medium.onnx")
        .unwrap();
    let response = client.get(model_url.clone()).send().await.unwrap();
    assert!(response.status().is_success());
    let headers = response.headers().clone();
    assert_eq!(headers["content-type"], "application/octet-stream");
    assert!(
        headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    assert_eq!(headers["cross-origin-resource-policy"], "same-origin");
    assert_eq!(headers["cross-origin-embedder-policy"], "require-corp");
    assert_eq!(headers["cross-origin-opener-policy"], "same-origin");
    assert!(headers.contains_key("etag"), "model has an ETag");
    assert!(
        headers
            .get("accept-ranges")
            .map(|v| v == "bytes")
            .unwrap_or(false),
        "range support advertised"
    );
    let etag = headers["etag"].to_str().unwrap().to_string();
    let body = response.bytes().await.unwrap();
    assert_eq!(body.len(), 5000);

    // --- conditional request (If-None-Match) => 304 ---
    let response = client
        .get(model_url.clone())
        .header("If-None-Match", &etag)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 304);

    // --- range request => 206 ---
    let response = client
        .get(model_url.clone())
        .header("Range", "bytes=0-99")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 206);
    let partial = response.bytes().await.unwrap();
    assert_eq!(partial.len(), 100);

    // --- config file ---
    let config_url = base_url
        .join("tts/piper/voices/en_US-amy-medium.onnx.json")
        .unwrap();
    let response = client.get(config_url).send().await.unwrap();
    assert!(response.status().is_success());
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(
        response.headers()["cross-origin-resource-policy"],
        "same-origin"
    );
    let cfg: Value = response.json().await.unwrap();
    assert_eq!(cfg["dataset"], "amy");

    // --- not found ---
    let response = client
        .get(base_url.join("tts/piper/voices/nope-x-low.onnx").unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 404);

    // --- unsupported suffix => not found ---
    let response = client
        .get(
            base_url
                .join("tts/piper/voices/en_US-amy-medium.txt")
                .unwrap(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 404);
}
