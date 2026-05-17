//! Effective-config assembly: load the file config, then layer
//! per-flag CLI overrides on top.
//!
//! Mirrors upstream `Cardano.Node.Configuration.POM` — the Haskell
//! "Partial Options Monoid" pattern that overlays the parsed
//! `NodeConfiguration` JSON onto a `<>`-monoidal stack of CLI
//! overrides. Yggdrasil's variant is split per override domain
//! (topology, inbound listen, block-producer credentials) instead of
//! one giant monoid.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Configuration/POM.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side `validate-config` subcommand handler. Performs operator preflight against `NodeConfigFile` (config + peer-snapshot inputs, recovery state, genesis-hash integrity, governor sanity, KES/Praos invariants). No upstream parallel — `cardano-node`'s equivalent is a runtime-startup check, not a separate subcommand.

use std::net::SocketAddr;
use std::path::PathBuf;

use eyre::{Result, WrapErr};

use yggdrasil_node_config::{
    NetworkPreset, NodeConfigFile, TraceNamespaceConfig, apply_topology_to_config, default_config,
    load_topology_file,
};

/// Namespace identifier used by the per-namespace trace config slot
/// for checkpoint-recovery events. Mirrors the namespace string the
/// runtime emits from `Node.Recovery.Checkpoint.*` traces.
pub(crate) const CHECKPOINT_TRACE_NAMESPACE: &str = "Node.Recovery.Checkpoint";

/// Load the effective `NodeConfigFile` from a file path or a network preset.
///
/// Returns the parsed config plus the path of its parent directory so
/// downstream override and path-resolution helpers can interpret
/// config-relative paths (genesis files, peer snapshot, KES key, etc.).
/// Falls through JSON → YAML to match the upstream Haskell parser.
pub fn load_effective_config(
    config: Option<PathBuf>,
    network: Option<NetworkPreset>,
) -> Result<(NodeConfigFile, Option<PathBuf>)> {
    match config {
        Some(path) => {
            let contents = std::fs::read_to_string(&path)
                .wrap_err_with(|| format!("failed to read config file {}", path.display()))?;
            let parsed: NodeConfigFile = match serde_json::from_str(&contents) {
                Ok(parsed) => parsed,
                Err(json_err) => serde_norway::from_str(&contents).map_err(|yaml_err| {
                    eyre::eyre!(
                        "failed to parse config file {} as JSON ({json_err}) or YAML ({yaml_err})",
                        path.display()
                    )
                })?,
            };
            Ok((parsed, path.parent().map(PathBuf::from)))
        }
        None => Ok(match network {
            Some(preset) => (preset.to_config(), Some(preset_config_base_dir(preset))),
            None => (default_config(), None),
        }),
    }
}

/// Resolve the on-disk directory shipping the vendored upstream-parity
/// configs for a network preset, e.g. `crates/node/yggdrasil-node/configuration/preview`.
pub fn preset_config_base_dir(preset: NetworkPreset) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("configuration")
        .join(preset.to_string())
}

/// Apply topology overrides from --topology CLI flag or TopologyFilePath config key.
///
/// If `cli_topology` is provided it takes priority.  Otherwise falls back to the
/// `TopologyFilePath` field in the config file.  The loaded topology replaces the
/// inline peer topology fields in the config.
pub fn apply_topology_override(
    file_cfg: &mut NodeConfigFile,
    cli_topology: Option<&std::path::Path>,
    config_base_dir: Option<&std::path::Path>,
) -> Result<()> {
    let topology_path = if let Some(path) = cli_topology {
        Some(path.to_path_buf())
    } else {
        file_cfg
            .topology_file_path
            .as_deref()
            .map(|s| crate::resolve_config_path(std::path::Path::new(s), config_base_dir))
    };

    if let Some(path) = topology_path {
        let topology = load_topology_file(&path)
            .wrap_err_with(|| format!("failed to load topology file {}", path.display()))?;
        apply_topology_to_config(file_cfg, &topology);

        // Also update the primary peer from the topology's first bootstrap
        // or root candidate when available.
        let candidates = topology.resolved_root_providers().ordered_candidates();
        if let Some(first) = candidates.first() {
            file_cfg.peer_addr = *first;
        }
    }

    Ok(())
}

/// Apply `--port` / `--host-addr` overrides to the inbound NtN listen
/// address.  When either flag is set the config's existing
/// `inbound_listen_addr` is replaced with a fresh `SocketAddr` derived
/// from the supplied parts (defaulting host to `0.0.0.0` and port to
/// upstream's `3001`).
pub fn apply_inbound_listen_overrides(
    file_cfg: &mut NodeConfigFile,
    port: Option<u16>,
    host_addr: Option<String>,
) -> Result<()> {
    if port.is_some() || host_addr.is_some() {
        let listen_ip: std::net::IpAddr = host_addr
            .as_deref()
            .unwrap_or("0.0.0.0")
            .parse()
            .wrap_err("invalid --host-addr")?;
        let listen_port = port.unwrap_or(3001);
        file_cfg.inbound_listen_addr = Some(SocketAddr::new(listen_ip, listen_port));
    }
    Ok(())
}

/// Overlay block-producer credential paths from the four
/// `--shelley-{kes,vrf,operational-certificate,operational-certificate-issuer-vkey}`
/// CLI flags onto the file config. Each override is independent — an
/// operator can supply a subset; the final composition is validated
/// downstream by `validate_config::ensure_block_producer_credential_policy`.
pub fn apply_block_producer_credential_overrides(
    file_cfg: &mut NodeConfigFile,
    shelley_kes_key: Option<&PathBuf>,
    shelley_vrf_key: Option<&PathBuf>,
    shelley_operational_certificate: Option<&PathBuf>,
    shelley_operational_certificate_issuer_vkey: Option<&PathBuf>,
) {
    if let Some(p) = shelley_kes_key {
        file_cfg.shelley_kes_key = Some(p.display().to_string());
    }
    if let Some(p) = shelley_vrf_key {
        file_cfg.shelley_vrf_key = Some(p.display().to_string());
    }
    if let Some(p) = shelley_operational_certificate {
        file_cfg.shelley_operational_certificate = Some(p.display().to_string());
    }
    if let Some(p) = shelley_operational_certificate_issuer_vkey {
        file_cfg.shelley_operational_certificate_issuer_vkey = Some(p.display().to_string());
    }
}

/// Get-or-insert the per-namespace trace config slot for the
/// `Node.Recovery.Checkpoint` namespace so the checkpoint-trace CLI
/// overrides (`--checkpoint-trace-{severity,backend,max-frequency}`)
/// can mutate it without rewriting the entire `trace_options` map.
pub fn checkpoint_trace_config_mut(file_cfg: &mut NodeConfigFile) -> &mut TraceNamespaceConfig {
    file_cfg
        .trace_options
        .entry(CHECKPOINT_TRACE_NAMESPACE.to_owned())
        .or_default()
}
