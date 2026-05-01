---
title: Configuration
layout: default
parent: User Manual
nav_order: 5
---

# Configuration

Yggdrasil is configured by a JSON config file, optionally overridden by CLI flags. The config schema mirrors the upstream Haskell node's `config.json` keys for byte-for-byte compatibility — you can drop in an unmodified upstream operator config and Yggdrasil will parse it.

## Where the config comes from

Resolution order, highest priority last:

1. **Network preset defaults** — built into the binary for `mainnet` / `preprod` / `preview`.
2. **`--config <path>`** — override with a custom file.
3. **CLI flags** — override individual values (e.g. `--port`, `--database-path`, `--peer`).

If you supply both `--network mainnet` and `--config custom.json`, the custom config wins, but unspecified values inherit from the preset.

## Generate a starter config

```bash
$ yggdrasil-node default-config > my-config.json
```

This emits the mainnet defaults. Edit and pass with `--config my-config.json`.

## Top-level config keys

The keys below are the Yggdrasil-specific snake_case names. PascalCase aliases matching upstream are also accepted — the column "Upstream alias" gives the upstream key for each.

### Network identity

| Key                     | Type    | Default     | Upstream alias                | Description |
|-------------------------|---------|-------------|-------------------------------|-------------|
| `network_magic`         | u32     | 764824073   | `NetworkMagic`                | Integer identifying the network in NtN handshake. |
| `requires_network_magic`| string  | derived     | `RequiresNetworkMagic`        | `RequiresNoMagic` for mainnet, `RequiresMagic` for testnets. |
| `protocol_versions`     | array   | `[13, 14]`  | (none)                        | Acceptable NtN handshake versions. |

### Storage and chain

| Key                     | Type    | Default     | Upstream alias                | Description |
|-------------------------|---------|-------------|-------------------------------|-------------|
| `storage_dir`           | string  | `./db`      | (CLI: `--database-path`)      | Root directory for immutable + volatile chain + ledger snapshots. |
| `max_ledger_snapshots`  | usize   | 8           | (none)                        | How many ledger checkpoints to retain. |
| `checkpoint_interval_slots` | u64 | 2160        | (none)                        | Minimum slot delta between checkpoint flushes. |

### Peer set

| Key                       | Type        | Default | Upstream alias                | Description |
|---------------------------|-------------|---------|-------------------------------|-------------|
| `peer_addr`               | string      | none    | (none)                        | Optional explicit primary peer (`host:port`). |
| `bootstrap_peers`         | array       | preset  | `BootstrapPeers`              | Ordered list of `{addr, port}`. |
| `local_roots`             | array       | preset  | `LocalRoots`                  | Trusted relays the operator manages. Each carries `accessPoints`, `advertise`, `valency`, `hotValency`, `warmValency`, `diffusionMode`, `trustable`. |
| `public_roots`            | array       | preset  | `PublicRoots`                 | DNS-published root peer set. |
| `use_ledger_after_slot`   | i64         | -1 (off)| `UseLedgerAfterSlot`          | Activate ledger-peer discovery after the given slot. `-1` disables. |
| `peer_snapshot_file`      | string      | none    | `PeerSnapshotFile`            | Path to a JSON peer-snapshot for cold-start fallback. |

### Governor targets

| Key                                    | Type | Default | Upstream alias                          | Description |
|----------------------------------------|------|---------|-----------------------------------------|-------------|
| `governor_target_known`                | u32  | 100     | `TargetNumberOfKnownPeers`              | Total known peers. |
| `governor_target_established`          | u32  | 50      | `TargetNumberOfEstablishedPeers`        | Concurrent connections. |
| `governor_target_active`               | u32  | 20      | `TargetNumberOfActivePeers`             | Hot peers (sync + relay). |
| `governor_target_known_big_ledger`     | u32  | 100     | `TargetNumberOfKnownBigLedgerPeers`     | Big-ledger known set. |
| `governor_target_established_big_ledger`| u32 | 30      | `TargetNumberOfEstablishedBigLedgerPeers`| Big-ledger connections. |
| `governor_target_active_big_ledger`    | u32  | 5       | `TargetNumberOfActiveBigLedgerPeers`    | Big-ledger hot peers. |
| `governor_tick_interval_secs`          | f64  | 1.0     | (none)                                  | Governor tick cadence. |

### BlockFetch concurrency

