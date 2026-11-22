//! Domain layer: pure business types and ports. No I/O, no framework code.

mod gateway;
mod sensor;

pub use gateway::{GatewayError, SensorGateway};
pub use sensor::{Placement, PlacementPolicy, SensorReading};
