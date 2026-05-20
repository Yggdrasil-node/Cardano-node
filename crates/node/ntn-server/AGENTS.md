# yggdrasil-node-ntn-server — Node-to-Node mini-protocol server

## Scope

Wave 5 PR 11 — NtN side of the diffusion layer. Hosts the
inbound-accept loop and the per-connection mini-protocol servers
for ChainSync, BlockFetch, KeepAlive, TxSubmission2, and
PeerSharing. Mirrors upstream `Ouroboros.Network.Server`.

## Rules — Non-Negotiable

- **No reverse dep on runtime/server orchestration outside this crate.**
  Consumers (yggdrasil-node binary + runtime) call into this crate;
  this crate MUST NOT reach back into runtime internals.
- **Storage / consensus boundaries.** Block / chain / mempool access
  is through trait abstractions defined here
  (`BlockProvider`, `ChainProvider`, `SharedChainDb`,
  `SharedTxSubmissionConsumer`) so test doubles can be injected.

## Naming parity

Synthesis crate. The lib.rs carries the `## Naming parity` stanza.

## R-arc tracking

Wave 5 PR 11.