| Key                                      | Type | Default | Upstream alias                   | Description |
|------------------------------------------|------|---------|----------------------------------|-------------|
| `max_concurrent_block_fetch_peers`       | u8   | 1       | (none)                           | Maximum hot peers fetching in parallel during sync. Default `1` keeps the proven single-peer pipeline. After running the §6.5 rehearsal, an operator can flip to `2` to mirror upstream `bfcMaxConcurrencyBulkSync = 2`. |

### Genesis files

| Key                     | Type    | Default        | Upstream alias                | Description |
|-------------------------|---------|----------------|-------------------------------|-------------|
| `byron_genesis_file`    | string  | preset         | `ByronGenesisFile`            | Path to Byron genesis JSON. |
| `byron_genesis_hash`    | string  | preset         | `ByronGenesisHash`            | Expected hash. (Currently parsed; canonical-CBOR verification is a future slice.) |
| `shelley_genesis_file`  | string  | preset         | `ShelleyGenesisFile`          | Path to Shelley genesis JSON. |
| `shelley_genesis_hash`  | string  | preset         | `ShelleyGenesisHash`          | Verified at startup. |
| `alonzo_genesis_file`   | string  | preset         | `AlonzoGenesisFile`           | Path to Alonzo genesis JSON. |
| `alonzo_genesis_hash`   | string  | preset         | `AlonzoGenesisHash`           | Verified at startup. |
| `conway_genesis_file`   | string  | preset         | `ConwayGenesisFile`           | Path to Conway genesis JSON. |
| `conway_genesis_hash`   | string  | preset         | `ConwayGenesisHash`           | Verified at startup. |

### Genesis-derived parameters

These are pulled from the Shelley genesis but exposed as config keys for explicit override (rare):

| Key                  | Type    | Default                   | Upstream alias    | Description |
|----------------------|---------|---------------------------|-------------------|-------------|
| `epoch_length`       | u64     | 432000 (mainnet)          | (genesis-derived) | Slots per epoch. |
| `security_param_k`   | u64     | 2160                      | (genesis-derived) | Stability parameter. |
| `active_slot_coeff`  | f64     | 0.05                      | (genesis-derived) | Block production probability per slot. |

### Tracing and metrics

| Key                          | Type    | Default | Upstream alias               | Description |
|------------------------------|---------|---------|------------------------------|-------------|
| `turn_on_logging`            | bool    | true    | `TurnOnLogging`              | Master logging switch. |
| `use_trace_dispatcher`       | bool    | true    | `UseTraceDispatcher`         | Enable namespace-aware dispatch. |
| `trace_options`              | object  | preset  | `TraceOptions`               | Per-namespace severity, frequency, backend, detail level. |
| `trace_option_node_name`     | string  | `relay-1`| `TraceOptionNodeName`       | Identifier in trace output. |
| `trace_option_forwarder`     | object  | none    | `TraceOptionForwarder`       | Cardano-tracer forwarder socket. |
| `metrics_port`               | u16     | none    | (none, CLI: `--metrics-port`)| If set, enables HTTP metrics on `127.0.0.1:<port>`. |

### Block production credentials

| Key                                    | Type    | Default | Upstream alias                            | Description |
|----------------------------------------|---------|---------|-------------------------------------------|-------------|
| `shelley_kes_key`                      | string  | none    | `ShelleyKesKey`                           | Path to KES `.skey`. |
| `shelley_vrf_key`                      | string  | none    | `ShelleyVrfKey`                           | Path to VRF `.skey`. |
| `shelley_operational_certificate`      | string  | none    | `ShelleyOperationalCertificate`           | Path to OpCert `.cert`. |
| `shelley_operational_certificate_issuer_vkey`| string| none | `ShelleyOperationalCertificateIssuerVkey` | Path to cold-key `.vkey`. |

If all four are supplied, the node activates block production. Otherwise it runs as a relay.

### NtC (local) socket

| Key                  | Type    | Default | Upstream alias    | Description |
|----------------------|---------|---------|-------------------|-------------|
| `local_socket_path`  | string  | none    | (CLI default: `<storage_dir>/node.sock`) | Unix socket path for `query` / `submit-tx` / wallet integration. |

### Inbound listener

| Key                  | Type    | Default | Upstream alias    | Description |
|----------------------|---------|---------|-------------------|-------------|
| `inbound_listen_addr`| string  | none    | (CLI: `--port` + `--host-addr`) | If set, the node accepts inbound NtN connections on this `host:port`. |

### Connection limits

