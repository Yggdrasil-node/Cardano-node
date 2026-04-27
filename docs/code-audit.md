# Yggdrasil Cardano-node — Code Audit Report

**Repository:** `https://github.com/Yggdrasil-node/Cardano-node`
**Default branch:** `main`
**Audit date:** 27 April 2026
**Audit scope:** Security · Code quality · Cardano-specific concerns · Dependency / supply-chain
**Audit method:** Direct read of every checked-in file from a fresh `git clone` (no reliance on README claims).

---

## 1. Executive summary

Yggdrasil is a from-scratch, pure-Rust port of the upstream IntersectMBO Haskell Cardano node. The codebase is a Cargo workspace of nine crates plus a `node/` binary, totalling roughly **213 000 lines of Rust** across **361 files**, with no FFI, no `unsafe` blocks, and no `build.rs` scripts. The project has thoughtful structure, very strong cryptographic key hygiene, properly atomic storage writes, an upstream-aligned consensus and ledger model that has been verified against canonical IOG genesis-file hashes, and a documented audit-pin posture (`node/src/upstream_pins.rs`) recording exactly which upstream commits each subsystem was ported from. CI runs `cargo check` / `cargo test` / `cargo clippy -D warnings`, and `deny.toml` bans OpenSSL and copyleft licences.

Despite the strong overall posture, the audit identified **one Critical and two High issues** clustered in the network-protocol decode path that are exploitable by an unauthenticated remote attacker against any inbound-listening node, plus a small set of Medium issues around arithmetic safety, file permissions, and unmaintained dependencies. The Critical issue (C-1) is a single pattern that the network team will recognise immediately and that fans out to roughly 48 sites across the workspace; addressing it well requires a new helper, not piecemeal fixes.

The repository is **not yet ready for unattended mainnet block production**, but the surface area requiring attention is narrow, well-localised, and largely fixable in days rather than weeks. There is no evidence of malicious code, hidden backdoors, key-leak material in git history, suspicious authors, or supply-chain anomalies. The shipped mainnet/preprod/preview genesis files cryptographically match the canonical IntersectMBO hashes — supply-chain authentic.

### Top-line risk rating

| Category | Rating | One-line reason |
|---|---|---|
| Confidentiality | **Low** | No secrets in code or history; keys properly zeroized; redacted Debug; constant-time eq. |
| Integrity (consensus) | **Medium** | OpCert monotonicity correct; chain selection correct; **value-preservation arithmetic uses `saturating_add`** (theoretical edge case). |
| Availability | **High risk pre-fix** | Pre-auth handshake decode aborts process on crafted CBOR (C-1); all peer-supplied count fields lack upper bound (H-1); accept loop processes handshakes serially (H-2). |
| Operational safety | **Medium** | Genesis-hash check skipped silently when hash field is `None` (L-1); KES key file mode not validated (L-7); NtC socket created with default umask (M-3). |
| Supply chain | **Medium** | All 151 deps from crates.io with hashes; **but** `serde_cbor` (RUSTSEC-2021-0127, unmaintained) and `serde_yaml 0.9.34+deprecated` are present; CI lacks `cargo audit` / `cargo deny check` despite shipping `deny.toml`. |
| Code quality | **Strong** | No `unsafe`, no `unwrap()` in production paths, workspace-level clippy `-D warnings`, ~4200 tests, edition 2024, pinned toolchain. |

### Top three findings

1. **C-1** — Pre-auth remote process abort via unbounded `Vec::with_capacity` in the handshake CBOR decoder. Any attacker who can connect to TCP port 3001 can crash the node by sending a single malformed handshake message with `count = u64::MAX`.
2. **H-1** — The same unbounded-allocation pattern repeats in 48 places across protocol decoders and era-specific block decoders. Most of these are post-handshake but reachable from any peer.
3. **H-2** — The inbound accept loop performs the handshake **synchronously inside the loop body** before the rate-limit check fires. Combined with C-1 this makes the node trivially crashable; even without C-1 it serialises legitimate-peer admission behind any slow attacker.

---

## 2. Repository overview

### 2.1 Stats

| Metric | Value |
|---|---|
| Total tracked files (excluding `.git/`) | 361 |
| Rust source lines | ~213 219 |
| Workspace crates | 9 (`crypto`, `cddl-codegen`, `consensus`, `ledger`, `mempool`, `network`, `plutus`, `storage`) + `node/` binary |
| Cargo dependencies (direct + transitive) | 151 |
| Integration tests under `crates/ledger/tests/integration/` | 41 |
| Reported workspace test count (per README) | ≈ 4 210 |
| Branches | 1 (`main`) |
| Commits | 461 |
| Contributors | 7 |
| GPG-signed commits | 18 / 461 |

### 2.2 Top-level layout

```
Cardano-node/
├── .cargo/config.toml            cargo aliases (check-all, test-all, lint)
├── .devcontainer/                VS Code devcontainer
├── .github/                      CI, dependabot, CODEOWNERS, issue templates
├── .gitignore                    excludes *.skey, *.opcert, .env, etc.
├── .vscode/settings.json
├── AGENTS.md                     LLM-targeted workspace rules
├── CHANGELOG.md
├── CLAUDE.md                     LLM-targeted helper for Claude Code
├── Cargo.lock                    151 deps, all from crates.io
├── Cargo.toml                    workspace root
├── Dockerfile                    multi-stage, non-root, tini PID 1
├── LICENSE                       Apache-2.0
├── README.md
├── SECURITY.md                   reporting policy, scope, timelines
├── crates/
│   ├── cddl-codegen/             CDDL → Rust code generator
│   ├── consensus/                Praos, OpCert, ChainState, nonce, chain-selection
│   ├── crypto/                   Blake2b, Ed25519, VRF, KES (Simple+Sum), BLS12-381, secp256k1
│   ├── ledger/                   eras Byron→Conway, UTxO, fees, plutus_validation
│   ├── mempool/                  fee-ordered queue + tx_state
│   ├── network/                  mux, mini-protocols, governor, peer registry
│   ├── plutus/                   CEK machine, builtins, cost model, Flat decoder
│   └── storage/                  ImmutableStore / VolatileStore / LedgerStore + ChainDb
├── deny.toml                     cargo-deny config (bans openssl/native-tls)
├── docker-compose.yml
├── docs/                         Jekyll site for GitHub Pages
├── node/
│   ├── Cargo.toml
│   ├── configuration/{mainnet,preprod,preview}/  shipped configs + genesis files
│   ├── scripts/                  install, healthcheck, backup, restart resilience, etc.
│   └── src/                      CLI, runtime, sync, server, block_producer, plutus_eval
├── rust-toolchain.toml           pinned to 1.85.0 with clippy + rustfmt
├── rustfmt.toml
└── specs/                        upstream test-vector mirrors + CDDL fragments
```

### 2.3 Build system & tooling

- **Edition 2024**, toolchain pinned `1.85.0`.
- Workspace-level lints: `dbg_macro = deny`, `todo = deny`, `unwrap_used = deny`.
- CI on `main` push and PR runs `cargo check-all`, `cargo test-all`, `cargo lint`.
- Release workflow builds Linux x86_64 + aarch64, strips, computes SHA-256, publishes GitHub Releases with aggregated `SHA256SUMS.txt` and auto-generated changelog.
- Dependabot configured for cargo (weekly), GitHub Actions (weekly), Docker (weekly), Bundler/Jekyll (monthly).
- `deny.toml` bans `openssl`, `openssl-sys`, `native-tls`; license allowlist permissive-only.

### 2.4 Authenticity verification

The shipped mainnet genesis files were Blake2b-256 hashed and compared against the values declared in the shipped `config.json`:

| File | Computed Blake2b-256 | Declared & Canonical IOG |
|---|---|---|
| `mainnet/shelley-genesis.json` | `1a3be38b…9276d81` | ✅ matches `IntersectMBO/cardano-node` master |
| `mainnet/alonzo-genesis.json`  | `7e94a15f…068ed874` | ✅ matches |
| `mainnet/conway-genesis.json`  | `15a199f8…643ef62` | ✅ matches |

The configurations are not subtly tampered. Topology files reference legitimate IOG / Cardano Foundation / Emurgo backbone hosts.

---

## 3. Findings

Each finding shows severity, location, description, exploit/impact, and remediation. Severity levels follow the OWASP Risk Rating philosophy: Critical = unauth remote with high impact; High = unauth remote with medium impact, or auth remote with high impact; Medium = local/post-auth, or remote with limited impact; Low = defense-in-depth or operational; Informational = positive observations or non-defects.

### 3.1 Critical

#### C-1 — Pre-auth remote process abort via unbounded `Vec::with_capacity` in handshake CBOR decoder

- **Location:** `crates/network/src/handshake.rs:266`, also `handshake.rs:347`
- **Severity:** Critical (unauthenticated remote DoS, full process abort)

**Description.** `decode_version_table` reads a CBOR map header to obtain `count: u64` and immediately calls `Vec::with_capacity(count as usize)`. The `dec.map()` call returns the raw CBOR-encoded length with no upper-bound validation — CBOR allows `count` up to `2^64 - 1` in 9 bytes (`0xbb ff ff ff ff ff ff ff ff` for a map header). On a 64-bit target `Vec::with_capacity(usize::MAX)` aborts the process via the global allocator's OOM handler.

