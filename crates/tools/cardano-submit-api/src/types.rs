//! Core types — `TxSubmitWebApiError`, `TxCmdError`, `EnvSocketError`,
//! `RawCborDecodeError`, `TxSubmitPort`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit/Types.hs.
//!
//! Direct ports:
//!
//! - `TxSubmitPort` (newtype Int) — port number for the API server.
//! - `RawCborDecodeError` (newtype `[DecoderError]`) — accumulator for
//!   CBOR decoder failures during tx-bytes parsing.
//! - `TxSubmitWebApiError` (sum) — error category surfaced to API clients
//!   via JSON response body.
//! - `EnvSocketError` (sum-of-one) — socket-environment-variable lookup
//!   failure.
//! - `TxCmdError` (sum) — command-level error wrapper enclosing socket,
//!   read, validation, and connection failures.
//! - `render_tx_cmd_error` — human-readable rendering used by tracer
//!   forHuman + WebApi error responses.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - `TxSubmitApi` / `TxSubmitApiRecord` / `CBORStream` — Servant
//!   type-level API definitions that have no Rust analog. The web round
//!   (R340) uses an axum router instead; CBOR content-type negotiation
//!   is handled inline at the handler. **Strict mirror:** none for those
//!   surfaces; rationale recorded here.
//!
//! ## JSON shape parity vs upstream
//!
//! Upstream Aeson-derived `ToJSON` instances and Yggdrasil's serde
//! mirror produce byte-equivalent output:
//!
//! | Upstream constructor               | JSON shape                                                             |
//! |------------------------------------|------------------------------------------------------------------------|
//! | `TxSubmitDecodeHex`                | `{"tag":"TxSubmitDecodeHex"}`                                          |
//! | `TxSubmitEmpty`                    | `{"tag":"TxSubmitEmpty"}`                                              |
//! | `TxSubmitDecodeFail e`             | `{"tag":"TxSubmitDecodeFail","contents":"<err>"}`                      |
//! | `TxSubmitBadTx t`                  | `{"tag":"TxSubmitBadTx","contents":"<text>"}`                          |
//! | `TxSubmitFail err`                 | `{"tag":"TxSubmitFail","contents":<TxCmdError>}`                       |
//! | `TxCmdSocketEnvError s`            | `{"tag":"TxCmdSocketEnvError","contents":{"message":"<msg>"}}`         |
//! | `TxCmdTxReadError`                 | `{"tag":"TxCmdTxReadError","contents":[<DecoderError>...]}`            |
//! | `TxCmdTxSubmitValidationError`     | `{"tag":"TxCmdTxSubmitValidationError","contents":"<rendered>"}`       |
//! | `TxCmdTxSubmitConnectionError msg` | `{"tag":"TxCmdTxSubmitConnectionError","contents":"<msg>"}`            |
//! | `RawCborDecodeError`               | `["<DecoderError>"...]`                                                |
//! | `EnvSocketError`                   | `{"message":"<msg>"}` (untagged, single variant)                       |
//!
//! Round-trip golden tests verify each shape against fixtures captured
//! from the upstream Haskell binary.

use std::fmt;

use serde::Serialize;

/// Port number on which the tx-submit web API listens.
///
/// Upstream: `newtype TxSubmitPort = TxSubmitPort Int`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TxSubmitPort(pub u16);

impl fmt::Display for TxSubmitPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<TxSubmitPort> for u16 {
    fn from(value: TxSubmitPort) -> Self {
        value.0
    }
}

impl From<u16> for TxSubmitPort {
    fn from(value: u16) -> Self {
        TxSubmitPort(value)
    }
}

/// A single CBOR decoder failure as a human-readable string.
///
/// Upstream uses `Cardano.Binary.DecoderError`; its Aeson `ToJSON`
/// instance is `Aeson.String . textShow`. The Rust newtype keeps the
/// string-form invariant by construction so both `Display` and
/// `Serialize` produce the same byte-equivalent output.
///
/// R340 may replace the inner string with a structured CBOR error type
/// once the web round wires `minicbor::decode::Error` through.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DecoderError(pub String);

impl fmt::Display for DecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for DecoderError {
    fn from(value: String) -> Self {
        DecoderError(value)
    }
}

impl From<&str> for DecoderError {
    fn from(value: &str) -> Self {
        DecoderError(value.to_string())
    }
}

/// Errors returned by raw CBOR transaction parsing.
///
/// Upstream: `newtype RawCborDecodeError = RawCborDecodeError [DecoderError]`.
/// Aeson default Generic-derived `ToJSON` for a newtype unwraps to the
/// inner value; `#[serde(transparent)]` matches that shape.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RawCborDecodeError(pub Vec<DecoderError>);

impl fmt::Display for RawCborDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RawCborDecodeError decode error: ")?;
        for (i, err) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            err.fmt(f)?;
        }
        Ok(())
    }
}

impl std::error::Error for RawCborDecodeError {}

/// Socket-environment-variable lookup error.
///
/// Upstream: `newtype EnvSocketError = CliEnvVarLookup Text`. Upstream's
/// manual `ToJSON` instance produces the bare object `{"message":"..."}`
/// without a constructor tag (the `deriving anyclass` form is bypassed).
/// `#[serde(untagged)]` + struct-variant matches that shape.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum EnvSocketError {
    /// Lookup of `CARDANO_NODE_SOCKET_PATH` (or equivalent) failed.
    CliEnvVarLookup {
        /// Operator-facing failure message.
        message: String,
    },
}

