//! Typed configuration surface for the `dmq-node` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/dmq-node/dmq-node/src/DMQ/Configuration/CLIOptions.hs.
//!
//! Direct port of the operator-facing CLI surface. The full upstream
//! `Configuration'` record uses a generic `Identity`-or-`Last` functor
//! encoding (`PartialConfig` = `Configuration' Last`, fully-applied
//! `Configuration` = `Configuration' Identity`) so partial CLI-derived
//! configs can be merged with file-derived configs via `Semigroup`.
//! Yggdrasil's port keeps the same partial-vs-resolved split via two
//! distinct types ([`PartialConfig`] = all-fields `Option<_>`,
//! [`Configuration`] = all-fields concrete) plus a [`PartialConfig::merge`]
//! helper mirroring the upstream `Semigroup` instance.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **Generic-derived `Semigroup`/`Monoid` via `gmappend`/`gmempty`**:
//!   upstream uses GHC.Generics + `Generic.Data` to derive merging
//!   automatically. Yggdrasil's port writes the merge function
//!   explicitly — it's small (one line per field), no different from
//!   the kes-agent-control [`CommonOptions::merge`] pattern.
//! - **`Data.Act` action-on-types machinery**: upstream uses the
//!   `Data.Act.gpact` derivation to apply env-var-derived defaults
//!   atop CLI-derived overrides. Yggdrasil's port uses straight
//!   field-level merging; the action-on-types abstraction has no
//!   semantic role at this surface.
//! - **`mkDiffusionConfiguration` / `readConfigurationFile` / etc.**:
//!   upstream's higher-level wiring functions live in `Configuration.hs`;
//!   they touch `Ouroboros.Network.Diffusion` configuration which is
//!   a substantial separate port (tracked under `remaining_work`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Path to a Unix domain socket for node-to-client communication.
///
/// Upstream: `newtype LocalAddress = LocalAddress { getFilePath :: FilePath }`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocalAddress(pub PathBuf);