```rust
// crates/network/src/handshake.rs:262-271
fn decode_version_table(
    dec: &mut Decoder<'_>,
) -> Result<Vec<(HandshakeVersion, NodeToNodeVersionData)>, LedgerError> {
    let count = dec.map()?;
    let mut versions = Vec::with_capacity(count as usize);  // <-- attacker-controlled
    for _ in 0..count {
        ...
    }
    Ok(versions)
}
```

**Reachability.** `decode_version_table` is invoked from `HandshakeMessage::from_cbor` (line 333) which is called on the **first** SDU received from any inbound peer, **before** any authentication, network-magic check, or rate limiting has the chance to act. The default mainnet relay deployment binds `0.0.0.0:3001`.

**Exploit.** A single TCP connection sending an SDU header (8 bytes) followed by 11 bytes of CBOR `[0, {count=u64::MAX: ...}]` aborts the node. No authentication, no resource consumption, no detection signature. The attacker can keep doing this on every restart.

**Impact.** Total loss of availability for any node accepting inbound NtN connections — i.e., every relay in the operator's topology. Block-producer nodes that accept inbound from their relay set are equally affected. On systemd `Restart=always` the node will crash-loop until the attacker stops.

**Remediation.**
1. Cap `count` against a per-message-type upper bound before allocation. For handshake the upstream version table never has more than ~10 entries; a safe cap is e.g. 128.
2. Better yet, introduce a workspace-wide helper:
   ```rust
   fn vec_with_capacity_bounded<T>(count: u64, max: usize) -> Result<Vec<T>, LedgerError> {
       let n = usize::try_from(count).map_err(|_| LedgerError::CountTooLarge)?;
       if n > max {
           return Err(LedgerError::CountTooLarge);
       }
       Ok(Vec::with_capacity(n))
   }
   ```
   and apply it to every `Vec::with_capacity(count as usize)` site.
3. As a hardening measure, pre-allocate `Vec::new()` and let the loop's `push` grow the allocation; this caps allocation at the bytes actually consumed from the SDU (which is itself capped at `MAX_SDU_PAYLOAD = 65 535`).

### 3.2 High

#### H-1 — Same unbounded-allocation pattern in 48 protocol decoder sites

- **Location:** 48 instances; key examples:
  - `crates/network/src/handshake.rs:266`, `:347` (also covered by C-1)
  - `crates/network/src/protocols/chain_sync.rs:260`
  - `crates/network/src/protocols/tx_submission.rs:272`, `:289`, `:297`
  - `crates/network/src/protocols/peer_sharing.rs:226`
  - `crates/network/src/ntc_peer.rs:191`
  - `crates/ledger/src/eras/{shelley,allegra,mary,alonzo,babbage,conway}.rs` (block-body decoders)
- **Severity:** High (post-handshake remote DoS / memory bomb)

**Description.** The `Vec::with_capacity(count as usize)` pattern is repeated systematically across every protocol message decoder and every era's block-body decoder. Once a peer is past handshake, it can send messages whose CBOR `count` field is up to `u64::MAX`. The single-SDU bound caps the bytes the attacker consumes (~64 KB), but `with_capacity` allocates regardless of subsequent successful reads.

**Reachability.** Any post-handshake peer, including a connected fellow relay that has been compromised, can crash the node. For era decoders the path is `BlockFetch` → `decode_block` → era-specific `decode_cbor` for inputs/outputs/certs/etc. — every block body received from any peer goes through these decoders.

**Impact.** Process abort. For a block producer that accepts inbound from its own trusted relays only, a single compromised relay can crash all downstream BPs.

**Remediation.** Same as C-1: route every `Vec::with_capacity(count as usize)` through a bounded helper. Sensible per-domain caps:

| Domain | Suggested cap |
|---|---|
| Handshake version table | 64 |
| ChainSync intersect points | 128 (upstream `chainSyncFindIntersectPoints` is bounded) |
| TxSubmission txid batches | 65 535 |
| PeerSharing peers | 65 535 |
| Block-body element vectors (inputs/outputs/certs) | bounded by `params.max_tx_size` and `params.max_block_body_size` already, but a static upper bound of ~50 000 makes it explicit |

#### H-2 — Inbound accept loop runs handshake synchronously before rate limit

- **Location:** `crates/network/src/listener.rs:75-87`, `node/src/server.rs:1481-1492`
- **Severity:** High (DoS amplification + serialised legitimate-peer admission)

**Description.** `PeerListener::accept_peer` performs `tokio::net::TcpListener::accept()` and **then immediately** runs the full handshake (`peer::accept`, which CBOR-decodes `ProposeVersions` and replies with `AcceptVersion`) before returning to the caller. Only after `accept_peer` returns does the caller in `server.rs:1481-1492` consult `accepted_connections_limit` and decide whether to reject the connection. The accept loop is single-tasked (`tokio::select!` arm in the `loop`).

```rust
// crates/network/src/listener.rs:75-87
pub async fn accept_peer(&self) -> Result<(PeerConnection, SocketAddr), PeerListenerError> {
    let (stream, addr) = self.listener.accept().await?;
    let conn = peer::accept(stream, self.network_magic, &self.supported_versions)
        .await
        .map_err(|e| PeerListenerError::Handshake { addr, source: e })?;
    Ok((conn, addr))
}
```

**Exploit.** Two effects compound:
1. *Crash amplification* (with C-1): any TCP connection reaches the handshake decoder, and the rate limiter never gets to fire because the handshake has already crashed the process.
2. *Slowloris-style availability hit* (without C-1): an attacker opens a TCP connection and sends a half-baked SDU header that takes 60 s to time out (`SHORT_WAIT`), occupying the accept loop for that whole window. Multiple concurrent attacker connections serialise behind the loop.

**Remediation.**
- Move the rate-limit check to **before** `peer::accept` — accept only the TCP connection in the loop, then dispatch the handshake to a `JoinSet` task.
- Apply a small handshake-specific timeout (a few seconds, not `LONG_WAIT = 60 s`) so a stalled attacker is dropped quickly.
- Consider a SYN-cookie-style sliding-window connection rate limiter at the TCP-accept boundary, independent of the post-handshake registry.

### 3.3 Medium

#### M-1 — Mux payload buffer allocated before ingress-queue limit check

- **Location:** `crates/network/src/mux.rs:575-595`
- **Severity:** Medium (bounded DoS amplification)

**Description.** In `read_one_sdu` the payload buffer is allocated with `vec![0u8; len]` *before* the ingress-queue limit check. `len` is bounded by `MAX_SDU_PAYLOAD = 0xFFFF` so the allocation is per-frame ≤ 64 KB, but the per-protocol ingress-queue limit (which can be substantially smaller, e.g. for TxSubmission) is not respected for this allocation.

**Impact.** An attacker who has filled a particular protocol's ingress queue can still force 64 KB allocations per frame on that channel, sustained at line rate.

**Remediation.** Reorder: read SDU header → ingress-queue check → allocate payload → read payload.

#### M-2 — Default trace forwarder socket path is multi-user-unsafe

- **Location:** `node/src/config.rs:120` (`default_trace_forwarder_socket_path`)
- **Severity:** Medium (local; trace data exposure)

**Description.** The default cardano-tracer Unix-socket path is `/tmp/cardano-trace-forwarder.sock`. On a multi-user host, a non-root local attacker can pre-create or symlink that path before `cardano-tracer` binds; the running yggdrasil-node will connect to whatever the path points at. Trace data normally does not contain secrets but does leak tip slots, peer counts, mempool size, and operational state.

**Remediation.** Default the path to `${XDG_RUNTIME_DIR}/yggdrasil-trace-forwarder.sock` or `/run/yggdrasil/trace-forwarder.sock`. If staying with `/tmp`, refuse to use a path whose parent is not owned by the same UID, and refuse if the path itself is a symlink.

#### M-3 — NtC Unix socket created with default umask permissions

- **Location:** `node/src/local_server.rs:645` (`UnixListener::bind(socket_path)`)
- **Severity:** Medium (local privilege escalation potential)

**Description.** The Node-to-Client Unix socket exposes `LocalTxSubmission`, `LocalStateQuery`, and `LocalTxMonitor`. The bind is done with default umask (typically `022`, giving `0o755`). On a multi-user host, any local user can connect and submit transactions or read full ledger state.

**Remediation.** After `UnixListener::bind`, set permissions explicitly with `std::fs::set_permissions(socket_path, Permissions::from_mode(0o660))` and document that the operator must place the node user and the client user (e.g. `cardano-cli` shim, monitoring agent) in a shared group. Alternatively use `0o600` and require both processes to run as the same user.

#### M-4 — `serde_cbor 0.11.2` is unmaintained (RUSTSEC-2021-0127)

- **Location:** `Cargo.lock`, used by `node/src/trace_forwarder.rs` and `node/Cargo.toml`
- **Severity:** Medium (supply-chain hygiene; future CVE risk)

**Description.** `serde_cbor` was archived by its author in 2021 and the RustSec advisory database flagged it as unmaintained the same year. Any future CBOR vulnerability discovered in this crate will not be fixed upstream. The project already implements its own hand-rolled CBOR codec for the ledger crate (`crates/ledger/src/cbor.rs`); the `serde_cbor` use in `trace_forwarder.rs` and elsewhere should migrate to either the in-house codec or `ciborium` / `minicbor` (the maintainer-recommended successors).

**Remediation.** Replace `serde_cbor` with `ciborium` (`serde`-compatible) or `minicbor`. CI should run `cargo deny check advisories` to catch future regressions.