impl fmt::Display for EnvSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnvSocketError::CliEnvVarLookup { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for EnvSocketError {}

/// Transaction-submission command error.
///
/// Upstream: `data TxCmdError = TxCmdSocketEnvError ... | TxCmdTxReadError ... | TxCmdTxSubmitValidationError ... | TxCmdTxSubmitConnectionError ...`
/// with `deriving anyclass instance ToJSON`. Aeson's default `ToJSON` for
/// a Generic-derived sum uses `TaggedObject "tag" "contents"` (Aeson 1.x+
/// default), producing `{"tag":"<Constructor>","contents":<payload>}`.
/// `#[serde(tag = "tag", content = "contents")]` matches it.
///
/// `TxCmdTxSubmitValidationError` now carries a [`TxSubmitValidationError`]
/// which preserves both the raw CBOR-encoded era-specific `ApplyTxError`
/// payload AND a human-readable rendering. The inner type's custom
/// `Serialize` keeps the upstream JSON wire shape (`{"contents":"<rendered>"}`)
/// byte-equivalent — only the rendered string surfaces in the JSON
/// envelope. Operators that want the structured payload reach for
/// `TxSubmitValidationError::raw_cbor()`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "tag", content = "contents")]
pub enum TxCmdError {
    /// Failure to look up `CARDANO_NODE_SOCKET_PATH` (or equivalent) in
    /// the process environment.
    TxCmdSocketEnvError(EnvSocketError),
    /// Raw CBOR decoder failure(s) when parsing transaction bytes.
    TxCmdTxReadError(RawCborDecodeError),
    /// Tx-validation rejection from the local cardano-node. Carries the
    /// raw CBOR-encoded era-specific reject payload + a string
    /// rendering; JSON serialisation emits only the rendering to keep
    /// upstream-byte-equivalence.
    TxCmdTxSubmitValidationError(TxSubmitValidationError),
    /// Connection to the local cardano-node socket failed.
    TxCmdTxSubmitConnectionError(String),
}

/// Structured transaction-validation rejection from the local node.
///
/// Carries both the raw CBOR-encoded era-specific `ApplyTxError`
/// payload (so future structured-decoder work can pattern-match on
/// individual variants like `FeeTooSmall` / `ValueNotConservedUTxO`
/// without re-fetching the rejection) AND a string rendering used
/// today's operator-facing output.
///
/// The custom `Serialize` impl emits only the rendered string so the
/// upstream JSON wire shape stays byte-equivalent:
/// `{"tag":"TxCmdTxSubmitValidationError","contents":"<rendered>"}`.
///
/// Upstream parallel: `Cardano.TxSubmit.Types.TxValidationErrorInCardanoMode`.
/// Yggdrasil's variant is era-opaque at the Rust-type level pending
/// the multi-era `ApplyTxError` decoder; see
/// `docs/TECH-DEBT.md` "cardano-submit-api validation error" for the
/// per-era structured-decoder roadmap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TxSubmitValidationError {
    /// Raw CBOR-encoded era-specific `ApplyTxError` payload as
    /// received from the local-tx-submission server.
    raw_cbor: Vec<u8>,
    /// Human-readable rendering — used by `Display` impls and JSON
    /// `contents` field.
    rendered: String,
}

impl TxSubmitValidationError {
    /// Construct from raw CBOR + a pre-rendered string. The renderer is
    /// typically the same one used by upstream's
    /// `renderTxValidationErrorInCardanoMode`; today the Rust side
    /// passes through whatever string the LSQ surface produced.
    pub fn new(raw_cbor: Vec<u8>, rendered: impl Into<String>) -> Self {
        Self {
            raw_cbor,
            rendered: rendered.into(),
        }
    }

    /// Construct from a string only — the raw CBOR slot is left empty.
    /// Used by call sites that built the error from a string before
    /// the raw bytes were threaded through; eligible for follow-on
    /// replacement once the LocalTxSubmission client exposes the raw
    /// reject payload alongside the rendered form.
    pub fn from_rendered(rendered: impl Into<String>) -> Self {
        Self {
            raw_cbor: Vec::new(),
            rendered: rendered.into(),
        }
    }

    /// Raw CBOR-encoded `ApplyTxError` bytes. Empty when the value was
    /// constructed via [`Self::from_rendered`].
    pub fn raw_cbor(&self) -> &[u8] {
        &self.raw_cbor
    }

    /// Human-readable rendering, suitable for stderr / JSON output.
    pub fn rendered(&self) -> &str {
        &self.rendered
    }
}

impl fmt::Display for TxSubmitValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.rendered)
    }
}

impl TxSubmitValidationError {
    /// Produce the upstream era-tagged `TxValidationErrorInCardanoMode`
    /// view of this rejection. The era is supplied by the caller (today
    /// the LocalTxSubmission client knows which era it was submitted
    /// against); the typed view shares the same raw CBOR and rendered
    /// string. Per-variant CBOR decoders are layered on top in follow-on
    /// rounds (Phase 2.5+ of the A5 plan).
    pub fn into_typed(self, era: TxValidationEra) -> TxValidationErrorInCardanoMode {
        let payload = EraApplyTxError {
            raw_cbor: self.raw_cbor,
            rendered: self.rendered,
        };
        TxValidationErrorInCardanoMode::from_raw(era, payload)
    }
}

/// Era discriminator for a `TxValidationErrorInCardanoMode` rejection.
///
/// Mirrors upstream `Cardano.Api.Eon.ShelleyBasedEra` — the era tag
/// that selects the appropriate `ApplyTxError <era>` newtype.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum TxValidationEra {
    /// Shelley-era tx rejection: `ShelleyApplyTxError (NonEmpty (ShelleyLedgerPredFailure ShelleyEra))`.
    Shelley,
    /// Allegra-era tx rejection.
    Allegra,
    /// Mary-era tx rejection.
    Mary,
    /// Alonzo-era tx rejection: `AlonzoApplyTxError (NonEmpty (ShelleyLedgerPredFailure AlonzoEra))`.
    Alonzo,
    /// Babbage-era tx rejection: `BabbageApplyTxError (NonEmpty (ShelleyLedgerPredFailure BabbageEra))`.
    Babbage,
    /// Conway-era tx rejection: `ConwayApplyTxError (NonEmpty (ConwayLedgerPredFailure ConwayEra))`.
    Conway,
}

impl TxValidationEra {
    /// Upstream constructor name (`<Era>ApplyTxError`) used in the
    /// stock-derived `Show (ApplyTxError <era>)` rendering. Useful for
    /// constructing typed rejection strings.
    pub fn apply_tx_error_constructor(self) -> &'static str {
        match self {
            Self::Shelley => "ShelleyApplyTxError",
            Self::Allegra => "AllegraApplyTxError",
            Self::Mary => "MaryApplyTxError",
            Self::Alonzo => "AlonzoApplyTxError",
            Self::Babbage => "BabbageApplyTxError",
            Self::Conway => "ConwayApplyTxError",
        }
    }
}

/// Era-specific `ApplyTxError` payload — currently raw CBOR + rendered
/// text, with per-variant CBOR decoders layered in follow-on rounds.
///
/// Mirrors upstream `newtype ApplyTxError <era> = <Era>ApplyTxError
/// (NonEmpty (<Era>LedgerPredFailure <era>))` — yggdrasil collapses the
/// `NonEmpty (PredicateFailure)` into raw CBOR bytes for now, with the
/// rendered string preserving the upstream operator-facing output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EraApplyTxError {
    raw_cbor: Vec<u8>,
    rendered: String,
}

