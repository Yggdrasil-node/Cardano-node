//! Common protocol vocabulary for the pure-Rust `kes-agent` port.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Rust namespace module collecting the
//! mirrored upstream `Cardano.KESAgent.Protocols.*` leaf modules.

pub mod agent_info;
pub mod bearer_util;
pub mod control_v0_driver;
pub mod control_v0_peers;
pub mod control_v0_protocol;
pub mod control_v1_driver;
pub mod control_v1_peers;
pub mod control_v1_protocol;
pub mod control_v2_driver;
pub mod control_v2_peers;
pub mod control_v2_protocol;
pub mod control_v3_driver;
pub mod control_v3_peers;
pub mod control_v3_protocol;
pub mod recv_result;
pub mod service_v0_driver;
pub mod service_v0_peers;
pub mod service_v0_protocol;
pub mod service_v1_driver;
pub mod service_v1_peers;
pub mod service_v1_protocol;
pub mod service_v2_driver;
pub mod service_v2_peers;
pub mod service_v2_protocol;
pub mod types;
pub mod version_handshake_driver;
pub mod version_handshake_peers;
pub mod version_handshake_protocol;