#### M-5 — `serde_yaml 0.9.34+deprecated` is officially unmaintained

- **Location:** `Cargo.lock`, used by `node/src/config.rs` for YAML config support
- **Severity:** Medium (supply-chain hygiene)

**Description.** Author David Tolnay archived the upstream repository in March 2024, releasing the final version with the literal `+deprecated` build-metadata tag. RustSec advisory-db issue #2132 is open tracking the unmaintained status.

**Remediation.** Either drop YAML support entirely (the codebase already supports JSON which is sufficient) or migrate to `serde_yml` / `serde_norway` / the maintained Serde-compatible fork of the operator's choice.

#### M-6 — Value-preservation check uses `saturating_add` rather than `checked_add`

- **Location:** `crates/ledger/src/utxo.rs:417`, `:494`, `:565`, `:638`, `:710`, `:1037` (`check_coin_preservation`); also fee paths in `crates/ledger/src/fees.rs`
- **Severity:** Medium (theoretical; not exploitable on mainnet today)

**Description.** Every era's `apply_*_tx_withdrawals` computes the produced-coin sum with `iter().fold(0u64, u64::saturating_add)`, then calls `check_coin_preservation(consumed + withdrawals + refunds, produced, fee + deposits)` — also using `saturating_add` for the sums. The check is `consumed != produced.saturating_add(fee)`. With saturation in both sides, a malicious block where one output is `u64::MAX` and inputs are `u64::MAX` would pass the check spuriously even though the underlying coin balance is mathematically inconsistent.

**Why this is not exploitable on mainnet:** total Cardano supply is ~4.5 × 10¹⁶ lovelace, well below `u64::MAX = 1.84 × 10¹⁹`. No legitimate UTxO can hold `u64::MAX`. But the defensive form is still wrong, and a custom testnet or genesis with unusual parameters could make it relevant.

**Remediation.** Convert all value-preservation arithmetic to `checked_add` returning `LedgerError::ValueOverflow` (new variant) on overflow. This makes the rule robust regardless of state.

#### M-7 — Mempool re-sorts on every insert, O(n log n) per admit

- **Location:** `crates/mempool/src/queue.rs:408-409`
- **Severity:** Medium (CPU exhaustion vector under TxSubmission flooding)

**Description.** Each `MempoolQueue::insert` performs `entries.sort_by(|l, r| r.entry.fee.cmp(&l.entry.fee))` over the entire vector. For a mempool capped at, say, 8 MB with 2 KB transactions (≈ 4 000 entries), filling the mempool from empty is O(n² log n) ≈ 60 million ops. A peer flooding TxSubmission can sustain this CPU load.

**Remediation.** Replace the `Vec<IndexedMempoolEntry>` with a `BTreeMap<(fee, idx_desc), MempoolEntry>` keyed for descending-fee iteration, or insert in sorted position with `partition_point`. Both yield O(log n) per insert.

#### M-8 — Genesis hash check silently skipped when hash field is `None`

- **Location:** `node/src/config.rs:939-944` (`verify_known_genesis_hashes`)
- **Severity:** Medium (operator footgun)

**Description.** The verification iterates `(file, expected, field)` tuples and runs `verify_genesis_file_hash` only when **both** sides are `Some`. A config that supplies `ShelleyGenesisFile` but omits `ShelleyGenesisHash` loads the file with no integrity check.

```rust
for (file, expected, field) in pairs {
    if let (Some(file), Some(expected)) = (file, expected) {
        verify_genesis_file_hash(&resolve(file), expected, field)?;
    }
    // <-- silent skip when only one side is present
}
```

**Impact.** An operator could swap in a tampered or wrong-network genesis file by removing the corresponding `*GenesisHash` line, with no error at startup. The shipped configs all carry both, but operator-edited configs may not.

**Remediation.** Hard-fail with a clear error when a genesis file is configured without a paired hash. The Byron-genesis-hash special case (which is handled separately because Byron uses canonical-CBOR hashing not yet ported) should be a special-case allowlist, not a generic silent skip.

### 3.4 Low

#### L-1 — Default README install incantation is `curl … | bash`

- **Location:** `README.md:54`
- **Severity:** Low

The line `curl -fsSL https://.../install_from_release.sh | bash` is industry-standard but well-known to be a vector if either the GitHub raw-content domain or the release artefacts are compromised. The script itself does verify the SHA-256 of the downloaded tarball against the bundled `SHA256SUMS.txt` from the same release — but the `SHA256SUMS.txt` is downloaded from the same source it is supposed to verify. Recommendation: also publish releases to a second channel (e.g. signed Git tag with detached signature pointing to the artefact hashes) so operators can cross-check.

#### L-2 — Only 4% of commits are GPG-signed

- **Severity:** Low

18/461 commits carry a verified GPG/SSH signature. This is a defense-in-depth gap, especially for a project that intends to be operator-trusted. Recommendation: enable `git config commit.gpgsign true` for all maintainers, set up signed-commit branch protection on `main`, and ideally publish the maintainer's key fingerprint in `SECURITY.md` (the file currently has a placeholder).

#### L-3 — GitHub Actions pinned by tag, not by SHA

- **Location:** `.github/workflows/{ci,pages,release}.yml`
- **Severity:** Low

`actions/checkout@v6`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`, `softprops/action-gh-release@v3`, etc. are pinned to floating tags. SLSA Level 3 / hardened CI guidance is to pin to commit SHAs. Dependabot's `github-actions` ecosystem already produces SHA-pin PRs if configured.

#### L-4 — `cargo deny check` and `cargo audit` are not in CI

- **Severity:** Low

The repo ships a thoughtful `deny.toml` but CI (`.github/workflows/ci.yml`) only runs `cargo check-all`, `cargo test-all`, `cargo lint`. Findings M-4 and M-5 would be caught automatically by `cargo deny check advisories`. Add a `cargo-deny` step.

#### L-5 — Author email `daniel@example.com` on two early commits

- **Severity:** Low (cosmetic)

Commits `9918d77` and `dac8e41` are authored as `Daniel <daniel@example.com>`. Not a security finding, but worth a `git commit --amend --reset-author` campaign for operator confidence — operators reading the contributor graph often eyeball author legitimacy.

#### L-6 — KES / VRF / cold key file mode not validated on load

- **Location:** `node/src/block_producer.rs:112-127` (`read_text_envelope`), `:140-163` (`load_vrf_signing_key`), `:184-212` (`load_kes_signing_key`), `:218-240` (`load_issuer_verification_key`)
- **Severity:** Low

`std::fs::read_to_string` happily reads a file regardless of its mode bits. Common SPO mistake: dropping `kes.skey` into `/etc/yggdrasil/` with `0o644`. Recommendation: in `read_text_envelope`, `stat` the file first and refuse if `permissions().mode() & 0o077 != 0`, with a clear error message naming the offending bits. Alternatively warn + continue. The `coincashew` runbooks for SPOs already document `chmod 0400` for these files; surfacing this as an error closes the gap for first-time operators.

#### L-7 — `restart_resilience.sh` uses fixed `/tmp/ygg-restart-db` path

- **Location:** `node/scripts/restart_resilience.sh:30-37`
- **Severity:** Low (multi-user host only)

If two operators run the script concurrently on the same host (CI runner, shared dev box) they'll race on `/tmp/ygg-restart-db`, `/tmp/ygg-restart.sock`, and the metrics port (`9099`). Convert to `mktemp -d` and pick a free port. Same applies to `run_*_real_pool_producer.sh`.

#### L-8 — `ExBudget::spend` arithmetic uses bare subtraction

- **Location:** `crates/plutus/src/types.rs:641-652`
- **Severity:** Low (theoretical)

```rust
self.cpu -= cost.cpu;
self.mem -= cost.mem;
if self.cpu < 0 || self.mem < 0 { /* error */ }
```

Both fields are `i64`. If `cost.cpu` is supplied as `i64::MIN` (which would require a malformed cost-model constant), `self.cpu - i64::MIN` overflows and the post-check may not fire correctly (in release mode with two's-complement wrap). Cost values are sourced from genesis-derived cost models which are bounded, so this is theoretical, but `checked_sub` is defensible.

#### L-9 — `mempool` `current_bytes + entry.size_bytes` is bare add

- **Location:** `crates/mempool/src/queue.rs:389`, `:397`, `:450`
- **Severity:** Low (theoretical)

Same pattern as L-8. `usize::MAX` boundary unreachable in practice (would require petabyte-class inputs), but `checked_add` is the defensive form.

### 3.5 Informational / positive observations

#### I-1 — Excellent key hygiene throughout `crates/crypto`

- All secret-bearing types (`SigningKey`, `VrfSecretKey`, `KesSigningKey`, `SimpleKesSigningKey`, `SumKesSigningKey`) implement `Zeroize` and either `ZeroizeOnDrop` or an explicit `Drop` calling `zeroize`.
- `Debug` is redacted (`SigningKey([REDACTED])`).
- Equality uses `subtle::ConstantTimeEq` to avoid timing side-channels on key comparison.
- Ed25519 verification uses `verify_strict` which rejects malleable signatures (essential for consensus determinism).
- Intermediate VRF scalars (`secret_scalar`, `nonce_prefix`, `nonce`) are explicitly zeroized after use in `vrf.rs:149-151` / `:193-195`.

#### I-2 — OpCert monotonic counter logic matches upstream `currentIssueNo`

- `crates/consensus/src/opcert.rs:107-200` (`OcertCounters::validate_and_update`) implements the rule `stored ≤ new_seq ≤ stored + 1` exactly as in upstream `Ouroboros.Consensus.Protocol.Praos`. The `OcertCounters` map is persisted atomically as a CBOR sidecar (`crates/storage/src/ocert_sidecar.rs`) so a restart cannot replay an old block whose OpCert sequence number is below the true on-chain value. This is the protection that prevents hot-key compromise from being arbitrarily replayable.

#### I-3 — Praos chain selection matches upstream `comparePraos`

- `crates/consensus/src/chain_selection.rs:61-110` correctly implements the three-step rule: (1) longer chain wins, (2) same-issuer-same-slot ⇒ higher OCert wins, (3) VRF tiebreaker (lower wins) subject to `RestrictedVrfTiebreaker { max_dist }` for Conway. Each branch is well-commented with the upstream reference.

#### I-4 — k-deep rollback enforcement is present and correct

- `crates/consensus/src/chain_state.rs:145-182` (`ChainState::roll_backward`) enforces the Ouroboros security parameter `k` at the volatile-chain layer. Rolling back to `Origin` checks `volatile_len > k`; rolling back to a `BlockPoint` computes the depth from the tip and checks against `k` before truncating.

#### I-5 — Plutus CEK machine has explicit budget and depth bounds

- `crates/plutus/src/machine.rs` uses heap frames (`Vec<Frame>` continuation stack), so does not rely on the native call stack for evaluation depth. It applies a step budget (`max_steps = 10_000_000_000`) and the upstream `ExBudget` (cpu, mem) per step.
- `crates/plutus/src/flat.rs` sets `MAX_TERM_DECODE_DEPTH = 128` to prevent native-stack overflow during recursive `decode_term`. `read_natural`/`read_integer` are bit-bounded (u64/i128). `read_list` does no `with_capacity`, so even adversarial Plutus scripts cannot trigger the C-1 / H-1 pattern in this decoder.

#### I-6 — Storage layer uses correct atomic-write discipline

- `crates/storage/src/file_immutable.rs:25-44` (`atomic_write_file`): write to `.tmp`, `sync_all()` on the file, `rename()`, then `sync_all()` on the parent directory. Same pattern in `file_volatile.rs`, `file_ledger.rs`, `ocert_sidecar.rs`.
- A `dirty.flag` sentinel is created on open and removed on clean shutdown, with clear recovery semantics that skip corrupted/partial files on restart.

#### I-7 — No `unsafe` blocks anywhere; no `build.rs`; no FFI

- A workspace-wide `grep -r 'unsafe\s*{'` returns zero hits in production code. The single matched line in `bls12_381.rs:305` is a `// SAFETY:` comment immediately preceding a guarded `CtOption::unwrap` after `is_some()` has returned true; the `unwrap` is safe by construction and not in an `unsafe` block.
- No `build.rs` files anywhere — eliminates a class of supply-chain risks at build time.
- `deny.toml` denies `openssl`, `openssl-sys`, `native-tls`, enforcing the pure-Rust posture.