impl EraApplyTxError {
    /// Construct from raw CBOR + a pre-rendered string. The rendered
    /// form is what gets surfaced through `Display` until per-variant
    /// decoders ship.
    pub fn new(raw_cbor: Vec<u8>, rendered: impl Into<String>) -> Self {
        Self {
            raw_cbor,
            rendered: rendered.into(),
        }
    }

    /// Raw CBOR-encoded era-specific `ApplyTxError` bytes.
    pub fn raw_cbor(&self) -> &[u8] {
        &self.raw_cbor
    }

    /// Human-readable rendering — pre-rendered upstream until per-variant
    /// decoders ship.
    pub fn rendered(&self) -> &str {
        &self.rendered
    }
}

impl fmt::Display for EraApplyTxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.rendered)
    }
}

/// Era-tagged transaction-validation rejection mirroring upstream
/// `Cardano.Api.TxValidationErrorInCardanoMode`.
///
/// Each variant carries the era-specific `ApplyTxError <era>` payload
/// (`EraApplyTxError`) — currently raw CBOR + rendered text. Follow-on
/// rounds (A5 Phase 2.5+) will replace `EraApplyTxError`'s flat
/// raw-bytes carrier with the full per-era predicate-failure sum types.
///
/// Operators that need the typed era discriminant reach for
/// `TxSubmitValidationError::into_typed(era)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxValidationErrorInCardanoMode {
    /// Shelley-era validation error.
    Shelley(EraApplyTxError),
    /// Allegra-era validation error.
    Allegra(EraApplyTxError),
    /// Mary-era validation error.
    Mary(EraApplyTxError),
    /// Alonzo-era validation error.
    Alonzo(EraApplyTxError),
    /// Babbage-era validation error.
    Babbage(EraApplyTxError),
    /// Conway-era validation error.
    Conway(EraApplyTxError),
}

impl TxValidationErrorInCardanoMode {
    /// Wrap an `EraApplyTxError` payload under the appropriate era
    /// variant. Used by `TxSubmitValidationError::into_typed`.
    pub fn from_raw(era: TxValidationEra, payload: EraApplyTxError) -> Self {
        match era {
            TxValidationEra::Shelley => Self::Shelley(payload),
            TxValidationEra::Allegra => Self::Allegra(payload),
            TxValidationEra::Mary => Self::Mary(payload),
            TxValidationEra::Alonzo => Self::Alonzo(payload),
            TxValidationEra::Babbage => Self::Babbage(payload),
            TxValidationEra::Conway => Self::Conway(payload),
        }
    }

    /// Return the era discriminator.
    pub fn era(&self) -> TxValidationEra {
        match self {
            Self::Shelley(_) => TxValidationEra::Shelley,
            Self::Allegra(_) => TxValidationEra::Allegra,
            Self::Mary(_) => TxValidationEra::Mary,
            Self::Alonzo(_) => TxValidationEra::Alonzo,
            Self::Babbage(_) => TxValidationEra::Babbage,
            Self::Conway(_) => TxValidationEra::Conway,
        }
    }

    /// Return the era-specific payload.
    pub fn payload(&self) -> &EraApplyTxError {
        match self {
            Self::Shelley(p)
            | Self::Allegra(p)
            | Self::Mary(p)
            | Self::Alonzo(p)
            | Self::Babbage(p)
            | Self::Conway(p) => p,
        }
    }
}

impl fmt::Display for TxValidationErrorInCardanoMode {
    /// Render upstream `Show (TxValidationErrorInCardanoMode)`:
    /// `<Era>ApplyTxError (<rendered>)` — matching upstream's
    /// stock-derived Show that wraps each per-era payload in its
    /// `<Era>ApplyTxError` constructor.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({})",
            self.era().apply_tx_error_constructor(),
            self.payload().rendered()
        )
    }
}

/// Top-level Shelley LEDGER predicate-failure variants.
///
/// Mirrors upstream `data ShelleyLedgerPredFailure era` from
/// `Cardano.Ledger.Shelley.Rules.Ledger`:
///
/// ```text
/// data ShelleyLedgerPredFailure era
///   = UtxowFailure (PredicateFailure (EraRule "UTXOW" era))
///   | DelegsFailure (PredicateFailure (EraRule "DELEGS" era))
///   | ShelleyWithdrawalsMissingAccounts Withdrawals
///   | ShelleyIncompleteWithdrawals (NonEmptyMap AccountAddress (Mismatch RelEQ Coin))
/// ```
///
/// Each variant currently carries raw CBOR bytes; per-rule typed
/// payloads (UTXOW + DELEGS sub-trees, Withdrawals map decoding,
/// NonEmptyMap of Mismatch values) land in follow-on rounds.
///
/// The variant discriminator matches upstream's CBOR encoding tag
/// (Word8 0/1/2/3) at index 0 of the outer 2-element array.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShelleyLedgerPredFailure {
    /// UTXOW sub-rule failure (CBOR tag 0). Payload is a
    /// `ShelleyUtxowPredFailure era` (one of ~10 variants including
    /// `InvalidWitnessesUTXOW`, `MissingVKeyWitnessesUTXOW`, etc.).
    /// Typed sub-decoder pending — payload is the raw inner CBOR.
    UtxowFailure(Vec<u8>),
    /// DELEGS sub-rule failure (CBOR tag 1). Payload is a
    /// `ShelleyDelegsPredFailure era` (one of ~3 variants delegating
    /// further into DELPL/POOL/DELEG sub-rules). Typed sub-decoder
    /// pending — payload is the raw inner CBOR.
    DelegsFailure(Vec<u8>),
    /// Withdrawals refer to accounts that are not in the reward map
    /// (CBOR tag 2). Payload is `Withdrawals = Map AccountAddress
    /// Coin` (R596 typed decoder).
    ShelleyWithdrawalsMissingAccounts(Withdrawals),
    /// Withdrawals do not fully exhaust the named accounts' reward
    /// balances (CBOR tag 3). Payload is `NonEmptyMap AccountAddress
    /// (Mismatch RelEQ Coin)` (R597 typed decoder via
    /// [`IncompleteWithdrawals`]).
    ShelleyIncompleteWithdrawals(IncompleteWithdrawals),
}

impl ShelleyLedgerPredFailure {
    /// Return the upstream CBOR-encoding tag for this variant
    /// (matches `Cardano.Ledger.Shelley.Rules.Ledger.encCBOR`).
    pub fn tag(&self) -> u8 {
        match self {
            Self::UtxowFailure(_) => 0,
            Self::DelegsFailure(_) => 1,
            Self::ShelleyWithdrawalsMissingAccounts(_) => 2,
            Self::ShelleyIncompleteWithdrawals(_) => 3,
        }
    }

