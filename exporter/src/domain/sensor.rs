use std::collections::HashSet;

/// Where a sensor lives. Outdoor sensors are excluded from whole-home averages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Indoor,
    Outdoor,
}

impl Placement {
    pub fn as_str(self) -> &'static str {
        match self {
            Placement::Indoor => "indoor",
            Placement::Outdoor => "outdoor",
        }
    }
}

/// One temperature/humidity sensor as last reported by the hub.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorReading {
    /// Stable identifier assigned by Tapo. Survives renames in the app.
    pub device_id: String,
    /// The nickname set in the Tapo app (e.g. "Living room").
    pub name: String,
    pub mac: String,
    pub model: String,
    pub placement: Placement,
    /// Whether the hub considers the sensor connected.
    pub online: bool,
    pub temperature_celsius: f64,
    pub humidity_percent: f64,
    pub battery_low: bool,
    pub rssi_dbm: i16,
    pub signal_level: u8,
}

/// Decides whether a sensor is indoor or outdoor.
///
/// A sensor is outdoor when its name or device id is in the configured list
/// (names are compared case-insensitively, ignoring surrounding whitespace).
#[derive(Debug, Clone, Default)]
pub struct PlacementPolicy {
    outdoor: HashSet<String>,
}

impl PlacementPolicy {
    pub fn new<I, S>(outdoor_sensors: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let outdoor = outdoor_sensors
            .into_iter()
            .map(|s| Self::normalize(s.as_ref()))
            .filter(|s| !s.is_empty())
            .collect();
        Self { outdoor }
    }

    pub fn classify(&self, name: &str, device_id: &str) -> Placement {
        if self.outdoor.contains(&Self::normalize(name))
            || self.outdoor.contains(&Self::normalize(device_id))
        {
            Placement::Outdoor
        } else {
            Placement::Indoor
        }
    }

    fn normalize(value: &str) -> String {
        value.trim().to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_name_case_insensitively() {
        let policy = PlacementPolicy::new([" Balcony ", ""]);
        assert_eq!(policy.classify("balcony", "abc"), Placement::Outdoor);
        assert_eq!(policy.classify("BALCONY ", "abc"), Placement::Outdoor);
        assert_eq!(policy.classify("Bedroom", "abc"), Placement::Indoor);
    }

    #[test]
    fn classifies_by_device_id() {
        let policy = PlacementPolicy::new(["802E1234"]);
        assert_eq!(policy.classify("Whatever", "802e1234"), Placement::Outdoor);
    }
}