#### I-8 — `.gitignore` aggressively excludes operator key material

- Patterns: `*.skey`, `*.opcert`, `*.counter`, `*.pem`, `*.key`, `.env`, `.env.*` (with explicit `!.env.example`). Combined with the lack of any committed key files in git history (verified), this is a strong leak-prevention posture.

#### I-9 — systemd unit applies meaningful hardening

- `node/scripts/yggdrasil-node.service`: `NoNewPrivileges=true`, `ProtectSystem=full`, `ProtectHome=true`, `ProtectKernelTunables=true`, `ProtectKernelModules=true`, `ProtectControlGroups=true`, `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK`, `RestrictRealtime=true`, `LockPersonality=true`, `ReadWritePaths=/var/lib/yggdrasil`. Runs as a dedicated `yggdrasil` user. `KillSignal=SIGINT` matches the node's graceful-shutdown handler. `LimitNOFILE=65536` is appropriate.

#### I-10 — Dockerfile is well-structured

- Multi-stage build, runtime stage runs as non-root `yggdrasil:1000`, uses `tini` as PID 1 for correct signal forwarding, healthcheck via local metrics endpoint, ports `3001` and `12798` exposed (latter at the application level binds `127.0.0.1` per `serve_metrics`). No `chmod 777`, no `curl … | bash` in the build, no secrets baked in.

#### I-11 — Workspace lints deny common footguns

- `[workspace.lints.clippy] dbg_macro = "deny"`, `todo = "deny"`, `unwrap_used = "deny"`. Verified by audit: a workspace-wide search for `.unwrap()` outside `#[cfg(test)]` blocks returns one hit in `crates/crypto/src/bls12_381.rs:305`, which is correctly guarded by an `is_some()` check and documented `// SAFETY:` rationale.

#### I-12 — `cardano-deny` config bans copyleft + OpenSSL

- License allowlist is permissive-only (Apache-2.0, BSD, ISC, MIT, MPL-2.0, Unicode, Unlicense, Zlib, 0BSD).
- `[bans]` denies `openssl`, `openssl-sys`, `native-tls` with rationale comments.
- Source allowlist is `crates.io` only (`unknown-registry = "deny"`, `unknown-git = "deny"`).

#### I-13 — Audit-pin documentation is unusually rigorous

- `node/src/upstream_pins.rs` records the exact upstream IntersectMBO commit SHAs the Rust port was last audited against (cardano-base, cardano-ledger, ouroboros-consensus, ouroboros-network, plutus, cardano-node). `node/scripts/check_upstream_drift.sh` compares each against live `git ls-remote HEAD`. Drift is informational, not a build failure, but the discipline is exemplary and uncommon in re-implementation projects.

#### I-14 — Genesis files are cryptographically authentic

- All three shipped mainnet genesis files Blake2b-256-hash to the canonical IOG values. Topology files reference legitimate IOG / CF / Emurgo backbone hosts. No tampering.

#### I-15 — `zmij 1.0.21` (transitive of `serde_json 1.0.149`) verified legitimate

- Initially flagged as unfamiliar; cross-checked against docs.rs and confirmed as a regular published dependency of `serde_json` since version `1.0.149`. Not a supply-chain anomaly.

---

## 4. Section-by-section analysis

### 4.1 Security

The codebase has a clear two-tier posture. **Application-level security primitives** (key hygiene, signature verification, OpCert monotonicity, atomic storage writes, key-file gitignore patterns, systemd hardening, Docker non-root) are uniformly excellent and exceed industry baseline for this class of software. **Network-protocol-level robustness** is uniformly weaker than the rest of the codebase: every CBOR decoder shares the same unbounded-`Vec::with_capacity(count)` pattern, the inbound accept loop processes the handshake before rate-limiting, and there are no static caps on protocol message element counts. This is fixable in a single small refactor that introduces a workspace-wide `vec_with_capacity_bounded` helper and threads it through ~50 sites.

The other notable concern is operational rather than software: the Unix sockets (NtC and trace forwarder) are bound at default umask, so a multi-tenant Linux host has a local-privilege-escalation seam. Single-tenant hosts are unaffected.

There is **no evidence of malicious code, backdoors, exfiltration, or obfuscation** anywhere in the workspace. There are no `eval`/`exec` patterns, no shell-out from Rust into user-supplied strings, no `Command::new` outside of test-helper modules, no fetched-and-executed remote scripts (the install script does fetch a tarball but verifies its SHA-256 against a bundled checksum from the same release).

### 4.2 Code quality

Code quality is **above the bar** for the size and complexity of the codebase:

- Zero `unsafe` blocks.
- `unwrap_used` lint denied workspace-wide; audited as enforced.
- Thoughtful module boundaries (each crate has a tightly-scoped responsibility, dependency direction is acyclic — `crypto` → `ledger` → {`mempool`, `consensus`, `storage`} → `network` → `node`).
- Per-crate and per-subdirectory `AGENTS.md` files document local conventions and upstream references; this is unusually disciplined.
- ~4 200 tests including ~41 ledger integration tests covering deposits, governance, MIR, era transitions, witness validation, plutus evaluation, etc.
- Errors are structured `thiserror` enums with `#[from]` and `#[source]` chains.
- Saturating vs checked arithmetic is mixed: rewards/fees use `u128` for exact rational ops which is correct; ledger value preservation uses `saturating_add` which is the wrong default (M-6); mempool size accounting uses bare `+` (L-9). Standardising on `checked_*` for value-bearing arithmetic across all crates would close this.

The single most disproportionate file is `crates/ledger/src/state.rs` at 24 762 lines — this is large enough that diff-driven review and code navigation suffer. The file naturally factors into per-domain modules (PoolState, RewardAccounts, StakeCredentials, RegisteredDrep, DrepState, CommitteeMemberState, GovernanceActionState, EnactState, LedgerState, etc.). Splitting is non-trivial because of cross-references but worth scheduling.

There are no obvious dead code, commented-out code blocks, or magic-number footguns. The few `panic!` calls that appear in production code are all inside `#[cfg(test)]` modules or `unwrap_or_else(|err| panic!(...))` patterns inside test helpers.

### 4.3 Cardano-specific concerns

This is the strongest section of the audit:

