//! Infrastructure layer: adapters to the outside world (Tapo hub, environment).

mod config;
mod tapo_hub_gateway;

pub use config::Config;
pub use tapo_hub_gateway::TapoHubGateway;
