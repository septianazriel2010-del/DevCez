use futures_util::stream;
use reqwest::{header, Client, StatusCode};
use serde::Serialize;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, State};

const EVENT_NAME: &str = "network-speed-test";
const DOWNLOAD_URL: &str = "https://speed.cloudflare.com/__down?bytes=";
const UPLOAD_URL: &str = "https://speed.cloudflare.com/__up";
const LATENCY_SAMPLES: usize = 5;
const LATENCY_WARMUP_REQUESTS: usize = 1;
const DOWNLOAD_BYTES: usize = 8 * 1024 * 1024;
const UPLOAD_BYTES: usize = 4 * 1024 * 1024;
const MAX_DOWNLOAD_BYTES: usize = 8 * 1024 * 1024;
const MAX_UPLOAD_BYTES: usize = 4 * 1024 * 1024;
const UPLOAD_CHUNK_BYTES: usize = 256 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SpeedTestUpdate {
    pub test_id: String,
    pub status: String,
    pub phase: String,
    pub progress_percent: u8,
    pub message: String,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub ping_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
}

struct ActiveSpeedTest {
    id: String,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone, Default)]
pub struct SpeedTestManager {
    active: Arc<Mutex<Option<ActiveSpeedTest>>>,
}

#[derive(Debug)]
enum SpeedTestError {
    Cancelled,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Measurements {
    download_mbps: f64,
    upload_mbps: f64,
    ping_ms: f64,
    jitter_ms: f64,
}

#[tauri::command]
pub fn start_speed_test(
    app: AppHandle,
    manager: State<'_, SpeedTestManager>,
) -> Result<String, String> {
    let id = new_test_id();
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut active = manager
            .active
            .lock()
            .map_err(|_| "Speed Test state is temporarily unavailable.".to_string())?;
        if active.is_some() {
            return Err("A Speed Test is already running.".to_string());
        }
        *active = Some(ActiveSpeedTest {
            id: id.clone(),
            cancelled: Arc::clone(&cancelled),
        });
    }

    emit_update(
        &app,
        SpeedTestUpdate {
            test_id: id.clone(),
            status: "running".to_string(),
            phase: "starting".to_string(),
            progress_percent: 0,
            message: "Preparing a real internet speed test.".to_string(),
            download_mbps: None,
            upload_mbps: None,
            ping_ms: None,
            jitter_ms: None,
        },
    );

    let app_handle = app.clone();
    let manager = manager.inner().clone();
    let test_id = id.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_speed_test(&app_handle, &test_id, Arc::clone(&cancelled)).await;
        match result {
            Ok(measurements) => emit_update(
                &app_handle,
                SpeedTestUpdate {
                    test_id: test_id.clone(),
                    status: "completed".to_string(),
                    phase: "complete".to_string(),
                    progress_percent: 100,
                    message: "Speed Test completed.".to_string(),
                    download_mbps: Some(measurements.download_mbps),
                    upload_mbps: Some(measurements.upload_mbps),
                    ping_ms: Some(measurements.ping_ms),
                    jitter_ms: Some(measurements.jitter_ms),
                },
            ),
            Err(SpeedTestError::Cancelled) => emit_update(
                &app_handle,
                terminal_update(&test_id, "cancelled", "cancelled", "Speed Test cancelled."),
            ),
            Err(SpeedTestError::Failed(error)) => emit_update(
                &app_handle,
                terminal_update(&test_id, "error", "error", &error),
            ),
        }
        if let Ok(mut active) = manager.active.lock() {
            if active.as_ref().is_some_and(|session| session.id == test_id) {
                *active = None;
            }
        }
    });

    Ok(id)
}

#[tauri::command]
pub fn stop_speed_test(
    test_id: String,
    manager: State<'_, SpeedTestManager>,
) -> Result<(), String> {
    let active = manager
        .active
        .lock()
        .map_err(|_| "Speed Test state is temporarily unavailable.".to_string())?;
    let session = active
        .as_ref()
        .filter(|session| session.id == test_id)
        .ok_or_else(|| "This Speed Test is no longer running.".to_string())?;
    session.cancelled.store(true, Ordering::SeqCst);
    Ok(())
}

