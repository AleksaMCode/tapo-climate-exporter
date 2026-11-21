use std::net::IpAddr;
use std::time::Duration;

use tapo::responses::{ChildDeviceHubResult, Status, T31XResult, TemperatureUnit};
use tapo::{ApiClient, HubHandler};
use tokio::sync::Mutex;
use tracing::info;

use crate::domain::{GatewayError, Placement, SensorGateway, SensorReading};

/// Reads T310/T315 sensors through a Tapo H100 hub on the local network.
///
/// Composes the `tapo` crate's hub handler. The login session is created
/// lazily and thrown away on any error, so the next call logs in again; this
/// covers expired sessions, hub reboots and the hub being down at startup.
pub struct TapoHubGateway {
    username: String,
    password: String,
    hub_ip: IpAddr,
    request_timeout: Duration,
    session: Mutex<Option<HubHandler>>,
}

impl TapoHubGateway {
    pub fn new(
        username: impl Into<String>,
        password: impl Into<String>,
        hub_ip: IpAddr,
        request_timeout: Duration,
    ) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            hub_ip,
            request_timeout,
            session: Mutex::new(None),
        }
    }

    async fn connect(&self) -> Result<HubHandler, GatewayError> {
        let hub = ApiClient::new(self.username.clone(), self.password.clone())
            .with_timeout(self.request_timeout)
            .h100(self.hub_ip.to_string())
            .await
            .map_err(|e| GatewayError::Connect(e.to_string()))?;
        info!(hub_ip = %self.hub_ip, "logged in to Tapo hub");
        Ok(hub)
    }

    fn to_reading(sensor: T31XResult) -> SensorReading {
        let temperature = f64::from(sensor.current_temperature);
        SensorReading {
            temperature_celsius: match sensor.temperature_unit {
                TemperatureUnit::Celsius => temperature,
                TemperatureUnit::Fahrenheit => (temperature - 32.0) * 5.0 / 9.0,
            },
            humidity_percent: f64::from(sensor.current_humidity),
            online: matches!(sensor.status, Status::Online),
            battery_low: sensor.at_low_battery,
            rssi_dbm: sensor.rssi,
            signal_level: sensor.signal_level,
            device_id: sensor.device_id,
            name: sensor.nickname,
            mac: sensor.mac,
            model: sensor.model,
            placement: Placement::Indoor,
        }
    }
}

impl SensorGateway for TapoHubGateway {
    async fn fetch_readings(&self) -> Result<Vec<SensorReading>, GatewayError> {
        // The lock also serialises requests, so the hub is never hit concurrently.
        let mut session = self.session.lock().await;

        let hub = match session.take() {
            Some(hub) => hub,
            None => self.connect().await?,
        };

        // One request returns every child device the hub knows about.
        let children = hub
            .get_child_device_list()
            .await
            .map_err(|e| GatewayError::Request(e.to_string()))?;

        // Only keep the session once it has proven to work.
        *session = Some(hub);

        Ok(children
            .into_iter()
            .filter_map(|child| match child {
                ChildDeviceHubResult::T31X(sensor) => Some(Self::to_reading(*sensor)),
                _ => None,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A T310 as the hub reports it, with the given unit, temperature and status.
    fn hub_sensor(unit: &str, temperature: f32, status: &str) -> T31XResult {
        serde_json::from_value(serde_json::json!({
            "at_low_battery": true,
            "avatar": "sensor_t310",
            "bind_count": 1,
            "category": "subg.trigger.temp-hmdt-sensor",
            "device_id": "802E000000000000000000000000000000000001",
            "fw_ver": "1.5.0",
            "hw_id": "HW",
            "hw_ver": "1.0",
            "jamming_rssi": -120,
            "jamming_signal_level": 1,
            "mac": "AABBCCDDEEFF",
            "model": "T310",
            "nickname": "Kitchen",
            "oem_id": "OEM",
            "parent_device_id": "HUB",
            "region": "Europe/Belgrade",
            "rssi": -57,
            "signal_level": 3,
            "specs": "EU",
            "status": status,
            "type": "SMART.TAPOSENSOR",
            "current_humidity_exception": 0,
            "current_humidity": 48,
            "current_temp_exception": 0.0,
            "current_temp": temperature,
            "lastOnboardingTimestamp": 1_700_000_000u64,
            "report_interval": 16,
            "status_follow_edge": false,
            "temp_unit": unit,
        }))
        .expect("valid T31X payload")
    }

    #[test]
    fn maps_every_field_of_a_celsius_sensor() {
        let reading = TapoHubGateway::to_reading(hub_sensor("celsius", 21.5, "online"));

        assert_eq!(
            reading.device_id,
            "802E000000000000000000000000000000000001"
        );
        assert_eq!(reading.name, "Kitchen");
        assert_eq!(reading.mac, "AABBCCDDEEFF");
        assert_eq!(reading.model, "T310");
        assert_eq!(reading.temperature_celsius, 21.5);
        assert_eq!(reading.humidity_percent, 48.0);
        assert!(reading.online);
        assert!(reading.battery_low);
        assert_eq!(reading.rssi_dbm, -57);
        assert_eq!(reading.signal_level, 3);
        // Placement is decided later by the application layer.
        assert_eq!(reading.placement, Placement::Indoor);
    }

    #[test]
    fn converts_fahrenheit_to_celsius() {
        let reading = TapoHubGateway::to_reading(hub_sensor("fahrenheit", 68.0, "online"));
        assert!((reading.temperature_celsius - 20.0).abs() < 1e-9);

        let freezing = TapoHubGateway::to_reading(hub_sensor("fahrenheit", 32.0, "online"));
        assert!(freezing.temperature_celsius.abs() < 1e-9);
    }

    #[test]
    fn maps_offline_status() {
        let reading = TapoHubGateway::to_reading(hub_sensor("celsius", 21.5, "offline"));
        assert!(!reading.online);
    }
}