    /// Return the upstream constructor name for stock-derived Show.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::UtxowFailure(_) => "UtxowFailure",
            Self::DelegsFailure(_) => "DelegsFailure",
            Self::ShelleyWithdrawalsMissingAccounts(_) => "ShelleyWithdrawalsMissingAccounts",
            Self::ShelleyIncompleteWithdrawals(_) => "ShelleyIncompleteWithdrawals",
        }
    }
}

impl fmt::Display for ShelleyLedgerPredFailure {
    /// Render upstream stock-derived `Show (ShelleyLedgerPredFailure
    /// era)`: `<Constructor> <payload>`. UTXOW + DELEGS sub-rule
    /// payloads remain raw-cbor until per-rule decoders ship; the
    /// withdrawal-related payloads (R596 + R597) render through their
    /// typed Display.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UtxowFailure(b) | Self::DelegsFailure(b) => {
                write!(f, "{} <raw-cbor {} bytes>", self.constructor(), b.len())
            }
            Self::ShelleyWithdrawalsMissingAccounts(w) => {
                write!(f, "ShelleyWithdrawalsMissingAccounts ({w})")
            }
            Self::ShelleyIncompleteWithdrawals(iw) => {
                write!(f, "ShelleyIncompleteWithdrawals (fromList [{iw}])")
            }
        }
    }
}

/// Mismatch between a supplied and expected value, parametric on the
/// relation tag (`RelEQ`, `RelLTEQ`, `RelGTEQ`, `RelSubset`).
///
/// Mirrors upstream `data Mismatch (r :: Relation) a = Mismatch
/// {mismatchSupplied :: !a, mismatchExpected :: !a}` from
/// `Cardano.Ledger.BaseTypes`. CBOR encoding is a 2-element record
/// `[supplied, expected]`. The Show is custom and produces:
/// `Mismatch (<RelationTag>) {supplied: <a>, expected: <a>}`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mismatch<T> {
    /// Relation tag (RelEQ / RelLTEQ / RelGTEQ / RelSubset) — used as
    /// the typeRep label in upstream's custom Show.
    pub relation: MismatchRelation,
    /// Operator-supplied value.
    pub supplied: T,
    /// Ledger-expected value.
    pub expected: T,
}

/// Upstream `Relation` kind for `Mismatch`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum MismatchRelation {
    /// Supplied is required to equal expected.
    RelEQ,
    /// Supplied is required to be ≤ expected.
    RelLTEQ,
    /// Supplied is required to be ≥ expected.
    RelGTEQ,
    /// Supplied is required to be a subset of expected.
    RelSubset,
}

impl MismatchRelation {
    /// Upstream `typeRep`-derived name used in the custom
    /// `Show (Mismatch r a)` header line.
    pub fn type_rep(self) -> &'static str {
        match self {
            Self::RelEQ => "RelEQ",
            Self::RelLTEQ => "RelLTEQ",
            Self::RelGTEQ => "RelGTEQ",
            Self::RelSubset => "RelSubset",
        }
    }
}

impl<T: fmt::Display> fmt::Display for Mismatch<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Mismatch ({}) {{supplied: {}, expected: {}}}",
            self.relation.type_rep(),
            self.supplied,
            self.expected
        )
    }
}

/// Coin amount renderer matching upstream `Show Coin`: `Coin <n>`
/// (Quiet-derived: keeps the constructor name, suppresses the field
/// record syntax).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct CoinShow(pub u64);

impl fmt::Display for CoinShow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Coin {}", self.0)
    }
}

/// Typed payload for
/// `ShelleyLedgerPredFailure::ShelleyIncompleteWithdrawals`.
///
/// Mirrors upstream `NonEmptyMap AccountAddress (Mismatch RelEQ
/// Coin)` — a map (with at least one entry) from reward account to a
/// supplied-vs-expected coin mismatch. Yggdrasil stores it as a
/// `BTreeMap<RewardAccount, Mismatch<u64>>` and rejects empty maps at
/// decode time to honour the NonEmpty invariant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncompleteWithdrawals {
    /// Map of reward-account → coin-mismatch pair. Guaranteed
    /// non-empty by `from_cbor`.
    pub entries: std::collections::BTreeMap<yggdrasil_ledger::RewardAccount, Mismatch<u64>>,
}

impl IncompleteWithdrawals {
    /// Decode `NonEmptyMap AccountAddress (Mismatch RelEQ Coin)` from
    /// the canonical CBOR shape. The inner Mismatch is encoded as a
    /// 2-element array `[supplied, expected]` per upstream
    /// `EncCBORGroup (Mismatch r a)`.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let count = dec.map().map_err(|err| {
            DecoderError(format!("IncompleteWithdrawals: expected CBOR map: {err:?}"))
        })?;
        if count == 0 {
            return Err(DecoderError(
                "IncompleteWithdrawals: NonEmptyMap requires at least one entry".to_string(),
            ));
        }
        let mut entries = std::collections::BTreeMap::new();
        for _ in 0..count {
            let key_bytes = dec.bytes().map_err(|err| {
                DecoderError(format!(
                    "IncompleteWithdrawals: expected map key bytes: {err:?}"
                ))
            })?;
            let account =
                yggdrasil_ledger::RewardAccount::from_bytes(key_bytes).ok_or_else(|| {
                    DecoderError(format!(
                        "IncompleteWithdrawals: invalid reward-account key ({} bytes)",
                        key_bytes.len()
                    ))
                })?;
            let len = dec.array().map_err(|err| {
                DecoderError(format!(
                    "IncompleteWithdrawals: expected Mismatch 2-array: {err:?}"
                ))
            })?;
            if len != 2 {
                return Err(DecoderError(format!(
                    "IncompleteWithdrawals: expected Mismatch 2-array, got len {len}"
                )));
            }
            let supplied = dec.unsigned().map_err(|err| {
                DecoderError(format!(
                    "IncompleteWithdrawals: expected supplied coin: {err:?}"
                ))
            })?;
            let expected = dec.unsigned().map_err(|err| {
                DecoderError(format!(
                    "IncompleteWithdrawals: expected expected coin: {err:?}"
                ))
            })?;
            entries.insert(
                account,
                Mismatch {
                    relation: MismatchRelation::RelEQ,
                    supplied,
                    expected,
                },
            );
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for IncompleteWithdrawals {
    /// Render the inner `fromList [(<k>, <v>), ...]` list — the outer
    /// `ShelleyIncompleteWithdrawals (fromList [...])` envelope is
    /// added by `ShelleyLedgerPredFailure::Display`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (account, mismatch) in &self.entries {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            let network = match account.network {
                0 => "Testnet",
                1 => "Mainnet",
                _ => "Unknown",
            };
            let inner = match account.credential {
                yggdrasil_ledger::StakeCredential::AddrKeyHash(h) => {
                    format!(
                        "KeyHashObj (KeyHash {{unKeyHash = \"{}\"}})",
                        hex::encode(h)
                    )
                }
                yggdrasil_ledger::StakeCredential::ScriptHash(h) => {
                    format!("ScriptHashObj (ScriptHash \"{}\")", hex::encode(h))
                }
            };
            let typed_mismatch = Mismatch {
                relation: mismatch.relation,
                supplied: CoinShow(mismatch.supplied),
                expected: CoinShow(mismatch.expected),
            };
            write!(
                f,
                "(AccountAddress {{aaNetworkId = {network}, aaId = {inner}}},{typed_mismatch})"
            )?;
        }
        Ok(())
    }
}

