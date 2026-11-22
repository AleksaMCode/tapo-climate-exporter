<a href="https://www.flaticon.com/free-icon/temperature_4413515" target="_blank">
   <img width="150" align="right" src="./resources/icon.png"></img>
</a>

# Tapo Climate Exporter

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Go 1.100.0](https://img.shields.io/badge/rust-1.100.0-B7410E.svg)](https://releases.rs/docs/1.100.0/)
[![Code style: rustfmt](https://img.shields.io/badge/code%20style%20-rustfmt-e3773e.svg)](https://rust-lang.github.io/rustfmt)
![](https://img.shields.io/github/v/release/AleksaMCode/tapo-climate-exporter)

Collects temperature and humidity from Tapo [T310](https://www.tp-link.com/us/smart-home/smart-sensor/tapo-t310/)/[T315](https://www.tp-link.com/us/smart-home/smart-sensor/tapo-t315/) sensors through an [H100](https://www.tp-link.com/us/home-networking/smart-hub/tapo-h100/) hub and shows them in Grafana.

<p align="center">
<img
src="./resources/grafana_screenshot.png?raw=true"
alt="Tapo Climate Exporter Grafana dashboard"
width="100%"
class="center"
/>
<p align="center">
    <label><b>Fig. 1</b>: Tapo Climate Exporter <code>0.1.0</code> Grafana dashboard</label>
    </p>
</p>

## Run it

```sh
cp .env.example .env          # fill in TAPO_*, GRAFANA_ADMIN_PASSWORD, OUTDOOR_SENSORS
docker compose up -d --build
```

| What       | Where                                   | Reachable from      |
|------------|-----------------------------------------|---------------------|
| Grafana    | `http://<server-ip>:3000`               | local network       |
| Prometheus | `http://localhost:9090`                 | the server only     |
| Exporter   | `http://localhost:9101/metrics`, `/health` | the server only  |

Grafana opens on the provisioned **Tapo Climate** dashboard. Log in with `GRAFANA_ADMIN_USER` / `GRAFANA_ADMIN_PASSWORD`.

> [!NOTE]
>  The `.env` file is also copied into the exporter image.

## Configuration

Everything is set in `.env`; see [.env.example](.env.example) for all variables and their defaults. The ones you will most likely touch:

| Variable                | Default | Meaning |
|-------------------------|---------|---------|
| `TAPO_USERNAME`, `TAPO_PASSWORD`, `TAPO_HUB_IP` | required | Hub login and its fixed IP |
| `POLL_INTERVAL_SECONDS` | `60`    | How often the hub is polled |
| `STALE_AFTER_SECONDS`   | `300`   | Readings older than this are no longer exported |
| `EXPECTED_SENSOR_COUNT` | `5`     | Number of sensors you own |
| `OUTDOOR_SENSORS`       | empty   | Names (or device ids) excluded from the home averages |
| `PROMETHEUS_RETENTION`  | `1y`    | Rolling history window |

The Prometheus scrape interval is set in [monitoring/prometheus/prometheus.yml](monitoring/prometheus/prometheus.yml) as it cannot read it from the environment.

## Metrics

| Metric | Labels | Meaning |
|--------|--------|---------|
| `tapo_sensor_temperature_celsius` | `device_id`, `name`, `placement` | Temperature (online sensors only) |
| `tapo_sensor_humidity_percent`    | same | Relative humidity (online sensors only) |
| `tapo_sensor_online`              | same | 1 if the hub reports the sensor as connected |
| `tapo_sensor_battery_low`         | same | 1 if the battery is low |
| `tapo_sensor_rssi_dbm`, `tapo_sensor_signal_level` | same | Signal between sensor and hub |
| `tapo_sensor_info`                | + `mac`, `model` | Static details, value is always 1 |
| `tapo_sensors_expected`           | | `EXPECTED_SENSOR_COUNT` |
| `tapo_sensors_reported`           | | Sensors in the current data |
| `tapo_hub_up`                     | | 1 if the last poll succeeded |
| `tapo_last_successful_poll_timestamp_seconds` | | When the readings were taken |
| `tapo_poll_failures_total`        | | Poll cycles that failed after all retries |

> [!TIP]
> - `name` is the nickname from the Tapo app; 
> - `device_id` is Tapo's stable id and does not change when you rename a sensor
> - `placement` is `indoor` or `outdoor`.

> [!NOTE]
> Whole-home average is calculated as: `avg(tapo_sensor_temperature_celsius{placement="indoor"})`.
