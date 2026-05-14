# yggdrasil-error — workspace error envelope (Wave 2)

## Scope

Single-purpose synthesis crate. Holds:

- `YggdrasilError` — a `#[derive(thiserror::Error)]` enum whose variants
  wrap each of the per-crate domain errors (`CryptoError`, `LedgerError`,
  `ConsensusError`, `StorageError`, `MachineError`) plus a `std::io::Error`
  catch-all for I/O failures at boundaries that don't fit the domain set.
- `Result<T, E = YggdrasilError>` alias.
- `IntoYggdrasil<T>` trait that converts any `Result<T, E: Into<YggdrasilError>>`
  into `Result<T>` at the public boundary.

## Rules — Non-Negotiable

- **No `From` impls for `eyre::Report` / `anyhow::Error`.** The binary
  `main` keeps using `eyre` for its outermost boundary; typed APIs
  convert into `YggdrasilError` only where the typed envelope adds
  value (RPC error reporting, trace correlation).
- **Per-crate enums stay in their source crate.** This crate adds no
  domain variants; it only wraps. If a domain enum gains a variant,
  no change is needed here.
- **No catch-all `Other(Box<dyn Error>)` variant.** Variants are
  enumerated; unknown errors stay as their domain type and get
  wrapped at their owning crate's boundary.

## Naming parity

Synthesis crate (no upstream mirror). Declared via the strict-mirror
`## Naming parity` block in `src/lib.rs`. Allowlisted in
`docs/strict-mirror-audit.tsv` under the `(c) NO_MIRROR_NEEDS_DOCSTRING`
verdict — Wave 2 PR 4 scaffold.

## R-arc tracking

Wave 2 PR 4. Not blocking any consumer yet — Wave 5 sub-crates and
Wave 6 observability crates will adopt this envelope when they need
typed cross-crate error reporting.
