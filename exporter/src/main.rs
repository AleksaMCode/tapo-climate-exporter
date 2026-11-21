//! Tapo Climate Exporter: polls a Tapo H100 hub for T310 temperature and
//! humidity readings and exposes them to Prometheus.
//!
//! Layers (dependencies point inwards):
//!   presentation -> application -> domain <- infrastructure
//! This file is the composition root that wires them together.

mod application;
mod domain;
mod infrastructure;
mod presentation;

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Context;
use tracing::info;
use tracing_subscriber::EnvFilter;

use application::{ClimateCache, ClimatePoller, RetryPolicy};
use domain::PlacementPolicy;
use infrastructure::{Config, TapoHubGateway};
use presentation::AppState;

#[tokio::main]
async fn main() -> ExitCode {
    // Load a .env file from the working directory if there is one.
    // Variables already set in the real environment take precedence.
    let _ = dotenvy::dotenv();

    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return healthcheck();
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> anyhow::Result<()> {
    let config = Config::from_env().context("invalid configuration")?;
    info!(?config, "starting tapo-climate-exporter");

    let cache = ClimateCache::new(config.stale_after);

    let gateway = TapoHubGateway::new(
        config.tapo_username.clone(),
        config.tapo_password.clone(),
        config.hub_ip,
        config.tapo_request_timeout,
    );
    let poller = ClimatePoller::new(
        gateway,
        cache.clone(),
        PlacementPolicy::new(&config.outdoor_sensors),
        config.poll_interval,
        RetryPolicy {
            max_retries: config.poll_max_retries,
            min_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(15),
        },
    );
    // Runs in the background for the lifetime of the process.
    tokio::spawn(poller.run());

    let app = presentation::router(AppState {
        cache,
        expected_sensor_count: config.expected_sensor_count,
    });
    let listener = tokio::net::TcpListener::bind(config.listen_addr)
        .await
        .with_context(|| format!("cannot listen on {}", config.listen_addr))?;
    info!(addr = %config.listen_addr, "serving /metrics and /health");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server failed")?;
    info!("shut down");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

/// `tapo-climate-exporter healthcheck`: used by the Docker HEALTHCHECK so the
/// image needs no curl. Succeeds if the local /health endpoint answers 200.
fn healthcheck() -> ExitCode {
    let listen_addr = std::env::var("LISTEN_ADDR")
        .ok()
        .and_then(|addr| addr.trim().parse::<SocketAddr>().ok())
        .unwrap_or(DEFAULT_LISTEN_ADDR);

    if health_endpoint_ok(probe_addr(listen_addr)) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Used by the health check when LISTEN_ADDR is unset or unparseable; matches
/// the default in `Config`.
const DEFAULT_LISTEN_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 9101);

/// The address the health check connects to for a server listening on
/// `listen_addr`.
///
/// A concrete address is probed as configured, because the server answers
/// nowhere else. A wildcard address (`0.0.0.0`, `[::]`) cannot be connected
/// to portably, so the loopback address of the same family is used instead.
fn probe_addr(listen_addr: SocketAddr) -> SocketAddr {
    let ip = match listen_addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, listen_addr.port())
}

/// True if `GET /health` on `addr` answers with HTTP 200 within a few seconds.
fn health_endpoint_ok(addr: SocketAddr) -> bool {
    let check = || -> std::io::Result<bool> {
        let timeout = Duration::from_secs(3);
        let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response.starts_with("HTTP/1.1 200"))
    };
    check().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    /// A one-shot HTTP server on a free local port that answers with `status_line`.
    fn serve_once(status_line: &'static str) -> SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 512];
            let _ = stream.read(&mut request);
            let _ =
                stream.write_all(format!("{status_line}\r\nContent-Length: 0\r\n\r\n").as_bytes());
        });
        addr
    }

    #[test]
    fn healthcheck_passes_on_http_200() {
        assert!(health_endpoint_ok(serve_once("HTTP/1.1 200 OK")));
    }

    #[test]
    fn healthcheck_fails_on_an_error_status() {
        assert!(!health_endpoint_ok(serve_once(
            "HTTP/1.1 503 Service Unavailable"
        )));
    }

    #[test]
    fn healthcheck_fails_when_nothing_is_listening() {
        // Bind to get a free port, then close it again.
        let addr = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap();
        assert!(!health_endpoint_ok(addr));
    }

    #[test]
    fn probe_uses_ipv4_loopback_for_the_ipv4_wildcard() {
        assert_eq!(
            probe_addr("0.0.0.0:9101".parse().unwrap()),
            "127.0.0.1:9101".parse().unwrap()
        );
        assert_eq!(
            probe_addr(DEFAULT_LISTEN_ADDR),
            "127.0.0.1:9101".parse().unwrap()
        );
    }

    #[test]
    fn probe_uses_ipv6_loopback_for_the_ipv6_wildcard() {
        assert_eq!(
            probe_addr("[::]:9200".parse().unwrap()),
            "[::1]:9200".parse().unwrap()
        );
    }

    #[test]
    fn probe_keeps_concrete_addresses() {
        for addr in ["[::1]:9101", "192.168.1.20:9101", "127.0.0.1:8080"] {
            let addr: SocketAddr = addr.parse().unwrap();
            assert_eq!(probe_addr(addr), addr);
        }
    }
}
