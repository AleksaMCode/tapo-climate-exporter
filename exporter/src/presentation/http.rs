use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use super::metrics;
use crate::application::ClimateCache;

#[derive(Clone)]
pub struct AppState {
    pub cache: ClimateCache,
    pub expected_sensor_count: u32,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .with_state(state)
}

/// Served from the cache only; a scrape never touches the hub.
async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let body = metrics::render(&state.cache.snapshot(), state.expected_sensor_count);
    (
        [(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}

/// Liveness: always 200 while the process is serving. Hub problems are
/// reported in the body and in the metrics, not as a failing health check,
/// so the container is not restarted just because the hub is unreachable.
async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let snapshot = state.cache.snapshot();
    Json(json!({
        "status": "ok",
        "hub_up": snapshot.hub_up,
        "sensors_reported": snapshot.sensors.len(),
        "sensors_expected": state.expected_sensor_count,
        "last_successful_poll_unix_seconds": snapshot.last_success_unix_seconds,
    }))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::*;
    use crate::domain::{Placement, SensorReading};

    fn app(cache: ClimateCache) -> Router {
        router(AppState {
            cache,
            expected_sensor_count: 5,
        })
    }

    fn cache_with_one_sensor() -> ClimateCache {
        let cache = ClimateCache::new(Duration::from_secs(300));
        cache.record_success(vec![SensorReading {
            device_id: "id-1".into(),
            name: "Kitchen".into(),
            mac: "AA".into(),
            model: "T310".into(),
            placement: Placement::Indoor,
            online: true,
            temperature_celsius: 21.5,
            humidity_percent: 48.0,
            battery_low: false,
            rssi_dbm: -57,
            signal_level: 3,
        }]);
        cache
    }

    /// Sends a GET through the router without opening a socket.
    async fn get(app: Router, path: &str) -> (StatusCode, String, String) {
        let response = app
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            content_type,
            String::from_utf8(body.to_vec()).unwrap(),
        )
    }

    #[tokio::test]
    async fn metrics_serves_cached_readings_as_prometheus_text() {
        let (status, content_type, body) = get(app(cache_with_one_sensor()), "/metrics").await;

        assert_eq!(status, StatusCode::OK);
        assert!(content_type.starts_with("text/plain; version=0.0.4"));
        assert!(body.contains("tapo_hub_up 1\n"));
        assert!(body.contains("tapo_sensors_expected 5\n"));
        assert!(body.contains(
            "tapo_sensor_temperature_celsius{device_id=\"id-1\",name=\"Kitchen\",placement=\"indoor\"} 21.5\n"
        ));
    }

    #[tokio::test]
    async fn health_reports_cache_state() {
        let (status, content_type, body) = get(app(cache_with_one_sensor()), "/health").await;
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(status, StatusCode::OK);
        assert!(content_type.starts_with("application/json"));
        assert_eq!(json["status"], "ok");
        assert_eq!(json["hub_up"], true);
        assert_eq!(json["sensors_reported"], 1);
        assert_eq!(json["sensors_expected"], 5);
        assert!(json["last_successful_poll_unix_seconds"].is_number());
    }

    #[tokio::test]
    async fn health_stays_ok_while_the_hub_is_down() {
        let cache = ClimateCache::new(Duration::from_secs(300));
        cache.record_failure();
        let (status, _, body) = get(app(cache), "/health").await;
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["hub_up"], false);
        assert_eq!(json["sensors_reported"], 0);
        assert!(json["last_successful_poll_unix_seconds"].is_null());
    }

    #[tokio::test]
    async fn unknown_paths_return_404() {
        let (status, _, _) = get(app(cache_with_one_sensor()), "/healthz").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