impl LocalAddress {
    /// Construct from any path-like value.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        LocalAddress(path.into())
    }

    /// Borrow the underlying path. Mirrors upstream `getFilePath`.
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for LocalAddress {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

/// Cardano network magic discriminator. Mirrors upstream
/// `Ouroboros.Network.Magic.NetworkMagic`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NetworkMagic(pub u32);

/// CLI-derived partial configuration. Every field is `Option<_>` so
/// the partial form can be merged with a file-derived partial form
/// via [`Self::merge`]; resolution to a fully-applied [`Configuration`]
/// happens via [`PartialConfig::resolve`] which fills in defaults.
///
/// Upstream: `PartialConfig = Configuration' Last`.
///
/// `Serialize` / `Deserialize` derives match upstream's
/// Generic-derived `FromJSON` instance for the `Configuration' Last`
/// shape: every field is optional and may be omitted from the JSON
/// document. Field names use camelCase to match upstream
/// (`hostAddr` / `hostIPv6Addr` / `portNumber` / `localSocket` /
/// `configurationFile` / `topologyFile` / `cardanoNodeSocket` /
/// `cardanoNetworkMagic` / `networkMagic` / `showVersion`).
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialConfig {
    /// `--host-addr IPv4` — IPv4 bind address.
    pub host_addr: Option<String>,
    /// `--host-ipv6-addr IPv6` — IPv6 bind address.
    pub host_ipv6_addr: Option<String>,
    /// `--port PORT_NUMBER` (`-p`) — bind port.
    pub port_number: Option<u16>,
    /// `--local-socket FILENAME` — Unix socket for node-to-client.
    pub local_address: Option<LocalAddress>,
    /// `--configuration-file FILENAME` (`-c`) — config file path.
    pub config_file: Option<PathBuf>,
    /// `--topology-file FILENAME` (`-t`) — topology file path.
    pub topology_file: Option<PathBuf>,
    /// `--cardano-node-socket FILENAME` — local cardano-node socket.
    pub cardano_node_socket: Option<PathBuf>,
    /// `--cardano-network-magic NATURAL` — cardano-node network magic.
    pub cardano_network_magic: Option<NetworkMagic>,
    /// `--dmq-network-magic NATURAL` — dmq-network's own magic.
    pub network_magic: Option<NetworkMagic>,
    /// `--version` (`-v`) — show version banner and exit.
    pub show_version: Option<bool>,
}

impl PartialConfig {
    /// Merge this partial-config with another; left wins on every
    /// field. Mirrors upstream's `Semigroup` instance derived via
    /// `Generic.Data.gmappend`.
    ///
    /// Used to thread CLI-flag-derived overrides on top of
    /// file-derived defaults: `cli.merge(file_config)`.
    pub fn merge(self, other: PartialConfig) -> PartialConfig {
        PartialConfig {
            host_addr: self.host_addr.or(other.host_addr),
            host_ipv6_addr: self.host_ipv6_addr.or(other.host_ipv6_addr),
            port_number: self.port_number.or(other.port_number),
            local_address: self.local_address.or(other.local_address),
            config_file: self.config_file.or(other.config_file),
            topology_file: self.topology_file.or(other.topology_file),
            cardano_node_socket: self.cardano_node_socket.or(other.cardano_node_socket),
            cardano_network_magic: self.cardano_network_magic.or(other.cardano_network_magic),
            network_magic: self.network_magic.or(other.network_magic),
            show_version: self.show_version.or(other.show_version),
        }
    }

    /// Resolve to a fully-applied [`Configuration`] using
    /// [`Configuration::defaults`] for any missing field.
    ///
    /// Mirrors the role of upstream's `Identity`-functor projection
    /// after `PartialConfig <> defaults` resolves to
    /// `Configuration' Identity`.
    pub fn resolve(self) -> Configuration {
        let defaults = Configuration::defaults();
        Configuration {
            host_addr: self.host_addr.unwrap_or(defaults.host_addr),
            host_ipv6_addr: self.host_ipv6_addr.unwrap_or(defaults.host_ipv6_addr),
            port_number: self.port_number.unwrap_or(defaults.port_number),
            local_address: self.local_address.unwrap_or(defaults.local_address),
            config_file: self.config_file.unwrap_or(defaults.config_file),
            topology_file: self.topology_file.unwrap_or(defaults.topology_file),
            cardano_node_socket: self
                .cardano_node_socket
                .unwrap_or(defaults.cardano_node_socket),
            cardano_network_magic: self
                .cardano_network_magic
                .unwrap_or(defaults.cardano_network_magic),
            network_magic: self.network_magic.unwrap_or(defaults.network_magic),
            show_version: self.show_version.unwrap_or(defaults.show_version),
        }
    }
}

/// Fully-applied configuration for `dmq-node`. Upstream:
/// `Configuration = Configuration' Identity`.
///
/// Defaults match upstream's `defaultConfiguration`:
/// - `host_addr = "0.0.0.0"`
/// - `host_ipv6_addr = "::"`
/// - `port_number = 3001`
/// - `local_address = "dmq-node.socket"`
/// - `config_file = "dmq-node.json"`
/// - `topology_file = "dmq-node-topology.json"`
/// - `cardano_node_socket = "node.socket"`
/// - `cardano_network_magic = 764824073` (mainnet)
/// - `network_magic = 0`
/// - `show_version = false`
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Configuration {
    /// IPv4 bind address.
    pub host_addr: String,
    /// IPv6 bind address.
    pub host_ipv6_addr: String,
    /// Bind port.
    pub port_number: u16,
    /// Unix socket for node-to-client.
    pub local_address: LocalAddress,
    /// Config file path.
    pub config_file: PathBuf,
    /// Topology file path.
    pub topology_file: PathBuf,
    /// Local cardano-node socket.
    pub cardano_node_socket: PathBuf,
    /// Cardano-node network magic.
    pub cardano_network_magic: NetworkMagic,
    /// DMQ network magic.
    pub network_magic: NetworkMagic,
    /// Whether `--version` was supplied (caller dispatches to
    /// version-banner emit + exit).
    pub show_version: bool,
}

impl Configuration {
    /// Default values matching upstream `defaultConfiguration`.
    pub fn defaults() -> Self {
        Configuration {
            host_addr: "0.0.0.0".to_string(),
            host_ipv6_addr: "::".to_string(),
            port_number: 3001,
            local_address: LocalAddress::new("dmq-node.socket"),
            config_file: PathBuf::from("dmq-node.json"),
            topology_file: PathBuf::from("dmq-node-topology.json"),
            cardano_node_socket: PathBuf::from("node.socket"),
            cardano_network_magic: NetworkMagic(764_824_073),
            network_magic: NetworkMagic(0),
            show_version: false,
        }
    }
}

impl Default for Configuration {
    fn default() -> Self {
        Configuration::defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_address_round_trip() {
        let addr = LocalAddress::new("/tmp/dmq.socket");
        assert_eq!(addr.as_path().to_str(), Some("/tmp/dmq.socket"));
    }

    #[test]
    fn network_magic_construction() {
        let m = NetworkMagic(764_824_073);
        assert_eq!(m.0, 764_824_073);
    }

    #[test]
    fn partial_config_default_all_none() {
        let p = PartialConfig::default();
        assert!(p.host_addr.is_none());
        assert!(p.port_number.is_none());
        assert!(p.config_file.is_none());
        assert!(p.show_version.is_none());
    }

    #[test]
    fn partial_config_merge_left_priority() {
        let cli = PartialConfig {
            host_addr: Some("127.0.0.1".to_string()),
            port_number: None,
            cardano_network_magic: Some(NetworkMagic(2)),
            ..PartialConfig::default()
        };
        let file = PartialConfig {
            host_addr: Some("0.0.0.0".to_string()),
            port_number: Some(3002),
            cardano_network_magic: Some(NetworkMagic(764_824_073)),
            ..PartialConfig::default()
        };
        let merged = cli.merge(file);
        assert_eq!(merged.host_addr.as_deref(), Some("127.0.0.1"));
        assert_eq!(merged.port_number, Some(3002));
        assert_eq!(merged.cardano_network_magic, Some(NetworkMagic(2)));
    }

    #[test]
    fn configuration_defaults_match_upstream() {
        let d = Configuration::defaults();
        assert_eq!(d.host_addr, "0.0.0.0");
        assert_eq!(d.host_ipv6_addr, "::");
        assert_eq!(d.port_number, 3001);
        assert_eq!(d.local_address.as_path().to_str(), Some("dmq-node.socket"));
        assert_eq!(d.config_file.to_str(), Some("dmq-node.json"));
        assert_eq!(d.topology_file.to_str(), Some("dmq-node-topology.json"));
        assert_eq!(d.cardano_node_socket.to_str(), Some("node.socket"));
        assert_eq!(d.cardano_network_magic, NetworkMagic(764_824_073));
        assert_eq!(d.network_magic, NetworkMagic(0));
        assert!(!d.show_version);
    }

    #[test]
    fn partial_config_resolve_uses_defaults_for_missing_fields() {
        let p = PartialConfig {
            host_addr: Some("10.0.0.1".to_string()),
            port_number: Some(4000),
            ..PartialConfig::default()
        };
        let c = p.resolve();
        assert_eq!(c.host_addr, "10.0.0.1");
        assert_eq!(c.port_number, 4000);
        // Defaulted fields:
        assert_eq!(c.host_ipv6_addr, "::");
        assert_eq!(c.config_file.to_str(), Some("dmq-node.json"));
    }

    #[test]
    fn partial_config_resolve_round_trips_all_supplied() {
        let p = PartialConfig {
            host_addr: Some("10.0.0.1".to_string()),
            host_ipv6_addr: Some("::1".to_string()),
            port_number: Some(4000),
            local_address: Some(LocalAddress::new("/tmp/dmq.socket")),
            config_file: Some(PathBuf::from("/etc/dmq.json")),
            topology_file: Some(PathBuf::from("/etc/dmq-topology.json")),
            cardano_node_socket: Some(PathBuf::from("/run/cardano.socket")),
            cardano_network_magic: Some(NetworkMagic(2)),
            network_magic: Some(NetworkMagic(7)),
            show_version: Some(true),
        };
        let c = p.resolve();
        assert_eq!(c.host_addr, "10.0.0.1");
        assert_eq!(c.host_ipv6_addr, "::1");
        assert_eq!(c.port_number, 4000);
        assert_eq!(c.local_address.as_path().to_str(), Some("/tmp/dmq.socket"));
        assert_eq!(c.config_file.to_str(), Some("/etc/dmq.json"));
        assert_eq!(c.topology_file.to_str(), Some("/etc/dmq-topology.json"));
        assert_eq!(c.cardano_node_socket.to_str(), Some("/run/cardano.socket"));
        assert_eq!(c.cardano_network_magic, NetworkMagic(2));
        assert_eq!(c.network_magic, NetworkMagic(7));
        assert!(c.show_version);
    }

    #[test]
    fn partial_config_resolve_empty_yields_defaults() {
        let p = PartialConfig::default();
        let c = p.resolve();
        assert_eq!(c, Configuration::defaults());
    }
}
