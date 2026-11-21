use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::domain::SensorReading;

/// Shared, in-memory cache of the latest readings.
///
/// The poller writes to it; the HTTP layer reads from it, so a scrape never
/// triggers a request to the hub. Cloning is cheap (it clones an `Arc`).
#[derive(Clone)]
pub struct ClimateCache {
    state: Arc<RwLock<CacheState>>,
    stale_after: Duration,
}

#[derive(Default)]
struct CacheState {
    sensors: Vec<SensorReading>,
    last_success: Option<LastSuccess>,
    hub_up: bool,
    poll_failures_total: u64,
}

#[derive(Clone, Copy)]
struct LastSuccess {
    /// Monotonic clock, used to measure the age of the data.
    at: Instant,
    /// Wall clock, exposed to Prometheus.
    wall: SystemTime,
}

/// A consistent, point-in-time view of the cache.
#[derive(Debug, Clone, PartialEq)]
pub struct ClimateSnapshot {
    /// Empty when no poll has succeeded yet or the data is stale.
    pub sensors: Vec<SensorReading>,
    /// Whether the most recent poll succeeded.
    pub hub_up: bool,
    pub last_success_unix_seconds: Option<f64>,
    pub poll_failures_total: u64,
}

impl ClimateCache {
    /// `stale_after`: readings older than this are dropped from snapshots.
    pub fn new(stale_after: Duration) -> Self {
        Self {
            state: Arc::new(RwLock::new(CacheState::default())),
            stale_after,
        }
    }

    pub fn record_success(&self, sensors: Vec<SensorReading>) {
        let mut state = self.write();
        state.sensors = sensors;
        state.hub_up = true;
        state.last_success = Some(LastSuccess {
            at: Instant::now(),
            wall: SystemTime::now(),
        });
    }

    /// Marks the hub as down. Previous readings are kept until they go stale.
    pub fn record_failure(&self) {
        let mut state = self.write();
        state.hub_up = false;
        state.poll_failures_total += 1;
    }

    pub fn snapshot(&self) -> ClimateSnapshot {
        self.snapshot_at(Instant::now())
    }

    fn snapshot_at(&self, now: Instant) -> ClimateSnapshot {
        let state = self.state.read().unwrap_or_else(|e| e.into_inner());
        let fresh = state
            .last_success
            .is_some_and(|last| now.saturating_duration_since(last.at) <= self.stale_after);

        ClimateSnapshot {
            sensors: if fresh {
                state.sensors.clone()
            } else {
                Vec::new()
            },
            hub_up: state.hub_up,
            last_success_unix_seconds: state.last_success.and_then(|last| {
                last.wall
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs_f64())
            }),
            poll_failures_total: state.poll_failures_total,
        }
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, CacheState> {
        // A poisoned lock only means a writer panicked; the data is still usable.
        self.state.write().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Placement;

    fn reading(name: &str) -> SensorReading {
        SensorReading {
            device_id: format!("id-{name}"),
            name: name.to_string(),
            mac: "AA".into(),
            model: "T310".into(),
            placement: Placement::Indoor,
            online: true,
            temperature_celsius: 21.5,
            humidity_percent: 40.0,
            battery_low: false,
            rssi_dbm: -60,
            signal_level: 3,
        }
    }

    #[test]
    fn starts_empty_and_down() {
        let snapshot = ClimateCache::new(Duration::from_secs(300)).snapshot();
        assert!(snapshot.sensors.is_empty());
        assert!(!snapshot.hub_up);
        assert_eq!(snapshot.last_success_unix_seconds, None);
    }

    #[test]
    fn keeps_last_readings_through_a_failure_until_stale() {
        let cache = ClimateCache::new(Duration::from_secs(300));
        cache.record_success(vec![reading("Kitchen")]);
        cache.record_failure();

        let now = Instant::now();
        let fresh = cache.snapshot_at(now + Duration::from_secs(200));
        assert_eq!(fresh.sensors.len(), 1);
        assert!(!fresh.hub_up);
        assert_eq!(fresh.poll_failures_total, 1);

        let stale = cache.snapshot_at(now + Duration::from_secs(400));
        assert!(stale.sensors.is_empty());
        assert!(stale.last_success_unix_seconds.is_some());
    }
}