async fn run_speed_test(
    app: &AppHandle,
    test_id: &str,
    cancelled: Arc<AtomicBool>,
) -> Result<Measurements, SpeedTestError> {
    ensure_not_cancelled(&cancelled)?;
    let client = Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| {
            SpeedTestError::Failed(format!("Could not prepare HTTPS test: {error}"))
        })?;

    emit_phase(app, test_id, "latency", 4, "Measuring connection latency.");
    let mut latencies = Vec::with_capacity(LATENCY_SAMPLES);
    for warmup in 0..LATENCY_WARMUP_REQUESTS {
        let _ = latency_probe(&client, Arc::clone(&cancelled)).await?;
        emit_phase(
            app,
            test_id,
            "latency",
            7 + warmup as u8,
            "Preparing the HTTPS connection.",
        );
    }
    for sample in 0..LATENCY_SAMPLES {
        let latency = latency_probe(&client, Arc::clone(&cancelled)).await?;
        latencies.push(latency);
        let progress = 10 + (((sample + 1) * 20) / LATENCY_SAMPLES) as u8;
        emit_phase(
            app,
            test_id,
            "latency",
            progress,
            &format!("Latency sample {} of {LATENCY_SAMPLES}.", sample + 1),
        );
    }
    let ping_ms = mean(&latencies).ok_or_else(|| {
        SpeedTestError::Failed("Latency measurements were unavailable.".to_string())
    })?;
    let jitter_ms = mean_absolute_consecutive_difference(&latencies).ok_or_else(|| {
        SpeedTestError::Failed("Jitter measurements were unavailable.".to_string())
    })?;

    emit_phase(
        app,
        test_id,
        "download",
        32,
        "Downloading a bounded test payload.",
    );
    let download_mbps = measure_download(&client, app, test_id, Arc::clone(&cancelled)).await?;

    emit_phase(
        app,
        test_id,
        "upload",
        68,
        "Uploading a bounded test payload.",
    );
    let upload_mbps = measure_upload(&client, app, test_id, Arc::clone(&cancelled)).await?;

    ensure_not_cancelled(&cancelled)?;
    Ok(Measurements {
        download_mbps,
        upload_mbps,
        ping_ms,
        jitter_ms,
    })
}

async fn latency_probe(client: &Client, cancelled: Arc<AtomicBool>) -> Result<f64, SpeedTestError> {
    let started = Instant::now();
    let response = cancellable(
        client.get(format!("{DOWNLOAD_URL}0")).send(),
        Arc::clone(&cancelled),
    )
    .await?;
    ensure_success(response.status(), "latency")?;
    let body = cancellable(response.bytes(), cancelled).await?;
    if !body.is_empty() {
        return Err(SpeedTestError::Failed(
            "The latency endpoint returned an unexpected response body.".to_string(),
        ));
    }
    Ok(started.elapsed().as_secs_f64() * 1000.0)
}

async fn measure_download(
    client: &Client,
    app: &AppHandle,
    test_id: &str,
    cancelled: Arc<AtomicBool>,
) -> Result<f64, SpeedTestError> {
    let started = Instant::now();
    let request = client
        .get(format!("{DOWNLOAD_URL}{DOWNLOAD_BYTES}"))
        .header(header::CACHE_CONTROL, "no-cache");
    let mut response = cancellable(request.send(), Arc::clone(&cancelled)).await?;
    ensure_success(response.status(), "download")?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DOWNLOAD_BYTES as u64)
    {
        return Err(SpeedTestError::Failed(
            "The download server returned a payload larger than the configured limit.".to_string(),
        ));
    }

    let mut received = 0usize;
    loop {
        ensure_not_cancelled(&cancelled)?;
        let chunk = cancellable(response.chunk(), Arc::clone(&cancelled)).await?;
        let Some(chunk) = chunk else {
            break;
        };
        received = received
            .checked_add(chunk.len())
            .ok_or_else(|| SpeedTestError::Failed("Download byte count overflowed.".to_string()))?;
        if received > MAX_DOWNLOAD_BYTES {
            return Err(SpeedTestError::Failed(
                "The download exceeded the configured 8 MiB data cap.".to_string(),
            ));
        }
        let progress = 32 + ((received * 33) / DOWNLOAD_BYTES).min(33) as u8;
        emit_phase(
            app,
            test_id,
            "download",
            progress,
            &format!(
                "Downloaded {} of {} MiB.",
                received / (1024 * 1024),
                DOWNLOAD_BYTES / (1024 * 1024)
            ),
        );
    }
    if received != DOWNLOAD_BYTES {
        return Err(SpeedTestError::Failed(format!(
            "The download was incomplete ({received} of {DOWNLOAD_BYTES} bytes)."
        )));
    }
    Ok(throughput_mbps(received, started.elapsed()))
}