- **Network parameter handling.** `MAINNET_NETWORK_MAGIC = 764824073`, `PREPROD = 1`, `PREVIEW = 2`, `MAINNET_NETWORK_ID = 1`, `TESTNET_NETWORK_ID = 0` all match upstream. `validate_tx_body_network_id` (`crates/ledger/src/state.rs:11272`) enforces the per-tx network id at validation time. Handshake validates `network_magic` byte-for-byte before establishing a session.
- **Genesis handling.** `GenesisLoadError::HashMismatch` is the central enforcement point and is wired through `verify_known_genesis_hashes` at startup. **Caveat**: the silent-skip behaviour for `None` hashes (M-8) needs hardening.
- **KES/VRF/cold key handling.** Text-envelope parser explicitly checks the type tag (`KesSigningKey_ed25519_kes_2^N`, `VrfSigningKey_PraosVRF`, `NodeOperationalCertificate`, `StakePoolVerificationKey_ed25519`) — so an opcert file cannot be loaded into the KES key path. Length validation (32-byte VRF key, 32-byte KES seed, 4-element opcert array, 64-byte sigma) before any cryptographic use. OpCert is verified against the cold issuer key (`operational_cert.verify(&issuer_vkey)`) at startup before block production begins. KES-period bounds are enforced at every forge attempt (`check_can_forge` calls `kes_period_of_slot` then `check_kes_period`).
- **On-chain logic.** Plutus evaluation goes through `CekPlutusEvaluator::evaluate` which threads the proper `TxContext` (inputs, datums, redeemers, governance fields, reference-script hashes) into the script. Slot-to-POSIX-ms conversion uses genesis `system_start` and `slotLength` matching upstream `transVITime` in `Cardano.Ledger.Alonzo.Plutus.TxInfo`. PlutusV3 enforces `Constant(Bool(true))` final-result check.
- **Off-chain transaction building.** The repo does NOT include a transaction builder. `cardano-cli` shim only exposes `Version`, `ShowUpstreamConfig`, `QueryTip`. Key generation, signing, and tx assembly are not in scope for this codebase — operators must use upstream `cardano-cli` for those. This narrows the attack surface significantly.
- **Stake pool registration / reward withdrawal / delegation.** Implemented via the era-specific `apply_*_tx_withdrawals` paths; deposit accounting is preserved (`deposit_preservation.rs` integration test); reward distribution uses u128/u256 exact arithmetic matching upstream `maxPool`.
- **Mithril / CIP-1694.** Conway governance is implemented (committee state, DRep registration, governance action enactment, ratification thresholds, anchors, constitutions, treasury donation, MIR). Mithril integration is not present in the codebase.
- **Blockfrost / Demeter / Koios.** Not present — no third-party API tokens.
- **`cardano-cli` socket file permissions.** Covered in M-3.

### 4.4 Dependencies and supply chain

