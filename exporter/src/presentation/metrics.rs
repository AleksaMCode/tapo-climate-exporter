use std::fmt::Write;

use crate::application::ClimateSnapshot;
use crate::domain::SensorReading;

/// Renders a snapshot in the Prometheus text exposition format.
///
/// Rendering by hand (instead of a registry of gauges) guarantees that a
/// sensor which disappears or goes stale also disappears from the output.
pub fn render(snapshot: &ClimateSnapshot, expected_sensor_count: u32) -> String {
    let mut out = MetricsWriter::default();
    let sensors = &snapshot.sensors;

    out.family(
        "tapo_hub_up",
        "gauge",
        "1 if the last poll of the hub succeeded, 0 otherwise.",
    );
    out.sample("tapo_hub_up", &[], bool_value(snapshot.hub_up));

    out.family(
        "tapo_last_successful_poll_timestamp_seconds",
        "gauge",
        "Unix time of the last successful poll; the readings are from this moment.",
    );
    if let Some(timestamp) = snapshot.last_success_unix_seconds {
        out.sample(
            "tapo_last_successful_poll_timestamp_seconds",
            &[],
            timestamp,
        );
    }

    out.family(
        "tapo_poll_failures_total",
        "counter",
        "Poll cycles that failed after all retries.",
    );
    out.sample(
        "tapo_poll_failures_total",
        &[],
        snapshot.poll_failures_total as f64,
    );

    out.family(
        "tapo_sensors_expected",
        "gauge",
        "Number of sensors configured as expected.",
    );
    out.sample(
        "tapo_sensors_expected",
        &[],
        f64::from(expected_sensor_count),
    );

    out.family(
        "tapo_sensors_reported",
        "gauge",
        "Number of sensors in the current (non-stale) data.",
    );
    out.sample("tapo_sensors_reported", &[], sensors.len() as f64);

    out.family(
        "tapo_sensor_online",
        "gauge",
        "1 if the hub reports the sensor as online, 0 otherwise.",
    );
    for s in sensors {
        out.sample("tapo_sensor_online", &identity(s), bool_value(s.online));
    }

    // Climate values are only exported for online sensors: an offline sensor's
    // last value would otherwise look like a live reading.
    out.family(
        "tapo_sensor_temperature_celsius",
        "gauge",
        "Current temperature in degrees Celsius.",
    );
    for s in sensors.iter().filter(|s| s.online) {
        out.sample(
            "tapo_sensor_temperature_celsius",
            &identity(s),
            s.temperature_celsius,
        );
    }

    out.family(
        "tapo_sensor_humidity_percent",
        "gauge",
        "Current relative humidity in percent.",
    );
    for s in sensors.iter().filter(|s| s.online) {
        out.sample(
            "tapo_sensor_humidity_percent",
            &identity(s),
            s.humidity_percent,
        );
    }

    out.family(
        "tapo_sensor_battery_low",
        "gauge",
        "1 if the sensor reports a low battery, 0 otherwise.",
    );
    for s in sensors {
        out.sample(
            "tapo_sensor_battery_low",
            &identity(s),
            bool_value(s.battery_low),
        );
    }

    out.family(
        "tapo_sensor_rssi_dbm",
        "gauge",
        "Signal strength between sensor and hub in dBm.",
    );
    for s in sensors {
        out.sample("tapo_sensor_rssi_dbm", &identity(s), f64::from(s.rssi_dbm));
    }

    out.family(
        "tapo_sensor_signal_level",
        "gauge",
        "Signal level between sensor and hub (0-3).",
    );
    for s in sensors {
        out.sample(
            "tapo_sensor_signal_level",
            &identity(s),
            f64::from(s.signal_level),
        );
    }

    out.family(
        "tapo_sensor_info",
        "gauge",
        "Static sensor details; the value is always 1.",
    );
    for s in sensors {
        let mut labels = identity(s).to_vec();
        labels.push(("mac", s.mac.as_str()));
        labels.push(("model", s.model.as_str()));
        out.sample("tapo_sensor_info", &labels, 1.0);
    }

    out.finish()
}

