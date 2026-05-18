# yggdrasil-node-genesis — per-era genesis loaders + protocol-parameter derivation

## Scope

Leaf-of-the-build-graph crate extracted from `yggdrasil-node` in
Wave 5 PR 7+8 so sister tools that need genesis-side helpers
(`cardano-cli`, `db-analyser`, `db-synthesizer`, `cardano-testnet`)
can depend on this surface without linking the runtime.

The crate ships:

- `ShelleyGenesis`, `AlonzoGenesis`, `ConwayGenesis`, `ByronGenesis*` —
  typed serde representations of the per-era genesis files.
- `GenesisLoadError` — single error enum surfaced by every loader.
- `build_protocol_parameters` — assembles a `ProtocolParameters`
  from the loaded values so the node can seed initial ledger state
  with network-accurate validation rules.
- `build_base_ledger_state` (+ `BaseLedgerStateInputs`) — the shared
  genesis→`LedgerState` builder (A3 R3c-1a): seeds the initial
  Byron-era multi-era `LedgerState` from pre-loaded genesis pieces, so
  the node (`startup.rs`) and the db-synthesizer build a byte-identical
  initial state.
- `slot_to_posix_ms`, `initial_funds_pseudo_txin`,
  `build_plutus_cost_model_from_protocol_values_for_protocol`, etc. —
  the operator helpers consumed by `yggdrasil-node`'s
  `local_server.rs`, `plutus_eval.rs`, and `commands/validate_config.rs`.

## Rules — Non-Negotiable

- **Leaf dependency.** Depends only on `yggdrasil-{crypto, ledger,
  plutus}`. Adding any other workspace crate as a dependency
  re-introduces the monolithic coupling that Wave 5 broke.
- **Tier 1 stability for `GenesisLoadError`.** The error enum's
  variant names are part of the parity-rehearsal evidence
  (operators key off them in log shippers). New variants OK in
  minor releases; renames are semver-major.
- **No internal-only types.** Anything `pub` here is part of the
  external API consumed by the `yggdrasil-node` binary + sister
  tools. Internal helpers stay `pub(crate)`.

## Naming parity

Synthesis crate. The lib.rs docstring carries the `## Naming parity`
stanza explaining that Yggdrasil unifies per-era Haskell genesis
loaders into one Rust module.

## R-arc tracking

Wave 5 PR 7+8 (extracted alongside yggdrasil-node-config because
`NodeConfigFile::verify_known_genesis_hashes` and several other
methods on `NodeConfigFile` return `GenesisLoadError`). A3 R3c-1a
(round 514) — added the shared `build_base_ledger_state` builder.
