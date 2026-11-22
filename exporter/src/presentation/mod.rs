//! Presentation layer: the HTTP API scraped by Prometheus.

mod http;
mod metrics;

pub use http::{AppState, router};