/// Labels identifying a sensor on every per-sensor metric.
fn identity(sensor: &SensorReading) -> [(&'static str, &str); 3] {
    [
        ("device_id", sensor.device_id.as_str()),
        ("name", sensor.name.as_str()),
        ("placement", sensor.placement.as_str()),
    ]
}

fn bool_value(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

#[derive(Default)]
struct MetricsWriter {
    out: String,
}

impl MetricsWriter {
    fn family(&mut self, name: &str, kind: &str, help: &str) {
        let _ = writeln!(self.out, "# HELP {name} {help}");
        let _ = writeln!(self.out, "# TYPE {name} {kind}");
    }

    fn sample(&mut self, name: &str, labels: &[(&str, &str)], value: f64) {
        self.out.push_str(name);
        if !labels.is_empty() {
            self.out.push('{');
            for (i, (key, val)) in labels.iter().enumerate() {
                if i > 0 {
                    self.out.push(',');
                }
                let _ = write!(self.out, "{key}=\"{}\"", escape_label(val));
            }
            self.out.push('}');
        }
        let _ = writeln!(self.out, " {value}");
    }

    fn finish(self) -> String {
        self.out
    }
}

fn escape_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Placement;

    fn reading(name: &str, placement: Placement, online: bool) -> SensorReading {
        SensorReading {
            device_id: format!("id-{name}"),
            name: name.to_string(),
            mac: "AABBCC".into(),
            model: "T310".into(),
            placement,
            online,
            temperature_celsius: 21.5,
            humidity_percent: 43.0,
            battery_low: false,
            rssi_dbm: -61,
            signal_level: 3,
        }
    }

    #[test]
    fn renders_hub_and_sensor_metrics() {
        let snapshot = ClimateSnapshot {
            sensors: vec![
                reading("Kitchen", Placement::Indoor, true),
                reading("Balcony", Placement::Outdoor, false),
            ],
            hub_up: true,
            last_success_unix_seconds: Some(1_700_000_000.5),
            poll_failures_total: 2,
        };
        let text = render(&snapshot, 5);

        assert!(text.contains("tapo_hub_up 1\n"));
        assert!(text.contains("tapo_last_successful_poll_timestamp_seconds 1700000000.5\n"));
        assert!(text.contains("tapo_poll_failures_total 2\n"));
        assert!(text.contains("tapo_sensors_expected 5\n"));
        assert!(text.contains("tapo_sensors_reported 2\n"));
        assert!(text.contains(
            "tapo_sensor_temperature_celsius{device_id=\"id-Kitchen\",name=\"Kitchen\",placement=\"indoor\"} 21.5\n"
        ));
        assert!(text.contains(
            "tapo_sensor_online{device_id=\"id-Balcony\",name=\"Balcony\",placement=\"outdoor\"} 0\n"
        ));
        // Offline sensors expose no climate values.
        assert!(!text.contains("tapo_sensor_temperature_celsius{device_id=\"id-Balcony\""));
        assert!(!text.contains("tapo_sensor_humidity_percent{device_id=\"id-Balcony\""));
        assert!(text.contains("mac=\"AABBCC\",model=\"T310\"} 1\n"));
    }

    #[test]
    fn renders_without_sensors_when_hub_never_answered() {
        let snapshot = ClimateSnapshot {
            sensors: vec![],
            hub_up: false,
            last_success_unix_seconds: None,
            poll_failures_total: 1,
        };
        let text = render(&snapshot, 5);

        assert!(text.contains("tapo_hub_up 0\n"));
        assert!(text.contains("tapo_sensors_reported 0\n"));
        // Only the HELP/TYPE comments mention the metric; there is no sample line.
        assert!(!text.contains("\ntapo_last_successful_poll_timestamp_seconds "));
        assert!(!text.contains("tapo_sensor_online{"));
    }

    #[test]
    fn escapes_label_values() {
        assert_eq!(escape_label("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
    }
}