/// Typed payload for `ShelleyLedgerPredFailure::ShelleyWithdrawalsMissingAccounts`.
///
/// Mirrors upstream `Withdrawals = Map AccountAddress Coin` from
/// `Cardano.Ledger.Address`. The CBOR encoding is a single map where
/// keys are 29-byte reward-account address bytes and values are
/// non-negative coin amounts.
///
/// Yggdrasil reuses the existing `RewardAccount` codec for keys; the
/// map is stored as `BTreeMap<RewardAccount, u64>` so its iteration
/// order matches upstream `Data.Map.toAscList` byte-lex sort.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct Withdrawals {
    /// Map of reward-account address → withdrawal amount (lovelace).
    pub entries: std::collections::BTreeMap<yggdrasil_ledger::RewardAccount, u64>,
}

impl Withdrawals {
    /// Decode `Withdrawals` from canonical Shelley-era CBOR bytes.
    /// Returns the parsed map alongside the raw bytes for callers
    /// that want to keep both views.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let count = dec
            .map()
            .map_err(|err| DecoderError(format!("Withdrawals: expected CBOR map: {err:?}")))?;
        let mut entries = std::collections::BTreeMap::new();
        for _ in 0..count {
            let key_bytes = dec.bytes().map_err(|err| {
                DecoderError(format!("Withdrawals: expected map key bytes: {err:?}"))
            })?;
            let account =
                yggdrasil_ledger::RewardAccount::from_bytes(key_bytes).ok_or_else(|| {
                    DecoderError(format!(
                        "Withdrawals: invalid reward-account key ({} bytes)",
                        key_bytes.len()
                    ))
                })?;
            let coin = dec.unsigned().map_err(|err| {
                DecoderError(format!("Withdrawals: expected coin value: {err:?}"))
            })?;
            entries.insert(account, coin);
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for Withdrawals {
    /// Render upstream `Show Withdrawals`: `Withdrawals {unWithdrawals
    /// = fromList [(AccountAddress {...}, Coin <n>),...]}`. The map
    /// entries iterate in `BTreeMap` byte-lex order matching upstream
    /// `Data.Map.toAscList`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Withdrawals {unWithdrawals = fromList [")?;
        let mut first = true;
        for (account, coin) in &self.entries {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            let network = match account.network {
                0 => "Testnet",
                1 => "Mainnet",
                _ => "Unknown",
            };
            let inner = match account.credential {
                yggdrasil_ledger::StakeCredential::AddrKeyHash(h) => {
                    format!(
                        "KeyHashObj (KeyHash {{unKeyHash = \"{}\"}})",
                        hex::encode(h)
                    )
                }
                yggdrasil_ledger::StakeCredential::ScriptHash(h) => {
                    format!("ScriptHashObj (ScriptHash \"{}\")", hex::encode(h))
                }
            };
            write!(
                f,
                "(AccountAddress {{aaNetworkId = {network}, aaId = {inner}}},Coin {coin})"
            )?;
        }
        f.write_str("]}")?;
        Ok(())
    }
}

/// Custom `Serialize` that emits ONLY the rendered string so the
/// upstream JSON `{"contents":"<rendered>"}` wire shape stays
/// byte-equivalent. The raw CBOR bytes are deliberately not
/// surfaced through JSON — operators that need them reach through
/// the Rust API.
impl Serialize for TxSubmitValidationError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.rendered)
    }
}

impl fmt::Display for TxCmdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&render_tx_cmd_error(self))
    }
}

impl std::error::Error for TxCmdError {}

/// Web-API-surface error returned to clients of `POST /api/submit/tx`.
///
/// Upstream: `data TxSubmitWebApiError = TxSubmitDecodeHex | TxSubmitEmpty | TxSubmitDecodeFail !DecoderError | TxSubmitBadTx !Text | TxSubmitFail TxCmdError`
/// with a hand-written `toJSON` instance using explicit tag/contents.
/// `#[serde(tag = "tag", content = "contents")]` matches it for both
/// unit variants (no `contents` field) and payload variants.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "tag", content = "contents")]
pub enum TxSubmitWebApiError {
    /// Hex decoding of the request body failed.
    TxSubmitDecodeHex,
    /// Request body was empty.
    TxSubmitEmpty,
    /// CBOR decoder failed on the (post-hex) tx bytes.
    TxSubmitDecodeFail(DecoderError),
    /// Tx semantic-content rejection (caller-friendly string).
    TxSubmitBadTx(String),
    /// Underlying tx-cmd error during submission to cardano-node.
    TxSubmitFail(TxCmdError),
}

impl fmt::Display for TxSubmitWebApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TxSubmitWebApiError::TxSubmitDecodeHex => f.write_str("TxSubmitDecodeHex"),
            TxSubmitWebApiError::TxSubmitEmpty => f.write_str("TxSubmitEmpty"),
            TxSubmitWebApiError::TxSubmitDecodeFail(err) => {
                write!(f, "TxSubmitDecodeFail: {err}")
            }
            TxSubmitWebApiError::TxSubmitBadTx(msg) => write!(f, "TxSubmitBadTx: {msg}"),
            TxSubmitWebApiError::TxSubmitFail(err) => write!(f, "TxSubmitFail: {err}"),
        }
    }
}

impl std::error::Error for TxSubmitWebApiError {}

