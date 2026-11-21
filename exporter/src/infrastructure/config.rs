use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    #[error("required environment variable {0} is missing or empty")]
    Missing(&'static str),
    #[error("environment variable {name} has invalid value {value:?}: {reason}")]
    Invalid {
        name: &'static str,
        value: String,
        reason: String,
    },
}

/// All runtime settings, read from environment variables.
#[derive(Clone, PartialEq)]
pub struct Config {
    /// Tapo account e-mail, used to authenticate against the hub on the LAN.
    pub tapo_username: String,
    pub tapo_password: String,
    pub hub_ip: IpAddr,
    pub tapo_request_timeout: Duration,
    pub poll_interval: Duration,
    pub poll_max_retries: usize,
    pub stale_after: Duration,
    pub expected_sensor_count: u32,
    pub outdoor_sensors: Vec<String>,
    pub listen_addr: SocketAddr,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |name: &str| {
            lookup(name)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let required = |name: &'static str| get(name).ok_or(ConfigError::Missing(name));

        let config = Self {
            tapo_username: required("TAPO_USERNAME")?,
            tapo_password: required("TAPO_PASSWORD")?,
            hub_ip: parse("TAPO_HUB_IP", &required("TAPO_HUB_IP")?)?,
            tapo_request_timeout: seconds(&get, "TAPO_REQUEST_TIMEOUT_SECONDS", 10)?,
            poll_interval: seconds(&get, "POLL_INTERVAL_SECONDS", 60)?,
            poll_max_retries: parse_or(&get, "POLL_MAX_RETRIES", 3)?,
            stale_after: seconds(&get, "STALE_AFTER_SECONDS", 300)?,
            expected_sensor_count: parse_or(&get, "EXPECTED_SENSOR_COUNT", 5)?,
            outdoor_sensors: get("OUTDOOR_SENSORS")
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            listen_addr: parse_or(&get, "LISTEN_ADDR", SocketAddr::from(([0, 0, 0, 0], 9101)))?,
        };

        if config.stale_after < config.poll_interval {
            return Err(ConfigError::Invalid {
                name: "STALE_AFTER_SECONDS",
                value: config.stale_after.as_secs().to_string(),
                reason: "must not be smaller than POLL_INTERVAL_SECONDS".into(),
            });
        }
        Ok(config)
    }
}

/// Never print the password.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("tapo_username", &self.tapo_username)
            .field("tapo_password", &"<redacted>")
            .field("hub_ip", &self.hub_ip)
            .field("tapo_request_timeout", &self.tapo_request_timeout)
            .field("poll_interval", &self.poll_interval)
            .field("poll_max_retries", &self.poll_max_retries)
            .field("stale_after", &self.stale_after)
            .field("expected_sensor_count", &self.expected_sensor_count)
            .field("outdoor_sensors", &self.outdoor_sensors)
            .field("listen_addr", &self.listen_addr)
            .finish()
    }
}

fn parse<T>(name: &'static str, value: &str) -> Result<T, ConfigError>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    value.parse().map_err(|e: T::Err| ConfigError::Invalid {
        name,
        value: value.to_string(),
        reason: e.to_string(),
    })
}

fn parse_or<T>(
    get: &impl Fn(&str) -> Option<String>,
    name: &'static str,
    default: T,
) -> Result<T, ConfigError>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    match get(name) {
        Some(value) => parse(name, &value),
        None => Ok(default),
    }
}

/// A strictly positive number of seconds.
fn seconds(
    get: &impl Fn(&str) -> Option<String>,
    name: &'static str,
    default: u64,
) -> Result<Duration, ConfigError> {
    let value: u64 = parse_or(get, name, default)?;
    if value == 0 {
        return Err(ConfigError::Invalid {
            name,
            value: "0".into(),
            reason: "must be greater than zero".into(),
        });
    }
    Ok(Duration::from_secs(value))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn load(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|name| map.get(name).cloned())
    }

    const REQUIRED: [(&str, &str); 3] = [
        ("TAPO_USERNAME", "me@example.com"),
        ("TAPO_PASSWORD", "secret"),
        ("TAPO_HUB_IP", "192.168.1.50"),
    ];

    #[test]
    fn applies_defaults() {
        let config = load(&REQUIRED).unwrap();
        assert_eq!(config.poll_interval, Duration::from_secs(60));
        assert_eq!(config.stale_after, Duration::from_secs(300));
        assert_eq!(config.expected_sensor_count, 5);
        assert!(config.outdoor_sensors.is_empty());
        assert_eq!(config.listen_addr.port(), 9101);
    }

    #[test]
    fn reads_overrides_and_outdoor_list() {
        let mut vars = REQUIRED.to_vec();
        vars.push(("POLL_INTERVAL_SECONDS", "30"));
        vars.push(("OUTDOOR_SENSORS", "Balcony, Garden ,"));
        let config = load(&vars).unwrap();
        assert_eq!(config.poll_interval, Duration::from_secs(30));
        assert_eq!(config.outdoor_sensors, vec!["Balcony", "Garden"]);
    }

    #[test]
    fn rejects_missing_and_invalid_values() {
        assert_eq!(
            load(&REQUIRED[..2]).unwrap_err(),
            ConfigError::Missing("TAPO_HUB_IP")
        );

        let mut vars = REQUIRED.to_vec();
        vars.push(("POLL_INTERVAL_SECONDS", "0"));
        assert!(matches!(
            load(&vars).unwrap_err(),
            ConfigError::Invalid {
                name: "POLL_INTERVAL_SECONDS",
                ..
            }
        ));
    }

    #[test]
    fn rejects_stale_window_shorter_than_poll_interval() {
        let mut vars = REQUIRED.to_vec();
        vars.push(("POLL_INTERVAL_SECONDS", "120"));
        vars.push(("STALE_AFTER_SECONDS", "60"));
        assert!(matches!(
            load(&vars).unwrap_err(),
            ConfigError::Invalid {
                name: "STALE_AFTER_SECONDS",
                ..
            }
        ));

        // Equal values are allowed.
        vars.pop();
        vars.push(("STALE_AFTER_SECONDS", "120"));
        assert!(load(&vars).is_ok());
    }

    #[test]
    fn rejects_an_invalid_hub_ip() {
        let vars = [
            ("TAPO_USERNAME", "me@example.com"),
            ("TAPO_PASSWORD", "secret"),
            ("TAPO_HUB_IP", "192.168.1.999"),
        ];
        assert!(matches!(
            load(&vars).unwrap_err(),
            ConfigError::Invalid {
                name: "TAPO_HUB_IP",
                ..
            }
        ));
    }

    #[test]
    fn debug_output_hides_the_password() {
        let printed = format!("{:?}", load(&REQUIRED).unwrap());
        assert!(!printed.contains("secret"));
    }
}
