use std::future::Future;

use super::SensorReading;

/// Why a fetch from the sensor source failed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum GatewayError {
    #[error("could not connect or log in to the hub: {0}")]
    Connect(String),
    #[error("hub request failed: {0}")]
    Request(String),
}

/// Port through which the application obtains sensor readings.
///
/// The domain does not care whether they come from a Tapo hub, a file or a
/// test double; infrastructure provides the implementation.
pub trait SensorGateway: Send + Sync + 'static {
    /// Fetch the current reading of every temperature/humidity sensor.
    ///
    /// `placement` on the returned readings is a default; the application
    /// layer applies the configured placement policy.
    fn fetch_readings(
        &self,
    ) -> impl Future<Output = Result<Vec<SensorReading>, GatewayError>> + Send;
}
