use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use tokio::net::TcpListener;

const INDEX_HTML: &str = include_str!("index.html");

// 50 MB — comfortably handles WAV, FLAC, and large MP3 files
const MAX_UPLOAD_BYTES: usize = 50 * 1024 * 1024;

pub(crate) fn router() -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/simulate", post(simulate_handler))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
}

pub async fn run(port: u16) {
    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).await.expect("failed to bind port");
    println!("Listening on http://localhost:{port}");
    axum::serve(listener, router()).await.unwrap();
}

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn simulate_handler(mut multipart: Multipart) -> Response {
    let mut audio_bytes: Vec<u8> = Vec::new();
    let mut audio_ext: Option<String> = None;
    let mut channels: usize = 8;
    let mut strategy = crate::vocoder::Strategy::Cis;

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name() {
            Some("audio") => {
                // Capture extension before consuming the field body
                audio_ext = field
                    .file_name()
                    .and_then(|name| std::path::Path::new(name).extension())
                    .and_then(|ext| ext.to_str())
                    .map(|s| s.to_lowercase());
                if let Ok(bytes) = field.bytes().await {
                    audio_bytes = bytes.to_vec();
                }
            }
            Some("channels") => {
                if let Ok(text) = field.text().await {
                    channels = text.parse().unwrap_or(8).clamp(1, 64);
                }
            }
            Some("strategy") => {
                if let Ok(text) = field.text().await {
                    strategy = match text.trim() {
                        "fs4" => crate::vocoder::Strategy::Fs4,
                        "fft" => crate::vocoder::Strategy::Fft,
                        _ => crate::vocoder::Strategy::Cis,
                    };
                }
            }
            _ => {}
        }
    }

    if audio_bytes.is_empty() {
        return (StatusCode::BAD_REQUEST, "missing audio field").into_response();
    }

    // Offload CPU-bound processing off the async executor
    let result = tokio::task::spawn_blocking(move || {
        crate::audio::decode_audio_bytes(&audio_bytes, audio_ext.as_deref()).and_then(|(samples, rate)| {
            let output = crate::vocoder::process(&samples, rate, channels, strategy, crate::vocoder::Carrier::Noise);
            crate::vocoder::encode_wav_bytes(&output, rate)
        })
    })
    .await
    .unwrap();

    match result {
        Ok(wav_bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "audio/wav")
            .header(header::CONTENT_DISPOSITION, "inline; filename=\"simulated.wav\"")
            .body(Body::from(wav_bytes))
            .unwrap(),
        Err(e) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(e))
            .unwrap(),
    }
}

#[cfg(test)]
mod server_tests {
    use super::*;
    use axum::{body::to_bytes, http::Request};
    use tower::ServiceExt; // for `.oneshot()`

    /// Build a minimal synthetic WAV (0.1 s, 440 Hz sine, 44100 Hz).
    fn test_wav_bytes() -> Vec<u8> {
        let signal: Vec<f32> = (0..4410)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
        crate::vocoder::encode_wav_bytes(&signal, 44100).expect("test WAV encode failed")
    }

    /// Build a multipart/form-data POST body for /simulate.
    fn simulate_body(wav: &[u8], strategy: &str, channels: u8, boundary: &str) -> Vec<u8> {
        let mut body: Vec<u8> = Vec::new();
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"strategy\"\r\n\r\n{strategy}\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"channels\"\r\n\r\n{channels}\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"test.wav\"\r\nContent-Type: audio/wav\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(wav);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        body
    }

    async fn post_simulate(wav: &[u8], strategy: &str, channels: u8) -> axum::response::Response {
        let boundary = "testbnd";
        let body = simulate_body(wav, strategy, channels, boundary);
        let req = Request::builder()
            .method("POST")
            .uri("/simulate")
            .header("content-type", format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(body))
            .unwrap();
        router().oneshot(req).await.unwrap()
    }

    /// GET / must return 200 with an HTML body.
    #[tokio::test]
    async fn get_index_returns_html() {
        let req = Request::builder()
            .uri("/")
            .body(Body::empty())
            .unwrap();
        let resp = router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.contains("text/html"), "expected text/html content-type, got {ct}");
    }

    /// POST /simulate with no audio field must return 400 Bad Request.
    #[tokio::test]
    async fn simulate_missing_audio_returns_400() {
        let boundary = "testboundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"channels\"\r\n\r\n8\r\n--{boundary}--\r\n"
        );
        let req = Request::builder()
            .method("POST")
            .uri("/simulate")
            .header("content-type", format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(body))
            .unwrap();
        let resp = router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// POST /simulate with a valid WAV (default CIS strategy, 8 channels)
    /// must return 200 audio/wav with a RIFF header.
    #[tokio::test]
    async fn simulate_valid_wav_returns_wav_response() {
        let resp = post_simulate(&test_wav_bytes(), "cis", 8).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert_eq!(ct, "audio/wav", "expected audio/wav, got {ct}");
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[0..4], b"RIFF", "response must be a valid WAV");
    }

    /// FS4 strategy must also succeed end-to-end through the HTTP route.
    #[tokio::test]
    async fn simulate_fs4_strategy_returns_wav() {
        let resp = post_simulate(&test_wav_bytes(), "fs4", 8).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[0..4], b"RIFF", "FS4 response must be a valid WAV");
    }

    /// 4-channel count must produce a valid WAV — exercises the non-default
    /// channel path used in the "very degraded" demo mode.
    #[tokio::test]
    async fn simulate_4_channels_returns_wav() {
        let resp = post_simulate(&test_wav_bytes(), "cis", 4).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[0..4], b"RIFF", "4-channel response must be a valid WAV");
    }

    /// 16-channel count must also work.
    #[tokio::test]
    async fn simulate_16_channels_returns_wav() {
        let resp = post_simulate(&test_wav_bytes(), "fs4", 16).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[0..4], b"RIFF", "16-channel response must be a valid WAV");
    }

    /// An unrecognised strategy string must fall back to CIS rather than erroring.
    /// The server uses `_ => Strategy::Cis` so "invalid" must still succeed.
    #[tokio::test]
    async fn simulate_unknown_strategy_falls_back_to_cis() {
        let resp = post_simulate(&test_wav_bytes(), "invalid_strategy", 8).await;
        assert_eq!(
            resp.status(), StatusCode::OK,
            "unknown strategy must fall back to CIS and return 200"
        );
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
    }

    /// Channel count 0 is clamped to 1 server-side; must not panic or 500.
    #[tokio::test]
    async fn simulate_channel_count_zero_is_clamped() {
        // channels=0 → clamp(1,64) → 1
        let resp = post_simulate(&test_wav_bytes(), "cis", 0).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
    }

    /// Submitting garbage bytes as the audio field must return 500, not panic.
    #[tokio::test]
    async fn simulate_invalid_audio_bytes_returns_500() {
        let garbage = b"this is not a wav file at all".to_vec();
        let resp = post_simulate(&garbage, "cis", 8).await;
        assert_eq!(
            resp.status(), StatusCode::INTERNAL_SERVER_ERROR,
            "invalid audio bytes must produce 500, not a panic"
        );
    }
}
