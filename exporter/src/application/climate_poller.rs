use std::time::Duration;

use backon::{ExponentialBuilder, Retryable};
use tokio::time::MissedTickBehavior;
use tracing::{debug, error, info, warn};

use super::ClimateCache;
use crate::domain::{GatewayError, PlacementPolicy, SensorGateway, SensorReading};

/// How a single poll cycle retries a failed fetch (exponential backoff with jitter).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetryPolicy {
    /// Retries after the first attempt, within one poll cycle.
    pub max_retries: usize,
    pub min_delay: Duration,
    pub max_delay: Duration,
}

/// Background use case: periodically fetch readings and refresh the cache.
///
/// It never gives up. A cycle that exhausts its retries marks the hub as down
/// and the next cycle starts again from scratch.
pub struct ClimatePoller<G> {
    gateway: G,
    cache: ClimateCache,
    placement: PlacementPolicy,
    interval: Duration,
    retry: RetryPolicy,
}

impl<G: SensorGateway> ClimatePoller<G> {
    pub fn new(
        gateway: G,
        cache: ClimateCache,
        placement: PlacementPolicy,
        interval: Duration,
        retry: RetryPolicy,
    ) -> Self {
        Self {
            gateway,
            cache,
            placement,
            interval,
            retry,
        }
    }

    /// Runs forever. The first poll happens immediately.
    pub async fn run(self) {
        info!(interval_seconds = self.interval.as_secs(), "poller started");
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            self.poll_once().await;
        }
    }

    /// One poll cycle: fetch (with retries) and update the cache.
    pub async fn poll_once(&self) {
        match self.fetch_with_retry().await {
            Ok(mut sensors) => {
                for sensor in &mut sensors {
                    sensor.placement = self.placement.classify(&sensor.name, &sensor.device_id);
                }
                sensors.sort_by(|a, b| a.name.cmp(&b.name));
                debug!(sensors = sensors.len(), "poll succeeded");
                self.cache.record_success(sensors);
            }
            Err(err) => {
                error!(error = %err, "poll failed after retries; will try again next cycle");
                self.cache.record_failure();
            }
        }
    }

    async fn fetch_with_retry(&self) -> Result<Vec<SensorReading>, GatewayError> {
        let backoff = ExponentialBuilder::default()
            .with_min_delay(self.retry.min_delay)
            .with_max_delay(self.retry.max_delay)
            .with_max_times(self.retry.max_retries)
            .with_jitter();

        (|| self.gateway.fetch_readings())
            .retry(backoff)
            .notify(|err: &GatewayError, delay: Duration| {
                warn!(error = %err, retry_in_ms = delay.as_millis() as u64, "fetch failed, retrying");
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::domain::Placement;

    /// Fails a fixed number of times, then succeeds.
    struct FlakyGateway {
        failures_left: AtomicUsize,
        calls: Arc<AtomicUsize>,
    }

    impl SensorGateway for FlakyGateway {
        async fn fetch_readings(&self) -> Result<Vec<SensorReading>, GatewayError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.failures_left.load(Ordering::SeqCst) > 0 {
                self.failures_left.fetch_sub(1, Ordering::SeqCst);
                return Err(GatewayError::Request("boom".into()));
            }
            Ok(vec![reading("Kitchen"), reading("Balcony")])
        }
    }

    fn reading(name: &str) -> SensorReading {
        SensorReading {
            device_id: format!("id-{name}"),
            name: name.to_string(),
            mac: "AA".into(),
            model: "T310".into(),
            placement: Placement::Indoor,
            online: true,
            temperature_celsius: 20.0,
            humidity_percent: 50.0,
            battery_low: false,
            rssi_dbm: -60,
            signal_level: 3,
        }
    }

    fn poller(failures: usize) -> (ClimatePoller<FlakyGateway>, ClimateCache, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let cache = ClimateCache::new(Duration::from_secs(300));
        let poller = ClimatePoller::new(
            FlakyGateway {
                failures_left: AtomicUsize::new(failures),
                calls: calls.clone(),
            },
            cache.clone(),
            PlacementPolicy::new(["balcony"]),
            Duration::from_secs(60),
            RetryPolicy {
                max_retries: 2,
                min_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(2),
            },
        );
        (poller, cache, calls)
    }

    #[tokio::test]
    async fn retries_then_caches_classified_readings() {
        let (poller, cache, calls) = poller(2);
        poller.poll_once().await;

        let snapshot = cache.snapshot();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(snapshot.hub_up);
        assert_eq!(snapshot.sensors.len(), 2);
        assert_eq!(snapshot.sensors[0].name, "Balcony");
        assert_eq!(snapshot.sensors[0].placement, Placement::Outdoor);
        assert_eq!(snapshot.sensors[1].placement, Placement::Indoor);
    }

    #[tokio::test]
    async fn marks_hub_down_when_retries_are_exhausted_and_recovers_later() {
        let (poller, cache, calls) = poller(4);
        poller.poll_once().await;
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(!cache.snapshot().hub_up);
        assert_eq!(cache.snapshot().poll_failures_total, 1);

        poller.poll_once().await;
        assert!(cache.snapshot().hub_up);
    }

    #[tokio::test(start_paused = true)]
    async fn run_polls_immediately_and_then_once_per_interval() {
        let (poller, cache, calls) = poller(0);
        let task = tokio::spawn(poller.run());

        // With the clock paused, sleeping advances time deterministically.
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "first poll happens at startup"
        );
        assert!(cache.snapshot().hub_up);

        tokio::time::sleep(Duration::from_secs(59)).await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "no poll before the interval has passed"
        );

        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        tokio::time::sleep(Duration::from_secs(120)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 4);

        task.abort();
    }
}