async fn measure_upload(
    client: &Client,
    app: &AppHandle,
    test_id: &str,
    cancelled: Arc<AtomicBool>,
) -> Result<f64, SpeedTestError> {
    if UPLOAD_BYTES > MAX_UPLOAD_BYTES {
        return Err(SpeedTestError::Failed(
            "The configured upload exceeds its safety limit.".to_string(),
        ));
    }
    let payload = vec![0_u8; UPLOAD_BYTES];
    let app_handle = app.clone();
    let event_test_id = test_id.to_string();
    let body = upload_body(payload, move |sent| {
        let progress = 70 + ((sent * 25) / UPLOAD_BYTES) as u8;
        emit_phase(
            &app_handle,
            &event_test_id,
            "upload",
            progress,
            &format!("Uploading {sent} of {UPLOAD_BYTES} bytes."),
        );
    });
    let started = Instant::now();
    let response = cancellable(
        client
            .post(UPLOAD_URL)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(body)
            .send(),
        Arc::clone(&cancelled),
    )
    .await?;
    ensure_success(response.status(), "upload")?;
    let received_bytes = response
        .headers()
        .get("cf-meta-upload-bytes")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            SpeedTestError::Failed(
                "Cloudflare did not confirm how many upload bytes it received.".to_string(),
            )
        })?;
    if received_bytes != UPLOAD_BYTES {
        return Err(SpeedTestError::Failed(format!(
            "Cloudflare confirmed {received_bytes} of {UPLOAD_BYTES} upload bytes."
        )));
    }
    if let Some(length) = response.content_length() {
        if length > 64 * 1024 {
            return Err(SpeedTestError::Failed(
                "The upload endpoint returned an unexpectedly large response.".to_string(),
            ));
        }
    }
    let _ = cancellable(response.bytes(), Arc::clone(&cancelled)).await?;
    emit_phase(
        app,
        test_id,
        "upload",
        96,
        "Upload transfer confirmed by the test server.",
    );
    Ok(throughput_mbps(received_bytes, started.elapsed()))
}

fn upload_body<F>(payload: Vec<u8>, on_progress: F) -> reqwest::Body
where
    F: FnMut(usize) + Send + 'static,
{
    reqwest::Body::wrap_stream(stream::unfold(
        (0usize, payload, on_progress),
        |(sent, payload, mut on_progress)| async move {
            if sent >= payload.len() {
                return None;
            }
            let next = (sent + UPLOAD_CHUNK_BYTES).min(payload.len());
            let chunk = payload[sent..next].to_vec();
            on_progress(next);
            Some((
                Ok::<Vec<u8>, std::io::Error>(chunk),
                (next, payload, on_progress),
            ))
        },
    ))
}

async fn cancellable<F, T, E>(request: F, cancelled: Arc<AtomicBool>) -> Result<T, SpeedTestError>
where
    F: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    tokio::select! {
        result = request => result.map_err(|error| SpeedTestError::Failed(network_error(&error.to_string()))),
        _ = wait_for_cancellation(cancelled) => Err(SpeedTestError::Cancelled),
    }
}