| Key                                | Type | Default | Upstream alias                       | Description |
|------------------------------------|------|---------|--------------------------------------|-------------|
| `accepted_connections_limit_hard`  | u32  | 512     | `AcceptedConnectionsLimit.hardLimit` | Reject new inbound when total reaches this. |
| `accepted_connections_limit_soft`  | u32  | 384     | `AcceptedConnectionsLimit.softLimit` | Apply 5s delay between accepts when reached. |
| `accepted_connections_limit_delay_secs`| f64 | 5.0  | `AcceptedConnectionsLimit.delay`     | The 5s value above. |

### Protocol caps

| Key                          | Type | Default | Upstream alias                   | Description |
|------------------------------|------|---------|----------------------------------|-------------|
| `max_major_protocol_version` | u32  | 10      | `MaxKnownMajorProtocolVersion`   | Reject blocks whose protocol version exceeds this. Bump when upstream signals an imminent hard fork. |
| `peer_sharing`               | u8   | 1       | `PeerSharing`                    | NtN handshake `peer_sharing` willingness flag. `0` = disabled, `1` = enabled. |
| `consensus_mode`             | string | `PraosMode` | `ConsensusMode`              | `PraosMode` or `GenesisMode`. |

## CLI flag overrides

Every flag listed in the table below overrides the corresponding config key when present:

| Flag                              | Overrides                              |
|-----------------------------------|----------------------------------------|
| `--config <path>`                 | (loads the file)                       |
| `--network <preset>`              | (selects preset)                       |
| `--peer <host:port>`              | `peer_addr`                            |
| `--network-magic <u32>`           | `network_magic`                        |
| `--port <u16>`                    | `inbound_listen_addr` port             |
| `--host-addr <ip>`                | `inbound_listen_addr` host             |
| `--database-path <path>`          | `storage_dir`                          |
| `--topology <path>`               | `topology_file_path`                   |
| `--metrics-port <u16>`            | `metrics_port`                         |
| `--batch-size <usize>`            | `sync_batch_size`                      |
| `--checkpoint-interval-slots <u64>`| `checkpoint_interval_slots`           |
| `--max-ledger-snapshots <usize>`  | `max_ledger_snapshots`                 |
| `--non-producing-node`            | role override: ignore producer credentials |
| `--shelley-kes-key <path>`        | `shelley_kes_key`                      |
| `--shelley-vrf-key <path>`        | `shelley_vrf_key`                      |
| `--shelley-operational-certificate <path>`| `shelley_operational_certificate`|
| `--shelley-operational-certificate-issuer-vkey <path>`| `shelley_operational_certificate_issuer_vkey` |

Unspecified flags fall through to config file → preset default.
Producer credential fields are atomic: provide all four fields to enable forging, provide none for relay/sync-only mode, or pass `--non-producing-node` to force relay mode while leaving credential paths in the file.

## Topology file

A `topology.json` defines bootstrap peers, public roots, and local roots. The format mirrors upstream P2P topology:

```jsonc
{
  "bootstrapPeers": [
    { "address": "backbone.cardano.iog.io", "port": 3001 },
    { "address": "backbone.mainnet.cardanofoundation.org", "port": 3001 },
    { "address": "backbone.mainnet.emurgornd.com", "port": 3001 }
  ],
  "publicRoots": [
    {
      "accessPoints": [
        { "address": "relay.example.com", "port": 3001 }
      ],
      "advertise": true
    }
  ],
  "localRoots": [
    {
      "accessPoints": [
        { "address": "10.0.0.5", "port": 3001 },
        { "address": "10.0.0.6", "port": 3001 }
      ],
      "advertise": false,
      "trustable": true,
      "valency": 2,
      "hotValency": 2,
      "warmValency": 2,
      "diffusionMode": "InitiatorAndResponderDiffusionMode"
    }
  ],
  "useLedgerAfterSlot": 130000000
}
```

Per local-root group:

- `accessPoints` — addresses (resolves all of them; treats them as one logical group).
- `advertise` — share the group's addresses with peers via PeerSharing? `false` for private relays.
- `trustable` — eligible for sensitive-mode bootstrap fallback? `true` for relays you control.
- `valency` — total connections to maintain to this group.
- `hotValency` — of those, how many should be hot. Default = `valency`.
- `warmValency` — alias for `valency` retained for upstream compatibility.
- `diffusionMode` — `InitiatorAndResponderDiffusionMode` (full duplex) or `InitiatorOnlyDiffusionMode` (outbound only, e.g. for a block producer behind NAT).

## Where to go next

- [Running a Node]({{ "/manual/running/" | relative_url }}) — getting the node onto a service manager.
- [Block Production]({{ "/manual/block-production/" | relative_url }}) — the credential-loading half of the config.
- [CLI Reference]({{ "/manual/cli-reference/" | relative_url }}) — flags listed by subcommand.
