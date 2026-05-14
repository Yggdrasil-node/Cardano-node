# yggdrasil-node-block-producer — block-producer primitives

## Scope

Block-producer credential loading, `ForgedBlock` types, and forge
primitives. Extracted from `yggdrasil-node` in Wave 5 PR 10 so:

1. The `default = ["forge"]` feature flag on `yggdrasil-node` can
   build a relay-only variant that compiles out the block-producer
   surface entirely (operators running pure relays don't ship the
   KES-secret-handling code).
2. Sister tools that load block-producer credentials (kes-agent,
   kes-agent-control) can depend on the types without linking the
   runtime.

The crate ships `BlockProducerCredentials`, `ForgedBlock`,
`ForgedBlockHeader`, `serialize_forged_block_cbor`,
`load_block_producer_credentials`, and the supporting per-credential
parse / KES-period / VRF-key helpers.

## Rules — Non-Negotiable

- **Security-sensitive.** Every public function that touches KES
  secret material MUST zeroize on drop (the `Zeroizing` newtypes from
  `yggdrasil-crypto`). Adding new public functions in this area
  requires the same posture.
- **Feature-flag conscious.** Wave 5 PR 13 wires `#[cfg(feature =
  "forge")]` gates around the actual block-creation code; today the
  flag is a declaration only.
- **Leaf in the build graph.** Depends only on `yggdrasil-{crypto,
  ledger, consensus}`. Adding any node-sub-crate dep breaks the
  layering.

## Naming parity

Synthesis crate. The lib.rs (former node/src/block_producer.rs)
carries the `## Naming parity` stanza.

## R-arc tracking

Wave 5 PR 10.