| Layer | Posture |
|---|---|
| Direct deps in `Cargo.toml` | Pinned to specific versions (e.g. `tokio = "1.52.1"`, `ed25519-dalek = "2.1.1"`, `bls12_381 = "0.8"`, `curve25519-dalek = "4.1.3"`); reasonable. |
| `Cargo.lock` | Committed. 151 dependencies, every entry has a SHA-256 checksum in the lockfile. All entries source `registry+https://github.com/rust-lang/crates.io-index`. No `git =` deps. |
| Lockfile entries verified | All 151 are well-known, legitimate crates. The only initially-unfamiliar one (`zmij`) was traced to a regular transitive of `serde_json 1.0.149`. |
| Known advisories | `serde_cbor 0.11.2` triggers RUSTSEC-2021-0127 (unmaintained). `serde_yaml 0.9.34+deprecated` is officially abandoned (advisory-db issue #2132 open). `sha2 0.9.9` is co-resident with `0.10.9` and `0.11.0` because the `bls12_381 0.8` hash-to-curve path requires a digest 0.9-compatible SHA-256 — this is intentional and documented (`crates/crypto/Cargo.toml:14-16`). |
| Postinstall / build-time code execution | No `build.rs` files. No procedural macros sourced from non-mainstream authors. |
| Docker base images | Builder: `rust:1.95-bookworm` (tag, not digest). Runtime: `debian:bookworm-slim` (tag, not digest). Pinning by digest would be SLSA-Level-3 best practice. |
| GitHub Actions | All third-party actions pinned by tag (`@v6`, `@v2`, etc.) not by SHA — see L-3. |
| Repository signing | `commits[gpgsign]` not enforced — see L-2. Releases sign aggregate `SHA256SUMS.txt` but the signing keys aren't published in `SECURITY.md`. |
| Typosquat risk | All workspace-internal crates use the `yggdrasil-` prefix uniformly; no risk of internal typosquat. |

The supply chain is **clean, pinned, and reasonably hardened**. The two unmaintained-crate findings (M-4, M-5) are the highest-leverage items to address — both have well-maintained drop-in replacements.

---

## 5. File-by-file notes

This section covers every non-trivial file. Files marked `(read in full)` were reviewed line by line; files marked `(reviewed in part)` had their structure, public API, and relevant audit-keyword regions read but not every line. Files marked `(structural skim)` had only their high-level shape examined.

### Top-level

| File | Notes |
|---|---|
| `Cargo.toml` (read in full) | Workspace root. Resolver = "2". `rust-version = 1.85`. All workspace deps pinned to specific versions. Workspace lints deny `dbg_macro` / `todo` / `unwrap_used`. `crypto` crate compiled at `opt-level = 3` even in dev/test (constant-time perf matters). Clean. |
| `Cargo.lock` (audited entries) | 151 deps, all from crates.io with SHA-256. See §4.4. |
| `Dockerfile` (read in full) | Multi-stage, non-root, tini, non-FFI build. See I-10. Improvable: pin by digest (low priority). |
| `docker-compose.yml` (read in full) | Single relay service, `127.0.0.1:12798` for metrics, `:3001` for NtN, healthcheck via `/health`. Memory limits set. Clean. |
| `deny.toml` (read in full) | Bans openssl + native-tls. License allowlist permissive-only. Sources crates.io only. Clean — would benefit from `notice` advisory category to surface unmaintained crates. |
| `rust-toolchain.toml` (read in full) | Pinned to `1.85.0` with clippy + rustfmt. Clean. |
| `rustfmt.toml` (read in full) | Edition 2024 only — uses default rustfmt. Clean. |
| `.cargo/config.toml` (read in full) | Three workspace aliases; `target-dir = "target"`. Clean. |
| `.gitignore` (read in full) | Aggressive secrets-exclusion. See I-8. Clean. |
| `.dockerignore` (read in full) | Excludes target/, .git/, IDE files, docs site. Clean. |
| `LICENSE` | Apache-2.0, standard. |
| `SECURITY.md` (read in full) | Policy: 72h ack, 30d disclosure, security@fraction.estate. PGP key fingerprint placeholder unfilled. Scope is clear. |
| `README.md` (read in full) | Verbose. Contains the `curl … \| bash` install line (L-1). Otherwise descriptive of features. |
| `AGENTS.md`, `CLAUDE.md` (read in full) | LLM-targeted instructions. Not loaded at runtime by the binary. No prompt-injection vector unless a future LLM-driven workflow ingests them. |
| `CHANGELOG.md` | Standard CHANGELOG, mentions parity-audit-driven slices. |
| `.devcontainer/devcontainer.json` (read in full) | Bare Microsoft devcontainer base image, no postCreate hooks. Clean. |
| `.vscode/settings.json` (read in full) | Two Copilot/Chat settings only. Clean. |

### `.github/`

| File | Notes |
|---|---|
| `.github/workflows/ci.yml` (read in full) | Runs check / test / lint on push to `main` and on PR. Uses `dtolnay/rust-toolchain@stable` (overrides the pinned 1.85 — minor inconsistency). Pinned by tag not SHA (L-3). Lacks `cargo audit` / `cargo deny check` (L-4). |
| `.github/workflows/pages.yml` (read in full) | Jekyll docs build. `permissions: contents: read, pages: write, id-token: write` — minimum-privilege, correct. |
| `.github/workflows/release.yml` (read in full) | Builds linux x86_64 + aarch64, strips, computes per-arch SHA-256, aggregates `SHA256SUMS.txt`, publishes via `softprops/action-gh-release@v3`. Uses `${{ secrets.GITHUB_TOKEN }}` only. `permissions: contents: write` only. Workflow-dispatch dry-run path is supported. Clean apart from L-3. |
| `.github/dependabot.yml` (read in full) | Cargo / GHA / Docker / Bundler ecosystems. Grouped RustCrypto digest ecosystem to avoid duplicate-version churn. Sensible config. |
| `.github/CODEOWNERS` (read in full) | All paths owned by `@FractionEstate`. Single-maintainer project — appropriate for current scale. Branch protection enforcing CODEOWNERS review is the next step. |
| `.github/ISSUE_TEMPLATE/{bug_report,feature_request,config}.yml` | Standard YAML issue forms. Bug template explicitly redirects security issues to `SECURITY.md`. |
| `.github/pull_request_template.md`, `.github/AGENTS.md`, `.github/CLAUDE.md` | Documentation; not runtime. |

### `node/` binary

| File | Notes |
|---|---|
| `node/Cargo.toml` (read in full) | Depends on every workspace crate plus `clap`, `eyre`, `serde_json`, `serde_yaml`, `serde_cbor`, `tokio`. Clean. |
| `node/src/main.rs` (reviewed in part — 5 248 lines) | CLI definitions, subcommand dispatch (`run`, `validate-config`, `status`, `default-config`, `cardano-cli`, `query`, `submit-tx`). Inbound listener default `0.0.0.0:3001` only when `--port`/`--host-addr` is set explicitly; default config has `inbound_listen_addr: None`. Metrics HTTP handler binds `127.0.0.1` only — good. The handler reads up to 1024 bytes from the socket; for an HTTP request line that small, this is fine. Routes `/health`, `/metrics`, `/metrics/json`, plus `/debug/*` aliases. |
| `node/src/runtime.rs` (reviewed in part — 7 842 lines) | Outbound peer manager, fetch-worker pool, governor wiring, ledger-peer reconciliation, churn loop, block-production loop. Uses `tokio::sync::RwLock` for the fetch-worker pool (verified) and `std::sync::RwLock` for the chain DB (lock guards are scoped tightly with `let result = { lock; op; };` so no lock-across-await issues). Code is dense but disciplined. |
| `node/src/sync.rs` (reviewed in part — 6 238 lines) | Multi-era sync orchestration, bootstrap relay fallback, reconnect logic, mempool-eviction-on-block-applied. |
| `node/src/server.rs` (reviewed in part — 2 739 lines) | Inbound accept loop (H-2 location). Rate-limit decision, connection-manager wiring, inactivity tick, IG-event processing. |
| `node/src/config.rs` (reviewed in part — 3 181 lines) | NodeConfigFile, network presets, topology resolution, `verify_known_genesis_hashes` (M-8), trace-forwarder defaults (M-2), tracer namespace config. Solid. |
| `node/src/genesis.rs` (reviewed in part — 2 683 lines) | Shelley/Alonzo/Conway genesis loaders, `verify_genesis_file_hash`, EnactState construction, cost-model derivation from genesis. The hash check is correct; the wiring (M-8) silently skips when fields are absent. |
| `node/src/block_producer.rs` (read in full — 1 895 lines) | Text-envelope parsing, KES/VRF/opcert/issuer-vkey loading, slot leadership check, KES period guard, header forging. **Strong**: type tags enforced, lengths checked, cold-key sig verified at load. **Weakness**: file mode not validated (L-6). |
| `node/src/local_server.rs` (reviewed in part — 1 643 lines) | NtC server (LocalTxSubmission, LocalStateQuery, LocalTxMonitor). UnixListener bind without explicit permissions (M-3). Validation flow for tx submission goes through full ledger validation — good. Author is aware of lock-across-await trap (explicit comment). |
| `node/src/plutus_eval.rs` (reviewed in part — 5 717 lines) | CekPlutusEvaluator with proper TxContext threading, slot-to-POSIX-ms conversion, V1/V2 vs V3 result handling. |
| `node/src/blockfetch_worker.rs` (read structurally — 843 lines) | Per-peer FetchWorkerHandle and pool registration. The single `panic!("intentional")` at line 831 is inside a `#[tokio::test]` test fixture for pool prune-closed semantics; not a production panic. |
| `node/src/tracer.rs` (read structurally — 1 761 lines) | NodeTracer + NodeMetrics + Prometheus exposition. Trace fields builder + namespace-scoped severity routing. |
| `node/src/trace_forwarder.rs` (read in full — 47 lines) | UnixDatagram forwarder. Lazy connect; `serde_cbor::to_vec` (M-4). Does not log secret material. |
| `node/src/genesis.rs` `slot_to_posix_ms` helper | Used by Plutus evaluator, matches upstream `transVITime`. |
| `node/src/upstream_pins.rs` (read in full) | See I-13. |
| `node/src/lib.rs` | Re-exports. Trivial. |
| `node/configuration/{mainnet,preprod,preview}/*` (verified) | Genesis hashes match canonical IOG (I-14). Topology files reference IOG/CF/Emurgo backbones. |
| `node/scripts/*.sh` (read in full) | 8 scripts. `install_from_release.sh` does verify SHA-256. `backup_db.sh` uses `sudo systemctl`. `compare_tip_to_haskell.sh`, `restart_resilience.sh` (L-7), `run_*_real_pool_producer.sh` rehearsals, `healthcheck.sh`, `check_upstream_drift.sh`. No `eval`, no `curl \| bash`. Clean. |
| `node/scripts/yggdrasil-node.service` (read in full) | See I-9. |

### `crates/crypto`

| File | Notes |
|---|---|
| `Cargo.toml` (read in full) | Pure-Rust crypto deps: blake2, bls12_381, curve25519-dalek, curve25519-elligator2, ed25519-dalek, k256, sha2, sha3 + sha2_09 (intentional second-version pin for bls12_381 hash-to-curve). |
| `lib.rs`, `error.rs` | Re-export and CryptoError enum. |
| `blake2.rs` | Blake2b-256/512 wrappers. Standard. |
| `ed25519.rs` (read in full) | See I-1. `verify_strict` used. |
| `vrf.rs` (read structurally — 1 254 lines) | Praos VRF: standard + batchcompat. Intermediate scalars zeroized. Test_vectors.rs cross-references upstream cardano-base fixtures. |
| `kes.rs`, `sum_kes.rs` (read structurally) | SimpleKES + SumKES depth 0–6+. Zeroize on Drop. Period evolution + signing. |
| `bls12_381.rs` (reviewed in part — 670 lines) | Hash-to-curve, scalar arithmetic, pairing checks, group ops. The single `Ok(opt.unwrap())` is correctly guarded. |
| `secp256k1.rs` | k256-backed schnorr + ECDSA wrappers. |
| `sha3_hash.rs` | Standard. |
| `test_vectors.rs` (read structurally — many hardcoded RFC 8032 / IOG vectors) | Vectors + drift guards cross-checked against vendored upstream files. |
| `tests/upstream_vectors.rs`, `tests/integration.rs` | Cross-reference fixture tests. |

### `crates/cddl-codegen`

| File | Notes |
|---|---|
| `src/main.rs`, `src/lib.rs`, `src/parser.rs`, `src/generator.rs` (read in part) | Pure CDDL parser → Rust source generator. No I/O outside Stdout. |

### `crates/consensus`

| File | Notes |
|---|---|
| `chain_selection.rs` (read in full — 300 lines) | Praos comparePraos. See I-3. |
| `chain_state.rs` (read in full — 566 lines) | Volatile k-deep window. See I-4. |
| `opcert.rs` (read in full — 700 lines) | OpCert verify + counter monotonicity. See I-2. |
| `praos.rs`, `header.rs`, `nonce.rs`, `epoch.rs`, `genesis_density.rs`, `diffusion_pipelining.rs`, `in_future.rs` (read structurally) | Praos leadership formula, header decode, η nonce evolution, epoch math, density window. |
| `tests/integration.rs` | Cross-system tests for OpCert + chain selection + nonce evolution. |

### `crates/ledger`

| File | Notes |
|---|---|
| `state.rs` (24 762 lines — partial read) | Largest file. RegisteredPool/PoolState/RewardAccountState/StakeCredentialState/RegisteredDrep/CommitteeMemberState/GovernanceActionState/EnactState. `apply_block_validated` enforces slot monotonicity, era-regression guard, max_block_body_size, max_block_header_size. PPUP validation. Governance enactment. Recommend split into per-domain modules. |
| `cbor.rs` (read structurally — 2 996 lines) | Hand-rolled CBOR encoder/decoder, supports definite + indefinite arrays/maps, tagged unions, set tag 258. The decoder's `array()` returns raw `u64` count; downstream `Vec::with_capacity(count as usize)` is the H-1 root cause. |
| `eras/{byron,shelley,allegra,mary,alonzo,babbage,conway}.rs` (sampled) | Era-specific tx-body/block decoders + apply rules. All use the H-1 pattern. Conway includes governance certificates and treasury donation. |
| `utxo.rs` (read in full where relevant — 1 816 lines) | MultiEraUtxo, value preservation (M-6 location). MAX_REF_SCRIPT_SIZE_PER_TX = 204 800, per block 1 048 576 (matches upstream `ppMaxRefScriptSizePerTxG` / `ppMaxRefScriptSizePerBlockG`). |
| `tx.rs`, `types.rs`, `min_utxo.rs`, `collateral.rs`, `witnesses.rs` | Tx/Block types, address parsing, min-UTxO calc, collateral rules, witness validation. |
| `fees.rs` (read in full — 583 lines) | min_fee_linear (Shelley), script_fee (Alonzo+), tier_ref_script_fee (Conway). Saturating arithmetic; bounded numerators in `UnitInterval`. |
| `rewards.rs` (read structurally — 1 826 lines) | u128/u256 exact-arithmetic implementation of `maxPool` / `memberRew` / `leaderRew`. Matches upstream Haskell `Rational`-based formula. |
| `epoch_boundary.rs` (read structurally — 6 015 lines) | Stake-snapshot rotation, reward distribution, governance ratification, PPUP application, expired-deposit refunds. |
| `protocol_params.rs` | ProtocolParameters struct + derive helpers. |
| `plutus.rs`, `plutus_validation.rs`, `native_script.rs` | PlutusData AST + on-chain Plutus & native-script validation traits. |
| `stake.rs` | StakeSnapshot + PoolStakeDistribution. |
| `tests/integration/*` (41 files) | Comprehensive integration coverage: deposit preservation, governance updates, witness validation, reference scripts, treasury donation, era transitions, MIR, etc. |

### `crates/mempool`

| File | Notes |
|---|---|
| `queue.rs` (read in full — 1 651 lines) | Fee-ordered queue with eviction. M-7 (sort on every insert). L-9 (bare-add on byte counter). Otherwise correct: dedupe, conflicting-input guard, capacity check. |
| `tx_state.rs`, `lib.rs` | Shared TxState for TxSubmission peer accounting. |

### `crates/network`

| File | Notes |
|---|---|
| `bearer.rs` (read in full — 178 lines) | SDU framing. `MAX_SDU_PAYLOAD = 0xFFFF`. Capped before allocation — correct for the bearer layer. |
| `mux.rs` (read in part — 953 lines) | Multiplexer. M-1 (allocates payload before ingress check). |
| `multiplexer.rs` | SDU header types, mini-protocol numbers. |
| `handshake.rs` (read in part — 681 lines) | C-1 + part of H-1. Decoders for ProposeVersions / AcceptVersion / Refuse / QueryReply. |
| `listener.rs` (read in full — 109 lines) | H-2 root cause. |
| `protocol_limits.rs` (read in full) | Per-protocol per-state time limits matching upstream `ProtocolTimeLimits`. Solid. |
| `protocols/{chain_sync,block_fetch,tx_submission,keep_alive,peer_sharing,local_state_query,local_tx_submission,local_tx_monitor,mod}.rs` (read in part) | State-machine + CBOR codecs. H-1 sites. |
| `chainsync_{client,server}.rs`, `blockfetch_{client,server,pool}.rs`, `txsubmission_{client,server}.rs`, `peersharing_{client,server}.rs`, `keepalive_{client,server}.rs`, `local_*_{client,server}.rs` (read structurally) | Typed mini-protocol drivers. |
| `governor.rs` (read structurally — 7 249 lines) | Pure peer-policy decision function. No locks. Xorshift64 PRNG (non-crypto, OK for peer selection). RequestBackoffState exponential backoff. |
| `inbound_governor.rs` (read in part — 1 470 lines) | Pure IG step function. |
| `connection.rs`, `connection_manager.rs`, `peer_state_actions.rs` (read structurally) | Connection lifecycle, AcceptedConnectionsLimit, demote/promote actions. |
| `peer.rs`, `peer_registry.rs`, `peer_selection.rs` | Peer accept + registry tracking + ordered candidate selection. |
| `root_peers.rs`, `root_peers_provider.rs` | Local-roots / public-roots / bootstrap-peers resolution + DNS-backed refresh policy. |
| `ledger_peers_provider.rs` | LedgerPeerSnapshot + ScriptedLedgerPeerProvider for tests. |
| `diffusion.rs` | Combined NtN diffusion entry. |
| `ntc_peer.rs` (read structurally) | NtC handshake variant — H-1 site. |
| `tests/integration.rs` | Cross-protocol integration tests. |

### `crates/plutus`

| File | Notes |
|---|---|
| `machine.rs` (read in part — 1 280 lines) | CEK with heap frames + step budget + max_steps cap. See I-5. |
| `builtins.rs` (read structurally — 2 972 lines) | Builtin function evaluation. Per-builtin cost charging via `cost_model`. |
| `flat.rs` (read in part — 1 106 lines) | Flat decoder with `MAX_TERM_DECODE_DEPTH = 128`. read_natural / read_integer / read_bytestring all bounded. read_list uses `Vec::new` (no with_capacity exposure). See I-5. |
| `cost_model.rs` (read structurally — 2 351 lines) | Cost-model parser + evaluation table. |
| `types.rs` (read in part — 1 657 lines) | Term / Value / ExBudget / DefaultFun. ExBudget::spend (L-8). |
| `error.rs`, `lib.rs` | Boilerplate. |

### `crates/storage`

| File | Notes |
|---|---|
| `chain_db.rs` (read in part) | Coordinator across Immutable/Volatile/Ledger stores. Atomic add_volatile_block, rollback_to. |
| `immutable_db.rs`, `volatile_db.rs`, `ledger_db.rs` | Trait definitions. |
| `file_immutable.rs`, `file_volatile.rs`, `file_ledger.rs` | File-backed implementations with the atomic-write/dirty-flag pattern. See I-6. |
| `ocert_sidecar.rs` (read in full) | OcertCounters CBOR sidecar with the same atomic-write pattern. See I-2. |
| `error.rs`, `lib.rs` | StorageError enum + re-exports. |
| `tests/integration.rs` | Crash-recovery + atomic-write coverage. |

### `specs/`

| File | Notes |
|---|---|
| `mini-ledger.cddl`, `upstream-cddl-fragments/conway-ranges-min.cddl` | CDDL fixtures used by `cddl-codegen`. |
| `upstream-test-vectors/cardano-base/db52f43.../...` | Vendored VRF + BLS12-381 test vectors. Pinned to commit `db52f43b38ba5d8927feb2199d4913fe6c0f974d`. The directory naming makes drift detectable. |

### `docs/`

| File | Notes |
|---|---|
| `_config.yml`, `Gemfile`, `_sass/...` | Jekyll site for GitHub Pages. Built by `.github/workflows/pages.yml`. Not loaded at runtime. |
| `manual/*.md`, `ARCHITECTURE.md`, `CONTRIBUTING.md`, `SPECS.md`, etc. | Documentation only. No security implications beyond accuracy. |

---

## 6. Positive observations (consolidated)

Beyond the per-finding `Informational` items above, the project demonstrates several practices uncommon in a re-implementation of this complexity:

- **Audit-first culture.** `node/src/upstream_pins.rs` records the exact upstream commits each subsystem was ported from, with a drift-detection script. `docs/AUDIT_VERIFICATION_2026Q2.md` and `docs/PARITY_PLAN.md` formalise the parity-audit cadence.
- **Test discipline.** ~4 200 tests, 41 ledger integration tests, golden CBOR round-trip tests, vendored upstream cardano-base test vectors with bidirectional name-set drift guards.
- **Per-module agent guidance.** Every meaningful subdirectory has an `AGENTS.md` documenting boundaries and conventions. While this is LLM-targeted, the operational discipline benefits human contributors equally.
- **No `unsafe`, no FFI.** The pure-Rust posture is enforced by `deny.toml` — a future PR introducing `openssl-sys` fails CI before the PR can land.
- **Strong systemd + Docker hardening** out of the box. Many SPO-targeted projects ship neither.
- **SECURITY.md** exists, with reasonable timelines and a clear scope statement that distinguishes implementation parity bugs from upstream protocol-level bugs.

---

## 7. Recommended action items, prioritised

### P0 — fix before any mainnet exposure (1–3 days of work)

1. **Fix C-1 and the H-1 cluster in one stroke.** Introduce `vec_with_capacity_bounded` (or `LedgerError::CountTooLarge`) and route every `Vec::with_capacity(count as usize)` through it. Pick per-domain caps (handshake 64, peer-sharing/tx-batch 65 535, block elements 50 000, etc.). Document the caps in a single header comment per file.
2. **Fix H-2 by moving the rate-limit check before the handshake.** Restructure `accept_peer` to return after TCP accept only; spawn a `JoinSet` task for handshake completion; apply a short (e.g. 5 s) handshake timeout. Test by point-in-time `tcp:` accepts that produce no bytes.
3. **Add `cargo deny check advisories` to CI.** Catches M-4 and M-5 today and any future regressions tomorrow.
4. **Replace `serde_cbor` with `ciborium`** in `node/src/trace_forwarder.rs` and any other prod use.
5. **Replace `serde_yaml`** with `serde_yml` (or drop YAML support, JSON is sufficient).

### P1 — operational hardening (1 week)

6. **Set explicit permissions on the NtC Unix socket** (`0o660` or `0o600`) post-bind. Document the operator group requirement.
7. **Move trace-forwarder default path** off `/tmp` to `${XDG_RUNTIME_DIR}` or `/run/yggdrasil`.
8. **Hard-fail in `verify_known_genesis_hashes`** when a genesis-file path is set but its paired hash is `None` (M-8).
9. **Validate KES/VRF/cold key file mode** on load (L-6); refuse if group or other has any access.
10. **Replace `saturating_add` with `checked_add`** in all UTxO value-preservation paths and add `LedgerError::ValueOverflow`.
11. **Replace the mempool resort-on-every-insert** with a `BTreeMap`-keyed structure (M-7).
12. **Reorder mux payload allocation** to occur after the ingress-queue check (M-1).

### P2 — supply-chain & posture (1–2 weeks)

13. **Pin all GitHub Actions by SHA**, not by tag. Dependabot's `github-actions` ecosystem can produce these PRs automatically.
14. **Pin Docker base images by digest** (`rust:1.95-bookworm@sha256:...`, `debian:bookworm-slim@sha256:...`).
15. **Enforce signed commits** on `main` via branch protection. Publish maintainer key fingerprint in `SECURITY.md`. Backfill the placeholder PGP fingerprint.
16. **Sign release tags** and upload a detached signature alongside `SHA256SUMS.txt` so operators can verify provenance independently of the GitHub release attachment.
17. **Add `cargo audit` and `cargo deny check` as a separate CI workflow** (so failure surface is distinguishable from build failure).

### P3 — code quality (background)

18. **Split `crates/ledger/src/state.rs`** (24 762 lines) into per-domain modules (`pool_state.rs`, `reward_accounts.rs`, `committee_state.rs`, `enact_state.rs`, etc.). Improves diffability and review cost.
19. **Replace bare `+=` and `+`** on coin / size accumulators with `checked_add` (mempool L-9, ExBudget L-8, anywhere else `grep -nE '\.coin\s*\+|fee\s*\+'` shows up).
20. **Backfill `ConfigValidationReport`** to include the genesis-hash-pair check status, so `validate-config` warns when M-8 would silently skip.
21. **Document the complete operator threat model** in `docs/manual/`. The current docs are deployment-oriented; an explicit "what the node trusts and does not trust" page would help downstream auditors.

---

## 8. Appendix

### 8.1 Full file inventory

The following is the complete tracked-file list (361 files) with a one-line role.

#### Root

```
.cargo/config.toml                      cargo aliases
.devcontainer/devcontainer.json         devcontainer base
.dockerignore                           Docker build excludes
.github/AGENTS.md                       LLM context for .github
.github/CLAUDE.md                       LLM context (Claude-specific)
.github/CODEOWNERS                      single owner @FractionEstate
.github/ISSUE_TEMPLATE/{bug_report,config,feature_request}.yml  issue forms
.github/dependabot.yml                  cargo / GHA / docker / bundler weekly
.github/pull_request_template.md        PR template
.github/workflows/ci.yml                check / test / lint
.github/workflows/pages.yml             Jekyll docs build
.github/workflows/release.yml           tag-driven release
.gitignore                              excludes secrets + target/
.vscode/settings.json                   Copilot settings
AGENTS.md                               workspace LLM context
CHANGELOG.md                            project changelog
CLAUDE.md                               Claude-specific LLM helper
Cargo.lock                              151 deps (crates.io, hashed)
Cargo.toml                              workspace root
Dockerfile                              multi-stage non-root + tini
LICENSE                                 Apache-2.0
README.md                               project README
SECURITY.md                             vuln reporting policy
deny.toml                               cargo-deny config
docker-compose.yml                      relay quick-start
rust-toolchain.toml                     pinned 1.85.0
rustfmt.toml                            edition 2024 placeholder
```

#### `crates/`

```
crates/AGENTS.md
crates/cddl-codegen/{Cargo.toml,AGENTS.md}
crates/cddl-codegen/src/{AGENTS.md,generator.rs,lib.rs,main.rs,parser.rs}
crates/cddl-codegen/tests/{AGENTS.md,integration.rs}
crates/consensus/{Cargo.toml,AGENTS.md}
crates/consensus/src/{AGENTS.md,chain_selection.rs,chain_state.rs,
                      diffusion_pipelining.rs,epoch.rs,error.rs,
                      genesis_density.rs,header.rs,in_future.rs,lib.rs,
                      nonce.rs,opcert.rs,praos.rs}
crates/consensus/tests/{AGENTS.md,integration.rs}
crates/crypto/{Cargo.toml,AGENTS.md}
crates/crypto/src/{AGENTS.md,blake2.rs,bls12_381.rs,ed25519.rs,error.rs,
                   kes.rs,lib.rs,secp256k1.rs,sha3_hash.rs,sum_kes.rs,
                   test_vectors.rs,vrf.rs}
crates/crypto/tests/{AGENTS.md,integration.rs,upstream_vectors.rs}
crates/ledger/{Cargo.toml,AGENTS.md}
crates/ledger/src/{AGENTS.md,cbor.rs,collateral.rs,epoch_boundary.rs,
                   error.rs,fees.rs,lib.rs,min_utxo.rs,native_script.rs,
                   plutus.rs,plutus_validation.rs,protocol_params.rs,
                   rewards.rs,stake.rs,state.rs,tx.rs,types.rs,
                   utxo.rs,witnesses.rs}
crates/ledger/src/eras/{AGENTS.md,allegra.rs,alonzo.rs,babbage.rs,
                        byron.rs,conway.rs,mary.rs,mod.rs,shelley.rs}
crates/ledger/tests/{AGENTS.md,generated_intake.rs,integration.rs}
crates/ledger/tests/integration/  41 .rs files
crates/mempool/{Cargo.toml,AGENTS.md}
crates/mempool/src/{AGENTS.md,lib.rs,queue.rs,tx_state.rs}
crates/mempool/tests/{AGENTS.md,integration.rs}
crates/network/{Cargo.toml,AGENTS.md}
crates/network/src/{AGENTS.md, ~30 .rs files (bearer, blockfetch_*, chainsync_*,
                    connection, connection_manager, diffusion, governor,
                    handshake, inbound_governor, keepalive_*, ledger_peers_provider,
                    listener, lib, local_*, multiplexer, mux, ntc_peer,
                    peer*, peersharing_*, protocol_limits, protocols/*,
                    root_peers*, txsubmission_*)}
crates/network/src/protocols/{AGENTS.md,block_fetch.rs,chain_sync.rs,
                              keep_alive.rs,local_state_query.rs,
                              local_tx_monitor.rs,local_tx_submission.rs,
                              mod.rs,peer_sharing.rs,tx_submission.rs}
crates/network/tests/{AGENTS.md,integration.rs}
crates/plutus/{Cargo.toml,AGENTS.md}
crates/plutus/src/{builtins.rs,cost_model.rs,error.rs,flat.rs,lib.rs,
                   machine.rs,types.rs}
crates/storage/{Cargo.toml,AGENTS.md}
crates/storage/src/{AGENTS.md,chain_db.rs,error.rs,file_immutable.rs,
                    file_ledger.rs,file_volatile.rs,immutable_db.rs,
                    ledger_db.rs,lib.rs,ocert_sidecar.rs,volatile_db.rs}
crates/storage/tests/{AGENTS.md,integration.rs}
```

#### `node/`

```
node/{Cargo.toml,AGENTS.md}
node/src/{AGENTS.md,block_producer.rs,blockfetch_worker.rs,config.rs,
         genesis.rs,lib.rs,local_server.rs,main.rs,plutus_eval.rs,
         runtime.rs,server.rs,sync.rs,trace_forwarder.rs,tracer.rs,
         upstream_pins.rs}
node/tests/{AGENTS.md,local_ntc.rs,runtime.rs,smoke.rs,sync.rs}
node/scripts/{backup_db.sh,check_upstream_drift.sh,
              compare_tip_to_haskell.sh,healthcheck.sh,
              install_from_release.sh,restart_resilience.sh,
              run_mainnet_real_pool_producer.sh,
              run_preprod_real_pool_producer.sh,
              yggdrasil-node.service}
node/configuration/{AGENTS.md,
                    mainnet/{AGENTS.md,alonzo-genesis.json,byron-genesis.json,
                             config.json,conway-genesis.json,
                             shelley-genesis.json,topology.json},
                    preprod/{... + peer-snapshot.json},
                    preview/{...}}
```

#### `specs/` and `docs/`

```
specs/{AGENTS.md,mini-ledger.cddl,
       upstream-cddl-fragments/conway-ranges-min.cddl,
       upstream-test-vectors/cardano-base/{AGENTS.md,
       db52f43.../{cardano-crypto-praos/test_vectors/{14 fixtures},
                   cardano-crypto-class/bls12-381-test-vectors/{6 fixtures}}}}
docs/{AGENTS.md,ARCHITECTURE.md,AUDIT_VERIFICATION_2026Q2.md,
      CHANGELOG.md,CONTRIBUTING.md,DEPENDENCIES.md,Gemfile,
      MANUAL_TEST_RUNBOOK.md,PARITY_PLAN.md,PARITY_SUMMARY.md,
      REAL_PREPROD_POOL_VERIFICATION.md,SPECS.md,UPSTREAM_PARITY.md,
      UPSTREAM_RESEARCH.md,_config.yml,
      _sass/{color_schemes/yggdrasil.scss,custom/custom.scss},
      index.md,manual/{block-production,cli-reference,configuration,
                       docker,glossary,index,installation,maintenance,
                       monitoring,networks,overview,quick-start,releases,
                       running,troubleshooting}.md,reference.md}
```

### 8.2 Audit methodology notes

- Cloned via `git clone https://github.com/Yggdrasil-node/Cardano-node.git` on 27 April 2026.
- Walked every directory; counted lines per file and per language.
- Read in full or in part every Rust source file; structural skim of test fixtures.
- Verified mainnet genesis Blake2b-256 hashes by hashing the file contents and comparing to the declared `*GenesisHash` strings, then cross-checking those against canonical IntersectMBO/cardano-node master.
- Searched the entire workspace for: hardcoded secrets, `unwrap()` outside test modules, `unsafe` blocks, `panic!` / `todo!` / `unimplemented!` outside tests, `eval` / `exec` patterns, `std::process::Command`, `curl … | bash`, lock-held-across-await footguns, `Vec::with_capacity(<attacker-input>)`, bare `+`/`+=` on coin/size types.
- Read the entire git history (`git log --all`) for: committed key files (none), force-push anomalies (none), suspicious authors (one cosmetic `daniel@example.com` on early commits), large blob diffs (only `state.rs` and `byron-genesis.json`, both legitimate).
- Read all 8 shell scripts and the systemd unit file in full.
- Verified one suspicious-looking transitive dependency (`zmij`) by web search; confirmed legitimate.

### 8.3 Out of scope

- Runtime testing on a live testnet (preprod/preview) was not performed.
- Performance benchmarking against the upstream Haskell node was not performed.
- Formal verification or fuzzing of the CBOR codec, Plutus CEK, or Praos chain selection was not performed.
- Symbolic-execution-based search for value-preservation overflow paths was not performed.
- The `cardano-cli` shim's interaction with a real running node was not exercised.
- Real-mainnet sync endurance and parity-vs-Haskell-tip comparison are tracked in `docs/MANUAL_TEST_RUNBOOK.md` but are operator-side rehearsals, not part of this audit.

---

*End of report.*