async fn wait_for_cancellation(cancelled: Arc<AtomicBool>) {
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

fn network_error(error: &str) -> String {
    if error.contains("dns") || error.contains("resolve") || error.contains("name or service") {
        format!("Could not resolve the Cloudflare Speed Test server: {error}")
    } else if error.contains("timed out") {
        format!("The network request timed out: {error}")
    } else if error.contains("connect") {
        format!("Could not connect to Cloudflare Speed Test: {error}")
    } else {
        format!("The network request failed: {error}")
    }
}

fn ensure_success(status: StatusCode, phase: &str) -> Result<(), SpeedTestError> {
    if status.is_success() {
        Ok(())
    } else {
        Err(SpeedTestError::Failed(format!(
            "Cloudflare Speed Test {phase} request failed with HTTP {status}."
        )))
    }
}

fn ensure_not_cancelled(cancelled: &AtomicBool) -> Result<(), SpeedTestError> {
    if cancelled.load(Ordering::SeqCst) {
        Err(SpeedTestError::Cancelled)
    } else {
        Ok(())
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn mean_absolute_consecutive_difference(values: &[f64]) -> Option<f64> {
    let differences = values
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .collect::<Vec<_>>();
    mean(&differences)
}

fn throughput_mbps(bytes: usize, duration: Duration) -> f64 {
    (bytes as f64 * 8.0) / duration.as_secs_f64().max(0.000_001) / 1_000_000.0
}

fn new_test_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    let sequence = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
    format!("speed-{timestamp}-{}-{sequence}", std::process::id())
}

fn emit_phase(app: &AppHandle, test_id: &str, phase: &str, progress: u8, message: &str) {
    emit_update(
        app,
        SpeedTestUpdate {
            test_id: test_id.to_string(),
            status: "running".to_string(),
            phase: phase.to_string(),
            progress_percent: progress.min(99),
            message: message.to_string(),
            download_mbps: None,
            upload_mbps: None,
            ping_ms: None,
            jitter_ms: None,
        },
    );
}

fn terminal_update(test_id: &str, status: &str, phase: &str, message: &str) -> SpeedTestUpdate {
    SpeedTestUpdate {
        test_id: test_id.to_string(),
        status: status.to_string(),
        phase: phase.to_string(),
        progress_percent: 0,
        message: message.to_string(),
        download_mbps: None,
        upload_mbps: None,
        ping_ms: None,
        jitter_ms: None,
    }
}

fn emit_update(app: &AppHandle, update: SpeedTestUpdate) {
    if let Err(error) = app.emit(EVENT_NAME, update) {
        eprintln!("Could not send Network Speed Test update to the frontend: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_mean_latency_and_mean_absolute_jitter() {
        let samples = [10.0, 12.0, 9.0, 13.0];
        assert_eq!(mean(&samples), Some(11.0));
        assert_eq!(mean_absolute_consecutive_difference(&samples), Some(3.0));
    }

    #[test]
    fn calculates_mbps_from_actual_bytes_and_duration() {
        assert_eq!(throughput_mbps(1_000_000, Duration::from_secs(1)), 8.0);
        assert_eq!(throughput_mbps(2_000_000, Duration::from_secs(2)), 8.0);
    }

    #[test]
    fn no_measurement_is_reported_for_empty_samples() {
        assert_eq!(mean(&[]), None);
        assert_eq!(mean_absolute_consecutive_difference(&[12.0]), None);
    }

    #[tokio::test]
    async fn cancellation_interrupts_an_in_flight_future() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = tokio::time::sleep(Duration::from_secs(30));
        let signal = Arc::clone(&cancelled);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            signal.store(true, Ordering::SeqCst);
        });
        let result = cancellable(
            async {
                request.await;
                Ok::<(), std::io::Error>(())
            },
            cancelled,
        )
        .await;
        assert!(matches!(result, Err(SpeedTestError::Cancelled)));
    }

    #[tokio::test]
    #[ignore = "contacts Cloudflare and transfers at most 12 MiB"]
    async fn cloudflare_speed_test_measures_real_bounded_transfers() {
        let client = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut samples = Vec::with_capacity(LATENCY_SAMPLES);
        for sample in 0..LATENCY_WARMUP_REQUESTS + LATENCY_SAMPLES {
            let latency = latency_probe(&client, Arc::clone(&cancelled))
                .await
                .expect("Cloudflare latency request");
            if sample >= LATENCY_WARMUP_REQUESTS {
                samples.push(latency);
            }
        }

        let download_started = Instant::now();
        let response = client
            .get(format!("{DOWNLOAD_URL}{DOWNLOAD_BYTES}"))
            .header(header::CACHE_CONTROL, "no-cache")
            .send()
            .await
            .expect("Cloudflare download request");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.bytes().await.expect("Cloudflare download body");
        assert_eq!(body.len(), DOWNLOAD_BYTES);
        let download_mbps = throughput_mbps(body.len(), download_started.elapsed());

        let upload_progress = Arc::new(Mutex::new(Vec::new()));
        let progress = Arc::clone(&upload_progress);
        let upload_started = Instant::now();
        let response = client
            .post(UPLOAD_URL)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(upload_body(vec![0_u8; UPLOAD_BYTES], move |sent| {
                if let Ok(mut progress) = progress.lock() {
                    progress.push(sent);
                }
            }))
            .send()
            .await
            .expect("Cloudflare upload request");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("cf-meta-upload-bytes")
                .unwrap()
                .to_str()
                .unwrap(),
            UPLOAD_BYTES.to_string()
        );
        let _ = response.bytes().await.expect("Cloudflare upload response");
        let upload_mbps = throughput_mbps(UPLOAD_BYTES, upload_started.elapsed());
        assert_eq!(upload_progress.lock().unwrap().last(), Some(&UPLOAD_BYTES));
        assert_eq!(samples.len(), LATENCY_SAMPLES);
        let ping_ms = mean(&samples).unwrap();
        let jitter_ms = mean_absolute_consecutive_difference(&samples).unwrap();
        assert!(download_mbps.is_finite() && download_mbps > 0.0);
        assert!(upload_mbps.is_finite() && upload_mbps > 0.0);
        assert!(ping_ms.is_finite() && ping_ms > 0.0);
        assert!(jitter_ms.is_finite());
        eprintln!(
            "Cloudflare measurements: download={download_mbps:.2} Mbps, upload={upload_mbps:.2} Mbps, ping={ping_ms:.2} ms, jitter={jitter_ms:.2} ms"
        );
        ensure_not_cancelled(&cancelled).unwrap();
    }
}