/// Render a `TxCmdError` as a human-readable line.
///
/// Mirrors upstream `Cardano.TxSubmit.Types.renderTxCmdError` byte-for-byte
/// (modulo the validation-string formatting, which currently uses the
/// pre-rendered string instead of the live `TxValidationErrorInCardanoMode`
/// pattern match — see `TxCmdTxSubmitValidationError` doc).
pub fn render_tx_cmd_error(err: &TxCmdError) -> String {
    match err {
        TxCmdError::TxCmdSocketEnvError(socket_error) => {
            format!("socket env error \"{socket_error}\"")
        }
        TxCmdError::TxCmdTxReadError(envelope_error) => {
            format!("transaction read error \"{envelope_error}\"")
        }
        TxCmdError::TxCmdTxSubmitValidationError(validation_error) => {
            format!("transaction submit error {}", validation_error.rendered())
        }
        TxCmdError::TxCmdTxSubmitConnectionError(msg) => {
            format!("transaction submit connection error: {msg}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(value: impl Serialize) -> String {
        serde_json::to_string(&value).expect("serializes")
    }

    #[test]
    fn tx_submit_port_serializes_transparently() {
        assert_eq!(json(TxSubmitPort(8090)), "8090");
    }

    #[test]
    fn tx_submit_port_round_trips_via_u16() {
        let port = TxSubmitPort::from(8090u16);
        assert_eq!(u16::from(port), 8090);
    }

    #[test]
    fn decoder_error_serializes_as_string() {
        assert_eq!(
            json(DecoderError("invalid CBOR tag 99".to_string())),
            "\"invalid CBOR tag 99\""
        );
    }

    #[test]
    fn raw_cbor_decode_error_serializes_as_array() {
        let err = RawCborDecodeError(vec![
            DecoderError("a".to_string()),
            DecoderError("b".to_string()),
        ]);
        assert_eq!(json(&err), r#"["a","b"]"#);
    }

    #[test]
    fn raw_cbor_decode_error_display_format() {
        let err = RawCborDecodeError(vec![
            DecoderError("a".to_string()),
            DecoderError("b".to_string()),
        ]);
        assert_eq!(err.to_string(), "RawCborDecodeError decode error: a, b");
    }

    #[test]
    fn env_socket_error_serializes_as_bare_object() {
        let err = EnvSocketError::CliEnvVarLookup {
            message: "CARDANO_NODE_SOCKET_PATH not set".to_string(),
        };
        assert_eq!(
            json(&err),
            r#"{"message":"CARDANO_NODE_SOCKET_PATH not set"}"#
        );
    }

    #[test]
    fn env_socket_error_display_uses_message() {
        let err = EnvSocketError::CliEnvVarLookup {
            message: "missing var".to_string(),
        };
        assert_eq!(err.to_string(), "missing var");
    }

    #[test]
    fn tx_cmd_socket_env_error_json_shape() {
        let err = TxCmdError::TxCmdSocketEnvError(EnvSocketError::CliEnvVarLookup {
            message: "x".to_string(),
        });
        assert_eq!(
            json(&err),
            r#"{"tag":"TxCmdSocketEnvError","contents":{"message":"x"}}"#
        );
    }

    #[test]
    fn tx_cmd_tx_read_error_json_shape() {
        let err =
            TxCmdError::TxCmdTxReadError(RawCborDecodeError(vec![DecoderError("bad".to_string())]));
        assert_eq!(
            json(&err),
            r#"{"tag":"TxCmdTxReadError","contents":["bad"]}"#
        );
    }

    #[test]
    fn tx_cmd_validation_error_json_shape() {
        let err = TxCmdError::TxCmdTxSubmitValidationError(TxSubmitValidationError::from_rendered(
            "FeeTooSmall",
        ));
        // Wire shape stays byte-equivalent to upstream's
        // `{"tag":"...","contents":"<rendered>"}`; the raw_cbor field
        // is hidden by the custom Serialize impl on
        // TxSubmitValidationError.
        assert_eq!(
            json(&err),
            r#"{"tag":"TxCmdTxSubmitValidationError","contents":"FeeTooSmall"}"#
        );
    }

    /// Same JSON shape when the value carries non-empty raw_cbor —
    /// the bytes must not leak into the JSON envelope.
    #[test]
    fn tx_cmd_validation_error_json_shape_hides_raw_cbor() {
        let err = TxCmdError::TxCmdTxSubmitValidationError(TxSubmitValidationError::new(
            vec![0xDE, 0xAD, 0xBE, 0xEF],
            "FeeTooSmall",
        ));
        assert_eq!(
            json(&err),
            r#"{"tag":"TxCmdTxSubmitValidationError","contents":"FeeTooSmall"}"#
        );
    }

    /// Raw bytes survive through the Rust API even though they don't
    /// surface in JSON — operators that want the structured form can
    /// recover them.
    #[test]
    fn tx_submit_validation_error_preserves_raw_cbor() {
        let bytes = vec![0x82, 0x01, 0x82, 0x05, 0x82, 0xFE, 0xFD];
        let err = TxSubmitValidationError::new(bytes.clone(), "ValueNotConservedUTxO");
        assert_eq!(err.raw_cbor(), bytes.as_slice());
        assert_eq!(err.rendered(), "ValueNotConservedUTxO");
    }

    /// `from_rendered` leaves raw_cbor empty.
    #[test]
    fn tx_submit_validation_error_from_rendered_has_empty_raw_cbor() {
        let err = TxSubmitValidationError::from_rendered("OutsideValidityIntervalUTxO");
        assert!(err.raw_cbor().is_empty());
        assert_eq!(err.rendered(), "OutsideValidityIntervalUTxO");
    }

    #[test]
    fn tx_cmd_connection_error_json_shape() {
        let err = TxCmdError::TxCmdTxSubmitConnectionError("ECONNREFUSED".to_string());
        assert_eq!(
            json(&err),
            r#"{"tag":"TxCmdTxSubmitConnectionError","contents":"ECONNREFUSED"}"#
        );
    }

    #[test]
    fn web_api_error_decode_hex_json_shape() {
        let err = TxSubmitWebApiError::TxSubmitDecodeHex;
        assert_eq!(json(&err), r#"{"tag":"TxSubmitDecodeHex"}"#);
    }

    #[test]
    fn web_api_error_empty_json_shape() {
        let err = TxSubmitWebApiError::TxSubmitEmpty;
        assert_eq!(json(&err), r#"{"tag":"TxSubmitEmpty"}"#);
    }

    #[test]
    fn web_api_error_decode_fail_json_shape() {
        let err = TxSubmitWebApiError::TxSubmitDecodeFail(DecoderError("trunc".to_string()));
        assert_eq!(
            json(&err),
            r#"{"tag":"TxSubmitDecodeFail","contents":"trunc"}"#
        );
    }

    #[test]
    fn web_api_error_bad_tx_json_shape() {
        let err = TxSubmitWebApiError::TxSubmitBadTx("over budget".to_string());
        assert_eq!(
            json(&err),
            r#"{"tag":"TxSubmitBadTx","contents":"over budget"}"#
        );
    }

    #[test]
    fn web_api_error_fail_json_shape() {
        let err = TxSubmitWebApiError::TxSubmitFail(TxCmdError::TxCmdTxSubmitConnectionError(
            "down".to_string(),
        ));
        assert_eq!(
            json(&err),
            r#"{"tag":"TxSubmitFail","contents":{"tag":"TxCmdTxSubmitConnectionError","contents":"down"}}"#
        );
    }

    #[test]
    fn render_tx_cmd_error_socket_env() {
        let err = TxCmdError::TxCmdSocketEnvError(EnvSocketError::CliEnvVarLookup {
            message: "missing".to_string(),
        });
        assert_eq!(render_tx_cmd_error(&err), "socket env error \"missing\"");
    }

    #[test]
    fn render_tx_cmd_error_read() {
        let err =
            TxCmdError::TxCmdTxReadError(RawCborDecodeError(vec![DecoderError("x".to_string())]));
        assert!(render_tx_cmd_error(&err).starts_with("transaction read error"));
    }

    #[test]
    fn render_tx_cmd_error_validation() {
        let err = TxCmdError::TxCmdTxSubmitValidationError(TxSubmitValidationError::from_rendered(
            "FeeTooSmall",
        ));
        assert_eq!(
            render_tx_cmd_error(&err),
            "transaction submit error FeeTooSmall"
        );
    }

    #[test]
    fn render_tx_cmd_error_connection() {
        let err = TxCmdError::TxCmdTxSubmitConnectionError("down".to_string());
        assert_eq!(
            render_tx_cmd_error(&err),
            "transaction submit connection error: down"
        );
    }

    #[test]
    fn tx_cmd_error_implements_std_error() {
        fn assert_error<E: std::error::Error>(_: &E) {}
        let err = TxCmdError::TxCmdTxSubmitConnectionError("x".to_string());
        assert_error(&err);
    }

    #[test]
    fn web_api_error_implements_std_error() {
        fn assert_error<E: std::error::Error>(_: &E) {}
        let err = TxSubmitWebApiError::TxSubmitEmpty;
        assert_error(&err);
    }

    #[test]
    fn tx_validation_era_constructor_names() {
        assert_eq!(
            TxValidationEra::Shelley.apply_tx_error_constructor(),
            "ShelleyApplyTxError"
        );
        assert_eq!(
            TxValidationEra::Allegra.apply_tx_error_constructor(),
            "AllegraApplyTxError"
        );
        assert_eq!(
            TxValidationEra::Mary.apply_tx_error_constructor(),
            "MaryApplyTxError"
        );
        assert_eq!(
            TxValidationEra::Alonzo.apply_tx_error_constructor(),
            "AlonzoApplyTxError"
        );
        assert_eq!(
            TxValidationEra::Babbage.apply_tx_error_constructor(),
            "BabbageApplyTxError"
        );
        assert_eq!(
            TxValidationEra::Conway.apply_tx_error_constructor(),
            "ConwayApplyTxError"
        );
    }

    #[test]
    fn tx_validation_error_in_cardano_mode_from_raw_preserves_era_and_payload() {
        let payload = EraApplyTxError::new(vec![0xDE, 0xAD], "fee too small");
        let err =
            TxValidationErrorInCardanoMode::from_raw(TxValidationEra::Conway, payload.clone());
        assert_eq!(err.era(), TxValidationEra::Conway);
        assert_eq!(err.payload(), &payload);
    }

    #[test]
    fn tx_validation_error_in_cardano_mode_display_wraps_in_constructor() {
        let payload =
            EraApplyTxError::new(vec![], "FeeTooSmall {expected = 200000, actual = 99000}");
        let err = TxValidationErrorInCardanoMode::from_raw(TxValidationEra::Babbage, payload);
        assert_eq!(
            err.to_string(),
            "BabbageApplyTxError (FeeTooSmall {expected = 200000, actual = 99000})"
        );
    }

    fn empty_withdrawals_payload() -> Withdrawals {
        Withdrawals::from_cbor(&[0xa0]).expect("empty withdrawals")
    }

    fn one_entry_incomplete_withdrawals_payload() -> IncompleteWithdrawals {
        // 1-entry map; 29-byte mainnet key-hash account; mismatch
        // [supplied=100, expected=200].
        let mut cbor = vec![0xa1_u8];
        cbor.push(0x58);
        cbor.push(29);
        cbor.push(0xE1);
        cbor.extend_from_slice(&[0x22_u8; 28]);
        // 2-array mismatch
        cbor.push(0x82);
        cbor.extend_from_slice(&[0x18, 0x64]); // 100
        cbor.extend_from_slice(&[0x18, 0xC8]); // 200
        IncompleteWithdrawals::from_cbor(&cbor).expect("one-entry mismatch")
    }

    #[test]
    fn shelley_ledger_pred_failure_tag_dispatch() {
        assert_eq!(ShelleyLedgerPredFailure::UtxowFailure(vec![]).tag(), 0);
        assert_eq!(ShelleyLedgerPredFailure::DelegsFailure(vec![]).tag(), 1);
        assert_eq!(
            ShelleyLedgerPredFailure::ShelleyWithdrawalsMissingAccounts(empty_withdrawals_payload())
                .tag(),
            2
        );
        assert_eq!(
            ShelleyLedgerPredFailure::ShelleyIncompleteWithdrawals(
                one_entry_incomplete_withdrawals_payload()
            )
            .tag(),
            3
        );
    }

    #[test]
    fn shelley_ledger_pred_failure_constructor_names() {
        assert_eq!(
            ShelleyLedgerPredFailure::UtxowFailure(vec![]).constructor(),
            "UtxowFailure"
        );
        assert_eq!(
            ShelleyLedgerPredFailure::DelegsFailure(vec![]).constructor(),
            "DelegsFailure"
        );
        assert_eq!(
            ShelleyLedgerPredFailure::ShelleyWithdrawalsMissingAccounts(empty_withdrawals_payload())
                .constructor(),
            "ShelleyWithdrawalsMissingAccounts"
        );
        assert_eq!(
            ShelleyLedgerPredFailure::ShelleyIncompleteWithdrawals(
                one_entry_incomplete_withdrawals_payload()
            )
            .constructor(),
            "ShelleyIncompleteWithdrawals"
        );
    }

    #[test]
    fn shelley_ledger_pred_failure_display_marks_raw_cbor() {
        let f = ShelleyLedgerPredFailure::UtxowFailure(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(f.to_string(), "UtxowFailure <raw-cbor 4 bytes>");
    }

    #[test]
    fn shelley_ledger_pred_failure_display_renders_typed_withdrawals() {
        let f = ShelleyLedgerPredFailure::ShelleyWithdrawalsMissingAccounts(
            empty_withdrawals_payload(),
        );
        assert_eq!(
            f.to_string(),
            "ShelleyWithdrawalsMissingAccounts (Withdrawals {unWithdrawals = fromList []})"
        );
    }

    #[test]
    fn shelley_ledger_pred_failure_display_renders_typed_incomplete_withdrawals() {
        let f = ShelleyLedgerPredFailure::ShelleyIncompleteWithdrawals(
            one_entry_incomplete_withdrawals_payload(),
        );
        let s = f.to_string();
        assert!(
            s.starts_with("ShelleyIncompleteWithdrawals (fromList [(AccountAddress {aaNetworkId = Mainnet, aaId = KeyHashObj"),
            "got: {s}"
        );
        assert!(
            s.contains("Mismatch (RelEQ) {supplied: Coin 100, expected: Coin 200}"),
            "got: {s}"
        );
        assert!(s.ends_with(")])"), "got: {s}");
    }

    #[test]
    fn withdrawals_from_cbor_empty_map() {
        // CBOR empty map: 0xa0
        let w = Withdrawals::from_cbor(&[0xa0]).expect("empty map");
        assert!(w.entries.is_empty());
        assert_eq!(w.to_string(), "Withdrawals {unWithdrawals = fromList []}");
    }

    #[test]
    fn withdrawals_from_cbor_one_entry() {
        // Build a mainnet key-hash reward account: 0xE1 + 28 0x11 bytes,
        // coin 1_000_000 (0x1A000F4240 = 4-byte unsigned).
        let mut cbor = vec![0xa1_u8]; // map with 1 entry
        // Key: bytes-29 prefix + 29 bytes
        cbor.push(0x58); // bytes with 1-byte length follows
        cbor.push(29);
        cbor.push(0xE1); // mainnet key-hash header
        cbor.extend_from_slice(&[0x11_u8; 28]);
        // Value: coin 1_000_000
        cbor.extend_from_slice(&[0x1A, 0x00, 0x0F, 0x42, 0x40]);

        let w = Withdrawals::from_cbor(&cbor).expect("single entry");
        assert_eq!(w.entries.len(), 1);
        let (account, coin) = w.entries.iter().next().expect("has entry");
        assert_eq!(account.network, 1);
        assert_eq!(*coin, 1_000_000);
        assert!(matches!(
            account.credential,
            yggdrasil_ledger::StakeCredential::AddrKeyHash(_)
        ));
        assert!(
            w.to_string().contains(
                "aaNetworkId = Mainnet, aaId = KeyHashObj (KeyHash {unKeyHash = \"11111111"
            )
        );
        assert!(w.to_string().ends_with(",Coin 1000000)]}"));
    }

    #[test]
    fn withdrawals_from_cbor_rejects_invalid_account_bytes() {
        // CBOR map with 1 entry, but key is a 28-byte string (one byte
        // short for a reward account, which must be 29 bytes).
        let mut cbor = vec![0xa1_u8];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x11_u8; 28]);
        cbor.push(0x01); // any value
        let err = Withdrawals::from_cbor(&cbor).expect_err("invalid account");
        assert!(
            err.to_string().contains("invalid reward-account key"),
            "expected reject message, got {err}"
        );
    }

    #[test]
    fn incomplete_withdrawals_rejects_empty_map() {
        let err =
            IncompleteWithdrawals::from_cbor(&[0xa0]).expect_err("empty NonEmpty must reject");
        assert!(
            err.to_string()
                .contains("NonEmptyMap requires at least one entry"),
            "got: {err}"
        );
    }

    #[test]
    fn incomplete_withdrawals_from_cbor_round_trips_supplied_expected() {
        // 2-entry map: two mainnet key-hash accounts (lex sort), each
        // with a [supplied, expected] mismatch pair.
        let mut cbor = vec![0xa2_u8];
        // entry A: account with 0x11 fill, mismatch [50, 60]
        cbor.push(0x58);
        cbor.push(29);
        cbor.push(0xE1);
        cbor.extend_from_slice(&[0x11_u8; 28]);
        cbor.push(0x82); // mismatch 2-array
        cbor.push(0x18);
        cbor.push(0x32); // 50
        cbor.push(0x18);
        cbor.push(0x3C); // 60
        // entry B: account with 0x22 fill, mismatch [100, 200]
        cbor.push(0x58);
        cbor.push(29);
        cbor.push(0xE1);
        cbor.extend_from_slice(&[0x22_u8; 28]);
        cbor.push(0x82);
        cbor.push(0x18);
        cbor.push(0x64); // 100
        cbor.push(0x18);
        cbor.push(0xC8); // 200
        let iw = IncompleteWithdrawals::from_cbor(&cbor).expect("two-entry mismatch");
        assert_eq!(iw.entries.len(), 2);
        let mut iter = iw.entries.iter();
        let (_, m_a) = iter.next().expect("entry A");
        assert_eq!(m_a.relation, MismatchRelation::RelEQ);
        assert_eq!(m_a.supplied, 50);
        assert_eq!(m_a.expected, 60);
        let (_, m_b) = iter.next().expect("entry B");
        assert_eq!(m_b.supplied, 100);
        assert_eq!(m_b.expected, 200);
    }

    #[test]
    fn tx_submit_validation_error_into_typed_round_trips() {
        let raw = TxSubmitValidationError::new(vec![0xCA, 0xFE], "rejected");
        let typed = raw.into_typed(TxValidationEra::Conway);
        assert_eq!(typed.era(), TxValidationEra::Conway);
        assert_eq!(typed.payload().raw_cbor(), &[0xCA, 0xFE]);
        assert_eq!(typed.payload().rendered(), "rejected");
        assert_eq!(typed.to_string(), "ConwayApplyTxError (rejected)");
    }
}
