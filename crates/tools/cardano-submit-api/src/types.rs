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
    /// `ShelleyUtxowPredFailure era` (one of 11 variants including
    /// `InvalidWitnessesUTXOW`, `MissingVKeyWitnessesUTXOW`, etc.)
    /// — R611 wired to the typed enum.
    UtxowFailure(ShelleyUtxowPredFailure),
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
            Self::UtxowFailure(utxow) => write!(f, "UtxowFailure ({utxow})"),
            Self::DelegsFailure(b) => {
                write!(f, "DelegsFailure <raw-cbor {} bytes>", b.len())
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

/// 32-byte hash newtype used for upstream `TxAuxDataHash` and similar
/// metadata hashes. Display matches upstream `Show TxAuxDataHash`
/// shape: `TxAuxDataHash {unTxAuxDataHash = SafeHash "<hex>"}` —
/// upstream's TxAuxDataHash is `newtype TxAuxDataHash = TxAuxDataHash
/// (SafeHash StandardCrypto EraIndependentTxAuxData)`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TxAuxDataHash(pub [u8; 32]);

impl fmt::Display for TxAuxDataHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TxAuxDataHash {{unTxAuxDataHash = SafeHash \"{}\"}}",
            hex::encode(self.0)
        )
    }
}

/// 28-byte script-hash newtype mirroring upstream
/// `newtype ScriptHash = ScriptHash (Hash ADDRHASH (Script era))`.
/// Display matches upstream's stock-derived Show
/// `ScriptHash "<hex>"`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ScriptHash(pub [u8; 28]);

impl fmt::Display for ScriptHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ScriptHash \"{}\"", hex::encode(self.0))
    }
}

/// Non-empty set of script hashes mirroring upstream
/// `NonEmptySet ScriptHash` from `Data.Set.NonEmpty` over the
/// canonical `Set ScriptHash` wire format.
///
/// CBOR shape: optional CBOR tag 258 followed by an array of
/// 28-byte byte-strings. The decoder is tag-tolerant (matches
/// upstream `decodeSet` semantics for protocol versions ≥ 9, which
/// permits but does not enforce the 258 prefix). Empty sets are
/// rejected at decode time to honour the NonEmpty invariant.
///
/// Stored as `BTreeSet<ScriptHash>` so iteration follows upstream
/// `Data.Set.toAscList` byte-lex order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptySetScriptHash {
    /// Decoded set entries. Guaranteed non-empty by `from_cbor`.
    pub entries: std::collections::BTreeSet<ScriptHash>,
}

impl NonEmptySetScriptHash {
    /// Decode a `NonEmptySet ScriptHash` from canonical CBOR bytes.
    /// Accepts either the bare list encoding or the tag-258
    /// wrapped form (`d9 01 02 ...`) per upstream's protocol-version
    /// ≥ 9 set decoder.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let major = dec
            .peek_major()
            .map_err(|err| DecoderError(format!("NonEmptySetScriptHash: peek: {err:?}")))?;
        if major == 6 {
            let tag = dec
                .tag()
                .map_err(|err| DecoderError(format!("NonEmptySetScriptHash: tag: {err:?}")))?;
            if tag != 258 {
                return Err(DecoderError(format!(
                    "NonEmptySetScriptHash: expected tag 258, got {tag}"
                )));
            }
        }
        let count = dec.array().map_err(|err| {
            DecoderError(format!(
                "NonEmptySetScriptHash: expected CBOR array: {err:?}"
            ))
        })?;
        if count == 0 {
            return Err(DecoderError(
                "NonEmptySetScriptHash: NonEmptySet requires at least one entry".to_string(),
            ));
        }
        let mut entries = std::collections::BTreeSet::new();
        for _ in 0..count {
            let hash_bytes = dec.bytes().map_err(|err| {
                DecoderError(format!(
                    "NonEmptySetScriptHash: expected ScriptHash bytes: {err:?}"
                ))
            })?;
            let arr: [u8; 28] = hash_bytes.try_into().map_err(|_| {
                DecoderError(format!(
                    "NonEmptySetScriptHash: ScriptHash must be 28 bytes, got {}",
                    hash_bytes.len()
                ))
            })?;
            entries.insert(ScriptHash(arr));
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptySetScriptHash {
    /// Render upstream stock-derived `Show (NonEmptySet ScriptHash)`:
    /// `NonEmptySet (fromList [ScriptHash "<hex>", ...])`. The
    /// `NonEmptySet` constructor wraps `Show (Set a)`'s
    /// `fromList [...]` envelope, since upstream uses `deriving
    /// stock (Show)` on `newtype NonEmptySet a = NonEmptySet (Set a)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NonEmptySet (fromList [")?;
        let mut first = true;
        for hash in &self.entries {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "{hash}")?;
        }
        f.write_str("])")?;
        Ok(())
    }
}

/// 28-byte key-hash newtype mirroring upstream
/// `newtype KeyHash (r :: KeyRole) = KeyHash {unKeyHash :: Hash
/// ADDRHASH (VerKeyDSIGN DSIGN)}` from `Cardano.Ledger.Hashes`.
/// The phantom `r :: KeyRole` (Witness, Stake, Pool, Genesis,
/// GenesisDelegate, ...) does not affect the wire format or Show.
/// Display matches the stock-derived record Show:
/// `KeyHash {unKeyHash = "<hex>"}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct KeyHash(pub [u8; 28]);

impl fmt::Display for KeyHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyHash {{unKeyHash = \"{}\"}}", hex::encode(self.0))
    }
}

/// Non-empty set of key hashes mirroring upstream
/// `NonEmptySet (KeyHash Witness)`. Wire-format and decoder
/// semantics mirror `NonEmptySetScriptHash` (R599) — tag-258
/// tolerant, non-empty invariant enforced at decode time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptySetKeyHash {
    /// Decoded set entries. Guaranteed non-empty by `from_cbor`.
    pub entries: std::collections::BTreeSet<KeyHash>,
}

impl NonEmptySetKeyHash {
    /// Decode `NonEmptySet (KeyHash Witness)` from canonical CBOR
    /// bytes. Accepts the bare-list or tag-258 wrapped form per
    /// upstream protocol-version ≥ 9 semantics.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let major = dec
            .peek_major()
            .map_err(|err| DecoderError(format!("NonEmptySetKeyHash: peek: {err:?}")))?;
        if major == 6 {
            let tag = dec
                .tag()
                .map_err(|err| DecoderError(format!("NonEmptySetKeyHash: tag: {err:?}")))?;
            if tag != 258 {
                return Err(DecoderError(format!(
                    "NonEmptySetKeyHash: expected tag 258, got {tag}"
                )));
            }
        }
        let count = dec.array().map_err(|err| {
            DecoderError(format!("NonEmptySetKeyHash: expected CBOR array: {err:?}"))
        })?;
        if count == 0 {
            return Err(DecoderError(
                "NonEmptySetKeyHash: NonEmptySet requires at least one entry".to_string(),
            ));
        }
        let mut entries = std::collections::BTreeSet::new();
        for _ in 0..count {
            let hash_bytes = dec.bytes().map_err(|err| {
                DecoderError(format!(
                    "NonEmptySetKeyHash: expected KeyHash bytes: {err:?}"
                ))
            })?;
            let arr: [u8; 28] = hash_bytes.try_into().map_err(|_| {
                DecoderError(format!(
                    "NonEmptySetKeyHash: KeyHash must be 28 bytes, got {}",
                    hash_bytes.len()
                ))
            })?;
            entries.insert(KeyHash(arr));
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptySetKeyHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NonEmptySet (fromList [")?;
        let mut first = true;
        for hash in &self.entries {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "{hash}")?;
        }
        f.write_str("])")?;
        Ok(())
    }
}

/// Possibly-empty `Set (KeyHash Witness)` mirroring upstream
/// `Set (KeyHash Witness)`. Wire-format is identical to
/// `NonEmptySetKeyHash` minus the non-empty invariant — used by
/// `MIRInsufficientGenesisSigsUTXOW` where an empty signature set
/// is a legitimate (if extreme) reject payload.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SetKeyHash {
    /// Decoded set entries; may be empty.
    pub entries: std::collections::BTreeSet<KeyHash>,
}

impl SetKeyHash {
    /// Decode `Set (KeyHash Witness)` from canonical CBOR bytes.
    /// Accepts the bare-list or tag-258 wrapped form. Empty sets
    /// are permitted.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        Self::from_decoder(&mut dec)
    }

    /// Decode from an in-progress `Decoder`. Used by parent payload
    /// decoders that have already consumed the outer envelope.
    pub fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let major = dec
            .peek_major()
            .map_err(|err| DecoderError(format!("SetKeyHash: peek: {err:?}")))?;
        if major == 6 {
            let tag = dec
                .tag()
                .map_err(|err| DecoderError(format!("SetKeyHash: tag: {err:?}")))?;
            if tag != 258 {
                return Err(DecoderError(format!(
                    "SetKeyHash: expected tag 258, got {tag}"
                )));
            }
        }
        let count = dec
            .array()
            .map_err(|err| DecoderError(format!("SetKeyHash: expected CBOR array: {err:?}")))?;
        let mut entries = std::collections::BTreeSet::new();
        for _ in 0..count {
            let hash_bytes = dec.bytes().map_err(|err| {
                DecoderError(format!("SetKeyHash: expected KeyHash bytes: {err:?}"))
            })?;
            let arr: [u8; 28] = hash_bytes.try_into().map_err(|_| {
                DecoderError(format!(
                    "SetKeyHash: KeyHash must be 28 bytes, got {}",
                    hash_bytes.len()
                ))
            })?;
            entries.insert(KeyHash(arr));
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for SetKeyHash {
    /// Render upstream `Show (Set (KeyHash r))`:
    /// `fromList [KeyHash {unKeyHash = "<hex>"}, ...]` (no
    /// NonEmptySet wrapper since upstream's `Set` is the raw type).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("fromList [")?;
        let mut first = true;
        for hash in &self.entries {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "{hash}")?;
        }
        f.write_str("]")?;
        Ok(())
    }
}

/// 32-byte verification-key newtype mirroring upstream
/// `newtype VKey (kd :: KeyRole) = VKey {unVKey :: VerKeyDSIGN DSIGN}`
/// from `Cardano.Ledger.Keys.Internal`. The phantom `kd :: KeyRole`
/// does not affect wire format or Show. Display matches upstream's
/// `deriving via Quiet (VKey kd) instance Show (VKey kd)`:
/// `VKey (VerKeyEd25519DSIGN "<hex>")`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct VKey(pub [u8; 32]);

impl fmt::Display for VKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VKey (VerKeyEd25519DSIGN \"{}\")", hex::encode(self.0))
    }
}

/// Non-empty list of verification keys mirroring upstream
/// `NonEmpty (VKey Witness)` from `Data.List.NonEmpty`.
///
/// CBOR wire format is a regular CBOR array of 32-byte bytestrings
/// with at least one entry (the NonEmpty invariant is enforced at
/// decode time). Iteration preserves insertion order to match
/// upstream `NonEmpty`'s sequential semantics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptyVKey {
    /// Decoded VKey entries. Guaranteed non-empty by `from_cbor`.
    pub entries: Vec<VKey>,
}

impl NonEmptyVKey {
    /// Decode `NonEmpty (VKey Witness)` from canonical CBOR bytes.
    /// The wire format is a CBOR array with ≥ 1 entry, each entry
    /// being a 32-byte bytestring.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let count = dec
            .array()
            .map_err(|err| DecoderError(format!("NonEmptyVKey: expected CBOR array: {err:?}")))?;
        if count == 0 {
            return Err(DecoderError(
                "NonEmptyVKey: NonEmpty requires at least one entry".to_string(),
            ));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let key_bytes = dec.bytes().map_err(|err| {
                DecoderError(format!("NonEmptyVKey: expected VKey bytes: {err:?}"))
            })?;
            let arr: [u8; 32] = key_bytes.try_into().map_err(|_| {
                DecoderError(format!(
                    "NonEmptyVKey: VKey must be 32 bytes, got {}",
                    key_bytes.len()
                ))
            })?;
            entries.push(VKey(arr));
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptyVKey {
    /// Render upstream `Show (NonEmpty a)`: `<head> :| [<tail>...]`.
    /// `:|` is the upstream `NonEmpty` data-constructor written
    /// infix. Single-entry case renders as `<head> :| []`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (head, tail) = self
            .entries
            .split_first()
            .expect("NonEmptyVKey enforces ≥1 entry at decode time");
        write!(f, "{head} :| [")?;
        let mut first = true;
        for k in tail {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "{k}")?;
        }
        f.write_str("]")?;
        Ok(())
    }
}

/// `ShelleyUtxowPredFailure` mirror.
///
/// Upstream: `data ShelleyUtxowPredFailure era` from
/// `Cardano.Ledger.Shelley.Rules.Utxow` with 11 variants encoded via
/// an outer 2-element array `[tag, payload]` (except tag 9 which
/// uses a 1-element array because it has no payload). The CBOR
/// shape mirrors `Cardano.Ledger.Shelley.Rules.Utxow.encCBOR`.
///
/// R598 ships the enum + Display for all 11 variants and a CBOR
/// decoder for the simple-payload tags (6/7/8/9). The remaining
/// variants (0/1/2/3/4/5/10) carry raw inner CBOR pending
/// per-variant NonEmptySet/sub-rule decoders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShelleyUtxowPredFailure {
    /// Tag 0: witnesses which failed in `verifiedWits` —
    /// `NonEmpty (VKey Witness)` (R601 typed decoder).
    InvalidWitnessesUTXOW(NonEmptyVKey),
    /// Tag 1: required vkey witnesses not supplied —
    /// `NonEmptySet (KeyHash Witness)` (R600 typed decoder).
    MissingVKeyWitnessesUTXOW(NonEmptySetKeyHash),
    /// Tag 2: required scripts not supplied —
    /// `NonEmptySet ScriptHash` (R599 typed decoder).
    MissingScriptWitnessesUTXOW(NonEmptySetScriptHash),
    /// Tag 3: scripts that failed validation —
    /// `NonEmptySet ScriptHash` (R599 typed decoder).
    ScriptWitnessNotValidatingUTXOW(NonEmptySetScriptHash),
    /// Tag 4: nested UTXO sub-rule failure (R610 wired to typed
    /// `ShelleyUtxoPredFailure`).
    UtxoFailure(ShelleyUtxoPredFailure),
    /// Tag 5: insufficient genesis signatures for an MIR
    /// certificate — `Set (KeyHash Witness)` (R600 typed decoder).
    MIRInsufficientGenesisSigsUTXOW(SetKeyHash),
    /// Tag 6: tx body claims metadata but its hash field is
    /// missing — the 32-byte hash that should have been present
    /// (typed).
    MissingTxBodyMetadataHash(TxAuxDataHash),
    /// Tag 7: tx body references a metadata hash but the metadata
    /// itself was not provided (typed).
    MissingTxMetadata(TxAuxDataHash),
    /// Tag 8: metadata hash in the body does not match the
    /// supplied metadata (typed Mismatch).
    ConflictingMetadataHash(Mismatch<TxAuxDataHash>),
    /// Tag 9: metadata strings out of range — no payload.
    InvalidMetadata,
    /// Tag 10: extraneous scripts supplied beyond what the tx
    /// required — `NonEmptySet ScriptHash` (R599 typed decoder).
    ExtraneousScriptWitnessesUTXOW(NonEmptySetScriptHash),
}

impl ShelleyUtxowPredFailure {
    /// Upstream CBOR tag (Word8) for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::InvalidWitnessesUTXOW(_) => 0,
            Self::MissingVKeyWitnessesUTXOW(_) => 1,
            Self::MissingScriptWitnessesUTXOW(_) => 2,
            Self::ScriptWitnessNotValidatingUTXOW(_) => 3,
            Self::UtxoFailure(_) => 4,
            Self::MIRInsufficientGenesisSigsUTXOW(_) => 5,
            Self::MissingTxBodyMetadataHash(_) => 6,
            Self::MissingTxMetadata(_) => 7,
            Self::ConflictingMetadataHash(_) => 8,
            Self::InvalidMetadata => 9,
            Self::ExtraneousScriptWitnessesUTXOW(_) => 10,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::InvalidWitnessesUTXOW(_) => "InvalidWitnessesUTXOW",
            Self::MissingVKeyWitnessesUTXOW(_) => "MissingVKeyWitnessesUTXOW",
            Self::MissingScriptWitnessesUTXOW(_) => "MissingScriptWitnessesUTXOW",
            Self::ScriptWitnessNotValidatingUTXOW(_) => "ScriptWitnessNotValidatingUTXOW",
            Self::UtxoFailure(_) => "UtxoFailure",
            Self::MIRInsufficientGenesisSigsUTXOW(_) => "MIRInsufficientGenesisSigsUTXOW",
            Self::MissingTxBodyMetadataHash(_) => "MissingTxBodyMetadataHash",
            Self::MissingTxMetadata(_) => "MissingTxMetadata",
            Self::ConflictingMetadataHash(_) => "ConflictingMetadataHash",
            Self::InvalidMetadata => "InvalidMetadata",
            Self::ExtraneousScriptWitnessesUTXOW(_) => "ExtraneousScriptWitnessesUTXOW",
        }
    }
}

impl fmt::Display for ShelleyUtxowPredFailure {
    /// Render upstream stock-derived `Show
    /// (ShelleyUtxowPredFailure era)`: `<Constructor> <payload>`.
    /// Typed payloads (tags 6/7/8/9) render through their typed
    /// Display; raw payloads (tags 0/1/2/3/4/5/10) emit a
    /// `<raw-cbor N bytes>` marker pending typed decoders.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UtxoFailure(utxo) => write!(f, "UtxoFailure ({utxo})"),
            Self::InvalidWitnessesUTXOW(keys) => {
                write!(f, "InvalidWitnessesUTXOW ({keys})")
            }
            Self::MissingVKeyWitnessesUTXOW(set) => {
                write!(f, "MissingVKeyWitnessesUTXOW ({set})")
            }
            Self::MIRInsufficientGenesisSigsUTXOW(set) => {
                write!(f, "MIRInsufficientGenesisSigsUTXOW ({set})")
            }
            Self::MissingScriptWitnessesUTXOW(set) => {
                write!(f, "MissingScriptWitnessesUTXOW ({set})")
            }
            Self::ScriptWitnessNotValidatingUTXOW(set) => {
                write!(f, "ScriptWitnessNotValidatingUTXOW ({set})")
            }
            Self::ExtraneousScriptWitnessesUTXOW(set) => {
                write!(f, "ExtraneousScriptWitnessesUTXOW ({set})")
            }
            Self::MissingTxBodyMetadataHash(h) => {
                write!(f, "MissingTxBodyMetadataHash ({h})")
            }
            Self::MissingTxMetadata(h) => write!(f, "MissingTxMetadata ({h})"),
            Self::ConflictingMetadataHash(mm) => {
                let typed = Mismatch {
                    relation: mm.relation,
                    supplied: mm.supplied,
                    expected: mm.expected,
                };
                write!(f, "ConflictingMetadataHash ({typed})")
            }
            Self::InvalidMetadata => f.write_str("InvalidMetadata"),
        }
    }
}

impl ShelleyUtxowPredFailure {
    /// Decode the full `ShelleyUtxowPredFailure` outer envelope from
    /// CBOR bytes. Returns the typed variant on success; for
    /// variants whose payload decoder is not yet ported, the
    /// returned variant carries the raw inner CBOR.
    ///
    /// Upstream encoding (`Cardano.Ledger.Shelley.Rules.Utxow.encCBOR`)
    /// wraps every variant in a CBOR array — length 2 for variants
    /// with a payload, length 1 for `InvalidMetadata` (tag 9).
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ShelleyUtxowPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if !(1..=2).contains(&len) {
            return Err(DecoderError(format!(
                "ShelleyUtxowPredFailure: expected 1- or 2-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ShelleyUtxowPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        if tag == 9 {
            if len != 1 {
                return Err(DecoderError(format!(
                    "ShelleyUtxowPredFailure: InvalidMetadata uses 1-element array, got len {len}"
                )));
            }
            return Ok(Self::InvalidMetadata);
        }
        if len != 2 {
            return Err(DecoderError(format!(
                "ShelleyUtxowPredFailure: tag {tag} uses 2-element array, got len {len}"
            )));
        }
        // For tags whose payload decoder is not yet ported, capture
        // the remaining bytes verbatim. We do that by re-encoding
        // the decoder's tail; since yggdrasil_ledger::Decoder does
        // not expose a tail accessor by default we just consume the
        // next CBOR datum and forward the slice it occupied.
        let payload_offset = dec.position();
        match tag {
            6 => {
                let bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!(
                        "MissingTxBodyMetadataHash: expected 32 bytes: {err:?}"
                    ))
                })?;
                let arr: [u8; 32] = bytes.try_into().map_err(|_| {
                    DecoderError(format!(
                        "MissingTxBodyMetadataHash: expected 32-byte hash, got {} bytes",
                        bytes.len()
                    ))
                })?;
                Ok(Self::MissingTxBodyMetadataHash(TxAuxDataHash(arr)))
            }
            7 => {
                let bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!("MissingTxMetadata: expected 32 bytes: {err:?}"))
                })?;
                let arr: [u8; 32] = bytes.try_into().map_err(|_| {
                    DecoderError(format!(
                        "MissingTxMetadata: expected 32-byte hash, got {} bytes",
                        bytes.len()
                    ))
                })?;
                Ok(Self::MissingTxMetadata(TxAuxDataHash(arr)))
            }
            8 => {
                let inner_len = dec.array().map_err(|err| {
                    DecoderError(format!(
                        "ConflictingMetadataHash: expected Mismatch 2-array: {err:?}"
                    ))
                })?;
                if inner_len != 2 {
                    return Err(DecoderError(format!(
                        "ConflictingMetadataHash: expected Mismatch 2-array, got len {inner_len}"
                    )));
                }
                let supplied_bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!(
                        "ConflictingMetadataHash: expected supplied 32-byte hash: {err:?}"
                    ))
                })?;
                let supplied: [u8; 32] = supplied_bytes.try_into().map_err(|_| {
                    DecoderError("ConflictingMetadataHash: supplied hash not 32 bytes".to_string())
                })?;
                let expected_bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!(
                        "ConflictingMetadataHash: expected expected 32-byte hash: {err:?}"
                    ))
                })?;
                let expected: [u8; 32] = expected_bytes.try_into().map_err(|_| {
                    DecoderError("ConflictingMetadataHash: expected hash not 32 bytes".to_string())
                })?;
                Ok(Self::ConflictingMetadataHash(Mismatch {
                    relation: MismatchRelation::RelEQ,
                    supplied: TxAuxDataHash(supplied),
                    expected: TxAuxDataHash(expected),
                }))
            }
            // Tags 2/3/10 share a `NonEmptySet ScriptHash` payload
            // (R599 typed decoder).
            2 | 3 | 10 => {
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError(
                        "ShelleyUtxowPredFailure: payload offset out of bounds".to_string(),
                    )
                })?;
                let set = NonEmptySetScriptHash::from_cbor(payload_bytes)?;
                Ok(match tag {
                    2 => Self::MissingScriptWitnessesUTXOW(set),
                    3 => Self::ScriptWitnessNotValidatingUTXOW(set),
                    10 => Self::ExtraneousScriptWitnessesUTXOW(set),
                    _ => unreachable!("tag range checked above"),
                })
            }
            // Tag 1: NonEmptySet (KeyHash Witness) (R600 typed
            // decoder).
            1 => {
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError(
                        "ShelleyUtxowPredFailure: payload offset out of bounds".to_string(),
                    )
                })?;
                let set = NonEmptySetKeyHash::from_cbor(payload_bytes)?;
                Ok(Self::MissingVKeyWitnessesUTXOW(set))
            }
            // Tag 5: Set (KeyHash Witness) (R600 typed decoder;
            // permits empty set, unlike tag 1).
            5 => {
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError(
                        "ShelleyUtxowPredFailure: payload offset out of bounds".to_string(),
                    )
                })?;
                let set = SetKeyHash::from_cbor(payload_bytes)?;
                Ok(Self::MIRInsufficientGenesisSigsUTXOW(set))
            }
            // Tag 0: NonEmpty (VKey Witness) (R601 typed decoder).
            0 => {
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError(
                        "ShelleyUtxowPredFailure: payload offset out of bounds".to_string(),
                    )
                })?;
                let keys = NonEmptyVKey::from_cbor(payload_bytes)?;
                Ok(Self::InvalidWitnessesUTXOW(keys))
            }
            // Tag 4: nested UTXO sub-rule (R610 typed via
            // `ShelleyUtxoPredFailure::from_cbor`).
            4 => {
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError(
                        "ShelleyUtxowPredFailure: payload offset out of bounds".to_string(),
                    )
                })?;
                let utxo = ShelleyUtxoPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::UtxoFailure(utxo))
            }
            other => Err(DecoderError(format!(
                "ShelleyUtxowPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

/// `ShelleyUtxoPredFailure` mirror — nested sub-rule under
/// `ShelleyUtxowPredFailure::UtxoFailure` (tag 4).
///
/// Upstream: `data ShelleyUtxoPredFailure era` from
/// `Cardano.Ledger.Shelley.Rules.Utxo` with 11 variants encoded via
/// upstream's `Sum` constructor — a CBOR list whose first element is
/// the Word8 tag and remaining elements are payload parts.
///
/// R602 ships the enum + Display for all 11 variants and a CBOR
/// decoder for the simple Mismatch-payload tags (1/2/3/4). The
/// remaining variants (0/5/6/7/8/9/10) carry raw inner CBOR
/// pending NonEmptySet TxIn / Value / NonEmpty TxOut / PPUP / Addr
/// decoders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShelleyUtxoPredFailure {
    /// Tag 0: bad transaction inputs — `NonEmptySet TxIn`
    /// (R603 typed decoder).
    BadInputsUTxO(NonEmptySetTxIn),
    /// Tag 1: transaction expired — `Mismatch RelLTEQ SlotNo` where
    /// supplied is the tx TTL and expected is the current slot.
    ExpiredUTxO(Mismatch<u64>),
    /// Tag 2: tx size too large — `Mismatch RelLTEQ Word32` where
    /// supplied is the tx size and expected is the protocol max.
    MaxTxSizeUTxO(Mismatch<u32>),
    /// Tag 3: tx has no inputs — no payload.
    InputSetEmptyUTxO,
    /// Tag 4: fee too small — `Mismatch RelGTEQ Coin` where
    /// supplied is the tx fee and expected is the min fee.
    FeeTooSmallUTxO(Mismatch<u64>),
    /// Tag 5: value not conserved — `Mismatch RelEQ (Value era)`.
    /// For Shelley-era (this enum's scope) Value = Coin = Word64,
    /// so the payload is a `Mismatch<u64>` with `RelEQ` relation
    /// (R608 typed decoder). Mary+ multi-asset Value lives under
    /// its own era-specific predicate-failure type.
    ValueNotConservedUTxO(Mismatch<u64>),
    /// Tag 6: outputs too small — `NonEmpty (TxOut era)` (R609
    /// typed NonEmpty wrapper; inner per-TxOut typed parse
    /// deferred to a follow-on round).
    OutputTooSmallUTxO(NonEmptyTxOut),
    /// Tag 7: nested PPUP sub-rule failure (R605 typed decoder
    /// via `ShelleyPpupPredFailure`).
    UpdateFailure(ShelleyPpupPredFailure),
    /// Tag 8: addresses with wrong network ID. 3-element CBOR
    /// array: `[8, expected-network, NonEmptySet Addr]` (R607
    /// typed decoder).
    WrongNetwork {
        /// Network ID the ledger expected.
        expected: Network,
        /// Addresses with the wrong network ID.
        wrongs: NonEmptySetAddr,
    },
    /// Tag 9: account addresses with wrong network ID. 3-element
    /// CBOR array: `[9, expected-network, NonEmptySet AccountAddress]`
    /// (R604 typed decoder).
    WrongNetworkWithdrawal {
        /// Network ID that the ledger expected.
        expected: Network,
        /// Account addresses with the wrong network ID.
        wrongs: NonEmptySetAccountAddress,
    },
    /// Tag 10: bootstrap-address attributes too big —
    /// `NonEmpty (TxOut era)` (R609 typed NonEmpty wrapper; inner
    /// per-TxOut typed parse deferred).
    OutputBootAddrAttrsTooBig(NonEmptyTxOut),
}

impl ShelleyUtxoPredFailure {
    /// Upstream CBOR tag (Word8) for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::BadInputsUTxO(_) => 0,
            Self::ExpiredUTxO(_) => 1,
            Self::MaxTxSizeUTxO(_) => 2,
            Self::InputSetEmptyUTxO => 3,
            Self::FeeTooSmallUTxO(_) => 4,
            Self::ValueNotConservedUTxO(_) => 5,
            Self::OutputTooSmallUTxO(_) => 6,
            Self::UpdateFailure(_) => 7,
            Self::WrongNetwork { .. } => 8,
            Self::WrongNetworkWithdrawal { .. } => 9,
            Self::OutputBootAddrAttrsTooBig(_) => 10,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::BadInputsUTxO(_) => "BadInputsUTxO",
            Self::ExpiredUTxO(_) => "ExpiredUTxO",
            Self::MaxTxSizeUTxO(_) => "MaxTxSizeUTxO",
            Self::InputSetEmptyUTxO => "InputSetEmptyUTxO",
            Self::FeeTooSmallUTxO(_) => "FeeTooSmallUTxO",
            Self::ValueNotConservedUTxO(_) => "ValueNotConservedUTxO",
            Self::OutputTooSmallUTxO(_) => "OutputTooSmallUTxO",
            Self::UpdateFailure(_) => "UpdateFailure",
            Self::WrongNetwork { .. } => "WrongNetwork",
            Self::WrongNetworkWithdrawal { .. } => "WrongNetworkWithdrawal",
            Self::OutputBootAddrAttrsTooBig(_) => "OutputBootAddrAttrsTooBig",
        }
    }
}

impl fmt::Display for ShelleyUtxoPredFailure {
    /// Render upstream stock-derived `Show
    /// (ShelleyUtxoPredFailure era)`: `<Constructor> <payload>`.
    /// Typed Mismatch payloads (tags 1/2/4) render through their
    /// typed Display; the InputSetEmptyUTxO variant (tag 3) has
    /// no payload; raw payloads emit a `<raw-cbor N bytes>` marker.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputTooSmallUTxO(outs) => {
                write!(f, "OutputTooSmallUTxO ({outs})")
            }
            Self::OutputBootAddrAttrsTooBig(outs) => {
                write!(f, "OutputBootAddrAttrsTooBig ({outs})")
            }
            Self::ValueNotConservedUTxO(mm) => {
                let typed = Mismatch {
                    relation: mm.relation,
                    supplied: CoinShow(mm.supplied),
                    expected: CoinShow(mm.expected),
                };
                write!(f, "ValueNotConservedUTxO ({typed})")
            }
            Self::UpdateFailure(ppup) => write!(f, "UpdateFailure ({ppup})"),
            Self::WrongNetwork { expected, wrongs } => {
                write!(f, "WrongNetwork {expected} ({wrongs})")
            }
            Self::WrongNetworkWithdrawal { expected, wrongs } => {
                write!(f, "WrongNetworkWithdrawal {expected} ({wrongs})")
            }
            Self::BadInputsUTxO(set) => write!(f, "BadInputsUTxO ({set})"),
            Self::ExpiredUTxO(mm) => write!(f, "ExpiredUTxO ({mm})"),
            Self::MaxTxSizeUTxO(mm) => write!(f, "MaxTxSizeUTxO ({mm})"),
            Self::InputSetEmptyUTxO => f.write_str("InputSetEmptyUTxO"),
            Self::FeeTooSmallUTxO(mm) => {
                let typed = Mismatch {
                    relation: mm.relation,
                    supplied: CoinShow(mm.supplied),
                    expected: CoinShow(mm.expected),
                };
                write!(f, "FeeTooSmallUTxO ({typed})")
            }
        }
    }
}

impl ShelleyUtxoPredFailure {
    /// Decode the full `ShelleyUtxoPredFailure` outer envelope from
    /// CBOR bytes. Upstream encoding wraps every variant in a CBOR
    /// list whose first element is the Word8 tag and remaining
    /// elements are payload parts. Length-1 for tag 3
    /// (`InputSetEmptyUTxO`), length-2 for tags 0/1/2/4/5/6/7/10,
    /// length-3 for tags 8/9 (`WrongNetwork[Withdrawal]`).
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ShelleyUtxoPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if !(1..=3).contains(&len) {
            return Err(DecoderError(format!(
                "ShelleyUtxoPredFailure: expected 1- to 3-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ShelleyUtxoPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        if tag == 3 {
            if len != 1 {
                return Err(DecoderError(format!(
                    "ShelleyUtxoPredFailure: InputSetEmptyUTxO uses 1-element array, got len {len}"
                )));
            }
            return Ok(Self::InputSetEmptyUTxO);
        }
        let payload_offset = dec.position();
        match tag {
            // Mismatch RelLTEQ SlotNo (Word64) — Mismatch payload is
            // a 2-element CBOR array [supplied, expected].
            1 => {
                let mm = decode_mismatch_u64(&mut dec, MismatchRelation::RelLTEQ)
                    .map_err(|err| DecoderError(format!("ExpiredUTxO: {}", err.0)))?;
                Ok(Self::ExpiredUTxO(mm))
            }
            // Mismatch RelLTEQ Word32 — supplied/expected fit u32.
            2 => {
                let inner_len = dec.array().map_err(|err| {
                    DecoderError(format!("MaxTxSizeUTxO: expected Mismatch 2-array: {err:?}"))
                })?;
                if inner_len != 2 {
                    return Err(DecoderError(format!(
                        "MaxTxSizeUTxO: expected Mismatch 2-array, got len {inner_len}"
                    )));
                }
                let supplied = dec
                    .unsigned()
                    .map_err(|err| DecoderError(format!("MaxTxSizeUTxO: supplied: {err:?}")))?;
                let expected = dec
                    .unsigned()
                    .map_err(|err| DecoderError(format!("MaxTxSizeUTxO: expected: {err:?}")))?;
                let supplied = u32::try_from(supplied).map_err(|_| {
                    DecoderError(format!(
                        "MaxTxSizeUTxO: supplied {supplied} does not fit Word32"
                    ))
                })?;
                let expected = u32::try_from(expected).map_err(|_| {
                    DecoderError(format!(
                        "MaxTxSizeUTxO: expected {expected} does not fit Word32"
                    ))
                })?;
                Ok(Self::MaxTxSizeUTxO(Mismatch {
                    relation: MismatchRelation::RelLTEQ,
                    supplied,
                    expected,
                }))
            }
            // Mismatch RelGTEQ Coin — Coin is Word64 in CBOR.
            4 => {
                let mm = decode_mismatch_u64(&mut dec, MismatchRelation::RelGTEQ)
                    .map_err(|err| DecoderError(format!("FeeTooSmallUTxO: {}", err.0)))?;
                Ok(Self::FeeTooSmallUTxO(mm))
            }
            // Tag 0: NonEmptySet TxIn (R603 typed decoder).
            0 => {
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError("ShelleyUtxoPredFailure: payload offset out of bounds".to_string())
                })?;
                let set = NonEmptySetTxIn::from_cbor(payload_bytes)?;
                Ok(Self::BadInputsUTxO(set))
            }
            // Tag 9: 3-element envelope `[9, expected-network,
            // NonEmptySet AccountAddress]` (R604 typed decoder).
            9 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "WrongNetworkWithdrawal: expected 3-element envelope, got len {len}"
                    )));
                }
                let expected = Network::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("WrongNetworkWithdrawal: {}", err.0)))?;
                let wrongs = NonEmptySetAccountAddress::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("WrongNetworkWithdrawal: {}", err.0)))?;
                Ok(Self::WrongNetworkWithdrawal { expected, wrongs })
            }
            // Tag 7: nested PPUP sub-rule (R605 typed decoder).
            7 => {
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError("ShelleyUtxoPredFailure: payload offset out of bounds".to_string())
                })?;
                let ppup = ShelleyPpupPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::UpdateFailure(ppup))
            }
            // Tag 8: 3-element envelope `[8, expected-network,
            // NonEmptySet Addr]` (R607 typed decoder).
            8 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "WrongNetwork: expected 3-element envelope, got len {len}"
                    )));
                }
                let expected = Network::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("WrongNetwork: {}", err.0)))?;
                let wrongs = NonEmptySetAddr::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("WrongNetwork: {}", err.0)))?;
                Ok(Self::WrongNetwork { expected, wrongs })
            }
            // Tag 5: Shelley-era `Mismatch RelEQ Coin` (R608 typed
            // decoder).
            5 => {
                let mm = decode_mismatch_u64(&mut dec, MismatchRelation::RelEQ)
                    .map_err(|err| DecoderError(format!("ValueNotConservedUTxO: {}", err.0)))?;
                Ok(Self::ValueNotConservedUTxO(mm))
            }
            // Tags 6/10: `NonEmpty (TxOut era)` (R609 typed
            // wrapper; inner per-TxOut typed parse deferred).
            6 | 10 => {
                let outs = NonEmptyTxOut::from_decoder(&mut dec, bytes)?;
                Ok(match tag {
                    6 => Self::OutputTooSmallUTxO(outs),
                    10 => Self::OutputBootAddrAttrsTooBig(outs),
                    _ => unreachable!("tag range checked above"),
                })
            }
            other => Err(DecoderError(format!(
                "ShelleyUtxoPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

/// Decode the canonical `Mismatch r Word64` 2-element CBOR array
/// `[supplied, expected]` into a typed `Mismatch<u64>`.
fn decode_mismatch_u64(
    dec: &mut yggdrasil_ledger::Decoder<'_>,
    relation: MismatchRelation,
) -> Result<Mismatch<u64>, DecoderError> {
    let inner_len = dec
        .array()
        .map_err(|err| DecoderError(format!("expected Mismatch 2-array: {err:?}")))?;
    if inner_len != 2 {
        return Err(DecoderError(format!(
            "expected Mismatch 2-array, got len {inner_len}"
        )));
    }
    let supplied = dec
        .unsigned()
        .map_err(|err| DecoderError(format!("supplied: {err:?}")))?;
    let expected = dec
        .unsigned()
        .map_err(|err| DecoderError(format!("expected: {err:?}")))?;
    Ok(Mismatch {
        relation,
        supplied,
        expected,
    })
}

/// 32-byte transaction-body hash newtype mirroring upstream
/// `newtype TxId = TxId {unTxId :: SafeHash EraIndependentTxBody}`.
/// Display matches upstream stock-derived record Show:
/// `TxId {unTxId = SafeHash "<hex>"}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TxId(pub [u8; 32]);

impl fmt::Display for TxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TxId {{unTxId = SafeHash \"{}\"}}", hex::encode(self.0))
    }
}

/// Transaction-output index newtype mirroring upstream
/// `newtype TxIx = TxIx {unTxIx :: Word16}`. Display matches
/// upstream stock-derived record Show: `TxIx {unTxIx = <n>}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TxIx(pub u16);

impl fmt::Display for TxIx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TxIx {{unTxIx = {}}}", self.0)
    }
}

/// Transaction input mirroring upstream `data TxIn = TxIn !TxId
/// !TxIx`. CBOR wire format is a 2-element array `[txid, ix]`.
/// Stock-derived Show: `TxIn (TxId {...}) (TxIx {...})` with each
/// single-arg constructor wrapped in parens at showsPrec 11.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TxIn {
    /// 32-byte transaction-body hash.
    pub tx_id: TxId,
    /// Output index within the referenced transaction.
    pub tx_ix: TxIx,
}

impl TxIn {
    /// Decode a single TxIn from the canonical 2-element CBOR array.
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let len = dec
            .array()
            .map_err(|err| DecoderError(format!("TxIn: expected 2-array: {err:?}")))?;
        if len != 2 {
            return Err(DecoderError(format!(
                "TxIn: expected 2-array, got len {len}"
            )));
        }
        let id_bytes = dec
            .bytes()
            .map_err(|err| DecoderError(format!("TxIn: expected TxId bytes: {err:?}")))?;
        let id_arr: [u8; 32] = id_bytes.try_into().map_err(|_| {
            DecoderError(format!(
                "TxIn: TxId must be 32 bytes, got {}",
                id_bytes.len()
            ))
        })?;
        let ix = dec
            .unsigned()
            .map_err(|err| DecoderError(format!("TxIn: expected TxIx: {err:?}")))?;
        let ix = u16::try_from(ix)
            .map_err(|_| DecoderError(format!("TxIn: TxIx {ix} does not fit Word16")))?;
        Ok(Self {
            tx_id: TxId(id_arr),
            tx_ix: TxIx(ix),
        })
    }
}

impl fmt::Display for TxIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TxIn ({}) ({})", self.tx_id, self.tx_ix)
    }
}

/// Non-empty set of transaction inputs mirroring upstream
/// `NonEmptySet TxIn`. Wire format and decoder semantics mirror
/// `NonEmptySetScriptHash` (R599) — tag-258 tolerant, non-empty
/// invariant enforced at decode time. Stored as `BTreeSet<TxIn>`
/// so iteration follows upstream `Data.Set.toAscList` byte-lex
/// order (TxIn's `Ord` instance compares by TxId then TxIx — same
/// as upstream).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptySetTxIn {
    /// Decoded set entries. Guaranteed non-empty by `from_cbor`.
    pub entries: std::collections::BTreeSet<TxIn>,
}

impl NonEmptySetTxIn {
    /// Decode `NonEmptySet TxIn` from canonical CBOR bytes.
    /// Accepts the bare-list or tag-258 wrapped form.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let major = dec
            .peek_major()
            .map_err(|err| DecoderError(format!("NonEmptySetTxIn: peek: {err:?}")))?;
        if major == 6 {
            let tag = dec
                .tag()
                .map_err(|err| DecoderError(format!("NonEmptySetTxIn: tag: {err:?}")))?;
            if tag != 258 {
                return Err(DecoderError(format!(
                    "NonEmptySetTxIn: expected tag 258, got {tag}"
                )));
            }
        }
        let count = dec.array().map_err(|err| {
            DecoderError(format!("NonEmptySetTxIn: expected CBOR array: {err:?}"))
        })?;
        if count == 0 {
            return Err(DecoderError(
                "NonEmptySetTxIn: NonEmptySet requires at least one entry".to_string(),
            ));
        }
        let mut entries = std::collections::BTreeSet::new();
        for _ in 0..count {
            entries.insert(TxIn::from_decoder(&mut dec)?);
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptySetTxIn {
    /// Render upstream `Show (NonEmptySet TxIn)`:
    /// `NonEmptySet (fromList [<TxIn>, ...])`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NonEmptySet (fromList [")?;
        let mut first = true;
        for tx_in in &self.entries {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "{tx_in}")?;
        }
        f.write_str("])")?;
        Ok(())
    }
}

/// Cardano network identifier mirroring upstream `data Network =
/// Testnet | Mainnet` from `Cardano.Ledger.BaseTypes`. CBOR encoding
/// is a single Word8: 0=Testnet, 1=Mainnet. Display matches upstream
/// stock-derived `Show Network`: the bare constructor name.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum Network {
    /// Testnet (Word8 = 0).
    Testnet,
    /// Mainnet (Word8 = 1).
    Mainnet,
}

impl Network {
    /// Decode `Network` from the next CBOR Word8 in the decoder.
    pub fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let n = dec
            .unsigned()
            .map_err(|err| DecoderError(format!("Network: expected Word8: {err:?}")))?;
        match n {
            0 => Ok(Self::Testnet),
            1 => Ok(Self::Mainnet),
            other => Err(DecoderError(format!(
                "Network: unknown network id {other} (expected 0 or 1)"
            ))),
        }
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Testnet => "Testnet",
            Self::Mainnet => "Mainnet",
        })
    }
}

/// Non-empty set of reward-account addresses mirroring upstream
/// `NonEmptySet AccountAddress`. Wire format and decoder semantics
/// mirror `NonEmptySetScriptHash` (R599) — tag-258 tolerant,
/// non-empty invariant enforced. Stored as
/// `BTreeSet<RewardAccount>` so iteration follows upstream
/// `Data.Set.toAscList` byte-lex order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptySetAccountAddress {
    /// Decoded set entries. Guaranteed non-empty by `from_cbor`.
    pub entries: std::collections::BTreeSet<yggdrasil_ledger::RewardAccount>,
}

impl NonEmptySetAccountAddress {
    /// Decode `NonEmptySet AccountAddress` from canonical CBOR
    /// bytes. Accepts the bare-list or tag-258 wrapped form.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        Self::from_decoder(&mut dec)
    }

    /// Decode from an in-progress `Decoder`. Used by parent payload
    /// decoders that have already consumed the outer envelope.
    pub fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let major = dec
            .peek_major()
            .map_err(|err| DecoderError(format!("NonEmptySetAccountAddress: peek: {err:?}")))?;
        if major == 6 {
            let tag = dec
                .tag()
                .map_err(|err| DecoderError(format!("NonEmptySetAccountAddress: tag: {err:?}")))?;
            if tag != 258 {
                return Err(DecoderError(format!(
                    "NonEmptySetAccountAddress: expected tag 258, got {tag}"
                )));
            }
        }
        let count = dec.array().map_err(|err| {
            DecoderError(format!(
                "NonEmptySetAccountAddress: expected CBOR array: {err:?}"
            ))
        })?;
        if count == 0 {
            return Err(DecoderError(
                "NonEmptySetAccountAddress: NonEmptySet requires at least one entry".to_string(),
            ));
        }
        let mut entries = std::collections::BTreeSet::new();
        for _ in 0..count {
            let key_bytes = dec.bytes().map_err(|err| {
                DecoderError(format!(
                    "NonEmptySetAccountAddress: expected AccountAddress bytes: {err:?}"
                ))
            })?;
            let account =
                yggdrasil_ledger::RewardAccount::from_bytes(key_bytes).ok_or_else(|| {
                    DecoderError(format!(
                        "NonEmptySetAccountAddress: invalid reward-account ({} bytes)",
                        key_bytes.len()
                    ))
                })?;
            entries.insert(account);
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptySetAccountAddress {
    /// Render upstream `Show (NonEmptySet AccountAddress)`:
    /// `NonEmptySet (fromList [AccountAddress {...}, ...])`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NonEmptySet (fromList [")?;
        let mut first = true;
        for account in &self.entries {
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
                "AccountAddress {{aaNetworkId = {network}, aaId = {inner}}}"
            )?;
        }
        f.write_str("])")?;
        Ok(())
    }
}

/// Protocol version mirroring upstream `data ProtVer = ProtVer
/// {pvMajor :: !Version, pvMinor :: !Natural}` from
/// `Cardano.Ledger.BaseTypes`. CBOR wire format is a 2-element
/// array `[major, minor]` (via CBORGroup). Display matches
/// upstream stock-derived record Show:
/// `ProtVer {pvMajor = <n>, pvMinor = <n>}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ProtVer {
    /// Major protocol version (upstream `pvMajor :: Version` — a
    /// Word that gates hard-fork era boundaries).
    pub major: u64,
    /// Minor protocol version (upstream `pvMinor :: Natural`).
    pub minor: u64,
}

impl ProtVer {
    /// Decode `ProtVer` as a CBOR 2-element array `[major, minor]`.
    pub fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let len = dec
            .array()
            .map_err(|err| DecoderError(format!("ProtVer: expected 2-array: {err:?}")))?;
        if len != 2 {
            return Err(DecoderError(format!(
                "ProtVer: expected 2-array, got len {len}"
            )));
        }
        let major = dec
            .unsigned()
            .map_err(|err| DecoderError(format!("ProtVer: major: {err:?}")))?;
        let minor = dec
            .unsigned()
            .map_err(|err| DecoderError(format!("ProtVer: minor: {err:?}")))?;
        Ok(Self { major, minor })
    }
}

impl fmt::Display for ProtVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ProtVer {{pvMajor = {}, pvMinor = {}}}",
            self.major, self.minor
        )
    }
}

/// PPUP voting period mirroring upstream
/// `data VotingPeriod = VoteForThisEpoch | VoteForNextEpoch`.
/// CBOR encoding: Word8 (0=VoteForThisEpoch, 1=VoteForNextEpoch).
/// Display matches upstream stock-derived constructor-name Show.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum VotingPeriod {
    /// Word8 = 0.
    VoteForThisEpoch,
    /// Word8 = 1.
    VoteForNextEpoch,
}

impl VotingPeriod {
    /// Decode `VotingPeriod` from the next CBOR Word8 in the
    /// decoder.
    pub fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let n = dec
            .unsigned()
            .map_err(|err| DecoderError(format!("VotingPeriod: expected Word8: {err:?}")))?;
        match n {
            0 => Ok(Self::VoteForThisEpoch),
            1 => Ok(Self::VoteForNextEpoch),
            other => Err(DecoderError(format!(
                "VotingPeriod: unknown voting period {other} (expected 0 or 1)"
            ))),
        }
    }
}

impl fmt::Display for VotingPeriod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::VoteForThisEpoch => "VoteForThisEpoch",
            Self::VoteForNextEpoch => "VoteForNextEpoch",
        })
    }
}

/// Cardano address newtype mirroring upstream
/// `data Addr = Addr Network PaymentCredential StakeReference
///          | AddrBootstrap BootstrapAddress`
/// from `Cardano.Ledger.Address`.
///
/// CBOR wire format is a single bytestring; the first byte's high
/// nibble encodes the address type (Shelley vs Bootstrap), the low
/// nibble encodes the network ID.
///
/// R607 stores the raw address bytes verbatim and renders them as
/// a hex-tagged marker; the full typed Shelley/Bootstrap split + the
/// upstream stock-derived Show parse-tree (PaymentCredential +
/// StakeReference) lands in a follow-on round.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Addr(pub Vec<u8>);

impl Addr {
    /// Decode a single `Addr` from a CBOR bytestring item.
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let bytes = dec
            .bytes()
            .map_err(|err| DecoderError(format!("Addr: expected bytes: {err:?}")))?;
        if bytes.is_empty() {
            return Err(DecoderError(
                "Addr: empty address bytestring is invalid".to_string(),
            ));
        }
        Ok(Self(bytes.to_vec()))
    }
}

impl fmt::Display for Addr {
    /// Render as `Addr <hex N bytes>` until the full Shelley /
    /// Bootstrap parse-tree port lands. The hex envelope preserves
    /// the raw on-wire bytes for operators while making the
    /// raw-payload status visible.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Addr <hex {} bytes: {}>",
            self.0.len(),
            hex::encode(&self.0)
        )
    }
}

/// Non-empty set of Cardano addresses mirroring upstream
/// `NonEmptySet Addr`. Wire format and decoder semantics mirror
/// `NonEmptySetScriptHash` (R599) — tag-258 tolerant, non-empty
/// invariant enforced. Stored as `BTreeSet<Addr>` so iteration
/// follows upstream `Data.Set.toAscList` byte-lex order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptySetAddr {
    /// Decoded set entries. Guaranteed non-empty by `from_cbor` /
    /// `from_decoder`.
    pub entries: std::collections::BTreeSet<Addr>,
}

impl NonEmptySetAddr {
    /// Decode `NonEmptySet Addr` from canonical CBOR bytes.
    /// Accepts the bare-list or tag-258 wrapped form.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        Self::from_decoder(&mut dec)
    }

    /// Decode from an in-progress `Decoder`.
    pub fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let major = dec
            .peek_major()
            .map_err(|err| DecoderError(format!("NonEmptySetAddr: peek: {err:?}")))?;
        if major == 6 {
            let tag = dec
                .tag()
                .map_err(|err| DecoderError(format!("NonEmptySetAddr: tag: {err:?}")))?;
            if tag != 258 {
                return Err(DecoderError(format!(
                    "NonEmptySetAddr: expected tag 258, got {tag}"
                )));
            }
        }
        let count = dec.array().map_err(|err| {
            DecoderError(format!("NonEmptySetAddr: expected CBOR array: {err:?}"))
        })?;
        if count == 0 {
            return Err(DecoderError(
                "NonEmptySetAddr: NonEmptySet requires at least one entry".to_string(),
            ));
        }
        let mut entries = std::collections::BTreeSet::new();
        for _ in 0..count {
            entries.insert(Addr::from_decoder(dec)?);
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptySetAddr {
    /// Render upstream `Show (NonEmptySet Addr)`:
    /// `NonEmptySet (fromList [<Addr>, ...])`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NonEmptySet (fromList [")?;
        let mut first = true;
        for addr in &self.entries {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "{addr}")?;
        }
        f.write_str("])")?;
        Ok(())
    }
}

/// Era-opaque transaction-output wrapper mirroring upstream
/// `TxOut era`. The wire format is era-specific:
///
/// - Shelley/Allegra/Mary: 2-element array `[address, value]`.
/// - Alonzo: 3-element array `[address, value, datum_hash]`.
/// - Babbage+: CBOR map with positional keys for the four fields.
///
/// R609 captures the raw bytes verbatim for each TxOut. The full
/// typed Shelley/Babbage parse-tree (Address + Value +
/// {DatumHash|Datum} + Script) lands in a follow-on round.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RawTxOut(pub Vec<u8>);

impl RawTxOut {
    /// Decode a single TxOut by consuming the next CBOR datum,
    /// regardless of era-specific shape. The bytes from the
    /// pre-call decoder position through the post-call position are
    /// captured verbatim.
    fn from_decoder(
        dec: &mut yggdrasil_ledger::Decoder<'_>,
        source: &[u8],
    ) -> Result<Self, DecoderError> {
        let start = dec.position();
        // Skip a single CBOR datum to advance the decoder. We use
        // the `array` / `map` outer envelope walk that matches the
        // era-specific top-level shape. For the Shelley-era 2-array
        // case, this consumes `[address, value]`; for Babbage+ map
        // it consumes the map and all entries.
        skip_single_datum(dec).map_err(|err| DecoderError(format!("TxOut: skip: {}", err.0)))?;
        let end = dec.position();
        let slice = source
            .get(start..end)
            .ok_or_else(|| DecoderError("TxOut: decoder position out of bounds".to_string()))?;
        Ok(Self(slice.to_vec()))
    }
}

impl fmt::Display for RawTxOut {
    /// Render as `TxOut <hex N bytes>` until the full era-specific
    /// parse-tree port lands. The hex preserves the raw on-wire
    /// bytes for operators while making the raw-payload status
    /// visible.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TxOut <hex {} bytes: {}>",
            self.0.len(),
            hex::encode(&self.0)
        )
    }
}

/// Skip a single CBOR datum at the decoder's current position
/// without inspecting its contents. Handles the major types
/// `unsigned`, `negative`, `bytes`, `text`, `array`, `map`, `tag`,
/// `simple/float` per RFC 8949 §3.
fn skip_single_datum(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<(), DecoderError> {
    let major = dec
        .peek_major()
        .map_err(|err| DecoderError(format!("peek-major: {err:?}")))?;
    match major {
        // 0: unsigned, 1: negative
        0 => {
            dec.unsigned()
                .map_err(|err| DecoderError(format!("unsigned: {err:?}")))?;
            Ok(())
        }
        1 => {
            // Negative ints decode as signed; fall back to a raw
            // datum walk via array(0). We don't currently need them
            // for TxOut payloads, so emit a focused error.
            Err(DecoderError(
                "skip_single_datum: negative integers not supported".to_string(),
            ))
        }
        // 2: bytes
        2 => {
            dec.bytes()
                .map_err(|err| DecoderError(format!("bytes: {err:?}")))?;
            Ok(())
        }
        // 4: array — recurse per element.
        4 => {
            let len = dec
                .array()
                .map_err(|err| DecoderError(format!("array: {err:?}")))?;
            for _ in 0..len {
                skip_single_datum(dec)?;
            }
            Ok(())
        }
        // 5: map — recurse per key/value pair.
        5 => {
            let len = dec
                .map()
                .map_err(|err| DecoderError(format!("map: {err:?}")))?;
            for _ in 0..len {
                skip_single_datum(dec)?;
                skip_single_datum(dec)?;
            }
            Ok(())
        }
        // 6: tag — consume then skip the tagged datum.
        6 => {
            dec.tag()
                .map_err(|err| DecoderError(format!("tag: {err:?}")))?;
            skip_single_datum(dec)
        }
        other => Err(DecoderError(format!(
            "skip_single_datum: unsupported major type {other}"
        ))),
    }
}

/// Non-empty list of transaction outputs mirroring upstream
/// `NonEmpty (TxOut era)`. CBOR wire format is a regular CBOR
/// array with ≥1 entry. NonEmpty invariant enforced at decode time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptyTxOut {
    /// Decoded entries. Guaranteed non-empty by `from_cbor` /
    /// `from_decoder`.
    pub entries: Vec<RawTxOut>,
}

impl NonEmptyTxOut {
    /// Decode `NonEmpty (TxOut era)` from canonical CBOR bytes.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        Self::from_decoder(&mut dec, bytes)
    }

    /// Decode from an in-progress `Decoder`. Caller must pass the
    /// original source bytes so each TxOut entry can be captured
    /// verbatim by byte range.
    pub fn from_decoder(
        dec: &mut yggdrasil_ledger::Decoder<'_>,
        source: &[u8],
    ) -> Result<Self, DecoderError> {
        let count = dec
            .array()
            .map_err(|err| DecoderError(format!("NonEmptyTxOut: expected CBOR array: {err:?}")))?;
        if count == 0 {
            return Err(DecoderError(
                "NonEmptyTxOut: NonEmpty requires at least one entry".to_string(),
            ));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(RawTxOut::from_decoder(dec, source)?);
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptyTxOut {
    /// Render upstream `Show (NonEmpty (TxOut era))`:
    /// `<head> :| [<tail>...]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (head, tail) = self
            .entries
            .split_first()
            .expect("NonEmptyTxOut enforces ≥1 entry at decode time");
        write!(f, "{head} :| [")?;
        let mut first = true;
        for t in tail {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "{t}")?;
        }
        f.write_str("]")?;
        Ok(())
    }
}

/// `ShelleyPpupPredFailure` mirror — nested PPUP sub-rule under
/// `ShelleyUtxoPredFailure::UpdateFailure` (UTXO tag 7).
///
/// Upstream: `data ShelleyPpupPredFailure era` from
/// `Cardano.Ledger.Shelley.Rules.Ppup` with 3 variants:
///
/// ```text
/// data ShelleyPpupPredFailure era
///   = NonGenesisUpdatePPUP (Mismatch RelSubset (Set (KeyHash GenesisRole)))
///   | PPUpdateWrongEpoch EpochNo EpochNo VotingPeriod
///   | PVCannotFollowPPUP ProtVer
/// ```
///
/// R606 wires all 3 variants to typed payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShelleyPpupPredFailure {
    /// Tag 0: update proposed by non-genesis key —
    /// `Mismatch RelSubset (Set (KeyHash GenesisRole))`
    /// (R606 typed via `Mismatch<SetKeyHash>` with `RelSubset`).
    NonGenesisUpdatePPUP(Mismatch<SetKeyHash>),
    /// Tag 1: update proposed for wrong epoch (R606 typed).
    PPUpdateWrongEpoch {
        /// Current epoch.
        current: u64,
        /// Epoch listed in the update.
        proposed: u64,
        /// Was the update intended for the current or next epoch?
        period: VotingPeriod,
    },
    /// Tag 2: protocol version cannot follow — `ProtVer`
    /// (R606 typed via `ProtVer` 2-element record decode).
    PVCannotFollowPPUP(ProtVer),
}

impl ShelleyPpupPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::NonGenesisUpdatePPUP(_) => 0,
            Self::PPUpdateWrongEpoch { .. } => 1,
            Self::PVCannotFollowPPUP(_) => 2,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::NonGenesisUpdatePPUP(_) => "NonGenesisUpdatePPUP",
            Self::PPUpdateWrongEpoch { .. } => "PPUpdateWrongEpoch",
            Self::PVCannotFollowPPUP(_) => "PVCannotFollowPPUP",
        }
    }

    /// Decode the full `ShelleyPpupPredFailure` outer envelope from
    /// CBOR bytes. Upstream encoding (via `Sum`) wraps every variant
    /// in a CBOR list whose first element is the Word8 tag and
    /// remaining elements are payload parts (tag 0/2 use a
    /// 2-element envelope; tag 1 uses a 4-element envelope).
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ShelleyPpupPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if !(2..=4).contains(&len) {
            return Err(DecoderError(format!(
                "ShelleyPpupPredFailure: expected 2- to 4-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ShelleyPpupPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        match tag {
            // Tag 0: `[0, Mismatch RelSubset (Set KeyHash)]` —
            // Mismatch is encoded as a 2-element CBOR array
            // `[supplied, expected]` per `EncCBOR (Mismatch r a)`.
            0 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "NonGenesisUpdatePPUP: expected 2-element envelope, got len {len}"
                    )));
                }
                let inner_len = dec.array().map_err(|err| {
                    DecoderError(format!(
                        "NonGenesisUpdatePPUP: expected Mismatch 2-array: {err:?}"
                    ))
                })?;
                if inner_len != 2 {
                    return Err(DecoderError(format!(
                        "NonGenesisUpdatePPUP: expected Mismatch 2-array, got len {inner_len}"
                    )));
                }
                let supplied = SetKeyHash::from_decoder(&mut dec).map_err(|err| {
                    DecoderError(format!("NonGenesisUpdatePPUP supplied: {}", err.0))
                })?;
                let expected = SetKeyHash::from_decoder(&mut dec).map_err(|err| {
                    DecoderError(format!("NonGenesisUpdatePPUP expected: {}", err.0))
                })?;
                Ok(Self::NonGenesisUpdatePPUP(Mismatch {
                    relation: MismatchRelation::RelSubset,
                    supplied,
                    expected,
                }))
            }
            // Tag 1: `[1, current, proposed, period]` — 4-element
            // envelope: two EpochNo (Word64) + VotingPeriod (Word8).
            1 => {
                if len != 4 {
                    return Err(DecoderError(format!(
                        "PPUpdateWrongEpoch: expected 4-element envelope, got len {len}"
                    )));
                }
                let current = dec.unsigned().map_err(|err| {
                    DecoderError(format!("PPUpdateWrongEpoch: current epoch: {err:?}"))
                })?;
                let proposed = dec.unsigned().map_err(|err| {
                    DecoderError(format!("PPUpdateWrongEpoch: proposed epoch: {err:?}"))
                })?;
                let period = VotingPeriod::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("PPUpdateWrongEpoch: {}", err.0)))?;
                Ok(Self::PPUpdateWrongEpoch {
                    current,
                    proposed,
                    period,
                })
            }
            // Tag 2: `[2, ProtVer]` — ProtVer is encoded as a
            // 2-element CBOR array (via CBORGroup).
            2 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "PVCannotFollowPPUP: expected 2-element envelope, got len {len}"
                    )));
                }
                let pv = ProtVer::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("PVCannotFollowPPUP: {}", err.0)))?;
                Ok(Self::PVCannotFollowPPUP(pv))
            }
            other => Err(DecoderError(format!(
                "ShelleyPpupPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ShelleyPpupPredFailure {
    /// Render upstream stock-derived `Show
    /// (ShelleyPpupPredFailure era)`: `<Constructor> <payload>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonGenesisUpdatePPUP(mm) => write!(f, "NonGenesisUpdatePPUP ({mm})"),
            Self::PPUpdateWrongEpoch {
                current,
                proposed,
                period,
            } => write!(f, "PPUpdateWrongEpoch {current} {proposed} {period}"),
            Self::PVCannotFollowPPUP(pv) => write!(f, "PVCannotFollowPPUP ({pv})"),
        }
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
        assert_eq!(
            ShelleyLedgerPredFailure::UtxowFailure(ShelleyUtxowPredFailure::InvalidMetadata).tag(),
            0
        );
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
            ShelleyLedgerPredFailure::UtxowFailure(ShelleyUtxowPredFailure::InvalidMetadata)
                .constructor(),
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
    fn shelley_ledger_pred_failure_display_routes_typed_utxow() {
        // R611 wired UtxowFailure to the typed
        // ShelleyUtxowPredFailure enum; Display now nests the
        // inner UTXOW Show envelope.
        let f = ShelleyLedgerPredFailure::UtxowFailure(ShelleyUtxowPredFailure::InvalidMetadata);
        assert_eq!(f.to_string(), "UtxowFailure (InvalidMetadata)");
    }

    #[test]
    fn shelley_ledger_pred_failure_display_marks_delegs_raw_cbor() {
        // DelegsFailure (LEDGER tag 1) is the last remaining raw
        // payload pending a `ShelleyDelegsPredFailure` decoder.
        let f = ShelleyLedgerPredFailure::DelegsFailure(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(f.to_string(), "DelegsFailure <raw-cbor 4 bytes>");
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
    fn shelley_utxow_pred_failure_invalid_metadata_decodes_tag9() {
        // Tag 9: 1-element CBOR array [0x81], tag 9 = 0x09
        let cbor = [0x81_u8, 0x09];
        let f = ShelleyUtxowPredFailure::from_cbor(&cbor).expect("InvalidMetadata");
        assert_eq!(f, ShelleyUtxowPredFailure::InvalidMetadata);
        assert_eq!(f.tag(), 9);
        assert_eq!(f.constructor(), "InvalidMetadata");
        assert_eq!(f.to_string(), "InvalidMetadata");
    }

    #[test]
    fn shelley_utxow_pred_failure_missing_tx_body_metadata_hash_decodes_tag6() {
        // Tag 6: 2-element array [0x82, 0x06, bytes(32)]
        let mut cbor = vec![0x82_u8, 0x06];
        // CBOR bytes header for 32 bytes
        cbor.extend_from_slice(&[0x58, 0x20]);
        cbor.extend_from_slice(&[0xAB_u8; 32]);
        let f = ShelleyUtxowPredFailure::from_cbor(&cbor).expect("MissingTxBodyMetadataHash");
        assert!(matches!(
            f,
            ShelleyUtxowPredFailure::MissingTxBodyMetadataHash(TxAuxDataHash(arr))
                if arr == [0xAB_u8; 32]
        ));
        assert!(f.to_string().starts_with(
            "MissingTxBodyMetadataHash (TxAuxDataHash {unTxAuxDataHash = SafeHash \"ababab"
        ));
    }

    #[test]
    fn shelley_utxow_pred_failure_missing_tx_metadata_decodes_tag7() {
        let mut cbor = vec![0x82_u8, 0x07];
        cbor.extend_from_slice(&[0x58, 0x20]);
        cbor.extend_from_slice(&[0xCD_u8; 32]);
        let f = ShelleyUtxowPredFailure::from_cbor(&cbor).expect("MissingTxMetadata");
        assert!(matches!(f, ShelleyUtxowPredFailure::MissingTxMetadata(_)));
        assert!(f.to_string().contains("MissingTxMetadata (TxAuxDataHash"));
    }

    #[test]
    fn shelley_utxow_pred_failure_conflicting_metadata_hash_decodes_tag8() {
        // Tag 8: outer array [0x82, 0x08, Mismatch], inner Mismatch
        // is [supplied 32-byte hash, expected 32-byte hash].
        let mut cbor = vec![0x82_u8, 0x08, 0x82];
        cbor.extend_from_slice(&[0x58, 0x20]);
        cbor.extend_from_slice(&[0x11_u8; 32]);
        cbor.extend_from_slice(&[0x58, 0x20]);
        cbor.extend_from_slice(&[0x22_u8; 32]);
        let f = ShelleyUtxowPredFailure::from_cbor(&cbor).expect("ConflictingMetadataHash");
        if let ShelleyUtxowPredFailure::ConflictingMetadataHash(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelEQ);
            assert_eq!(mm.supplied.0, [0x11_u8; 32]);
            assert_eq!(mm.expected.0, [0x22_u8; 32]);
        } else {
            panic!("expected ConflictingMetadataHash, got {f:?}");
        }
        let s = f.to_string();
        assert!(s.contains("Mismatch (RelEQ)"));
        assert!(s.contains("supplied: TxAuxDataHash {unTxAuxDataHash = SafeHash \"11"));
        assert!(s.contains("expected: TxAuxDataHash {unTxAuxDataHash = SafeHash \"22"));
    }

    #[test]
    fn non_empty_set_script_hash_decodes_tag258_form() {
        // tag 258 (0xD9 01 02) + array(2) + bytes(28) + bytes(28)
        let mut cbor = vec![0xD9, 0x01, 0x02];
        cbor.push(0x82); // array(2)
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x11_u8; 28]);
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x22_u8; 28]);
        let set = NonEmptySetScriptHash::from_cbor(&cbor).expect("tag258 set");
        assert_eq!(set.entries.len(), 2);
        let s = set.to_string();
        assert!(
            s.starts_with("NonEmptySet (fromList [ScriptHash \"11"),
            "got {s}"
        );
        assert!(s.contains("ScriptHash \"22"));
    }

    #[test]
    fn non_empty_set_script_hash_decodes_bare_list() {
        // bare array(1) + bytes(28), no tag prefix
        let mut cbor = vec![0x81_u8];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x33_u8; 28]);
        let set = NonEmptySetScriptHash::from_cbor(&cbor).expect("bare list");
        assert_eq!(set.entries.len(), 1);
    }

    #[test]
    fn non_empty_set_script_hash_rejects_empty_set() {
        let cbor = vec![0xD9, 0x01, 0x02, 0x80]; // tag 258 + array(0)
        let err = NonEmptySetScriptHash::from_cbor(&cbor).expect_err("empty must reject");
        assert!(
            err.to_string()
                .contains("NonEmptySet requires at least one entry"),
            "got: {err}"
        );
    }

    #[test]
    fn non_empty_set_script_hash_rejects_wrong_hash_length() {
        // tag 258 + array(1) + bytes(27)
        let mut cbor = vec![0xD9, 0x01, 0x02, 0x81];
        cbor.push(0x58);
        cbor.push(27);
        cbor.extend_from_slice(&[0x44_u8; 27]);
        let err = NonEmptySetScriptHash::from_cbor(&cbor).expect_err("wrong length must reject");
        assert!(
            err.to_string().contains("ScriptHash must be 28 bytes"),
            "got: {err}"
        );
    }

    #[test]
    fn shelley_utxow_pred_failure_missing_script_witnesses_decodes_tag2() {
        // outer [0x82, 0x02, tag 258 + array(1) + bytes(28)]
        let mut cbor = vec![0x82_u8, 0x02];
        cbor.extend_from_slice(&[0xD9, 0x01, 0x02]);
        cbor.push(0x81);
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x77_u8; 28]);
        let f = ShelleyUtxowPredFailure::from_cbor(&cbor).expect("typed tag-2");
        if let ShelleyUtxowPredFailure::MissingScriptWitnessesUTXOW(set) = &f {
            assert_eq!(set.entries.len(), 1);
        } else {
            panic!("expected MissingScriptWitnessesUTXOW, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with("MissingScriptWitnessesUTXOW (NonEmptySet (fromList [ScriptHash \"77"),
            "got: {s}"
        );
    }

    #[test]
    fn shelley_utxow_pred_failure_utxo_failure_routes_to_typed_utxo() {
        // UTXOW tag 4 with inner UTXO tag 3 (InputSetEmptyUTxO,
        // no payload). Outer envelope: [0x82, 0x04, [0x81, 0x03]].
        let cbor = [0x82_u8, 0x04, 0x81, 0x03];
        let f = ShelleyUtxowPredFailure::from_cbor(&cbor).expect("UtxoFailure");
        if let ShelleyUtxowPredFailure::UtxoFailure(utxo) = &f {
            assert_eq!(utxo.tag(), 3);
            assert!(matches!(utxo, ShelleyUtxoPredFailure::InputSetEmptyUTxO));
        } else {
            panic!("expected typed UtxoFailure, got {f:?}");
        }
        assert_eq!(f.to_string(), "UtxoFailure (InputSetEmptyUTxO)");
    }

    #[test]
    fn shelley_utxow_pred_failure_utxo_failure_nests_full_utxo_predicate() {
        // UTXOW tag 4 wrapping a UTXO tag 4 (FeeTooSmallUTxO) —
        // full nested Display chain `UtxoFailure (FeeTooSmallUTxO
        // (Mismatch (RelGTEQ) {supplied: Coin N, expected: Coin
        // M}))`.
        let cbor = [0x82_u8, 0x04, 0x82, 0x04, 0x82, 0x18, 100, 0x18, 200];
        let f = ShelleyUtxowPredFailure::from_cbor(&cbor).expect("nested utxo");
        assert_eq!(
            f.to_string(),
            "UtxoFailure (FeeTooSmallUTxO (Mismatch (RelGTEQ) {supplied: Coin 100, expected: Coin 200}))"
        );
    }

    #[test]
    fn shelley_utxow_pred_failure_invalid_witnesses_decodes_tag0() {
        // outer [0x82, 0x00, array(1) + bytes(32)]
        let mut cbor = vec![0x82_u8, 0x00];
        cbor.push(0x81); // array(1)
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0xEE_u8; 32]);
        let f = ShelleyUtxowPredFailure::from_cbor(&cbor).expect("typed tag-0");
        if let ShelleyUtxowPredFailure::InvalidWitnessesUTXOW(keys) = &f {
            assert_eq!(keys.entries.len(), 1);
            assert_eq!(keys.entries[0].0, [0xEE_u8; 32]);
        } else {
            panic!("expected InvalidWitnessesUTXOW typed, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with("InvalidWitnessesUTXOW (VKey (VerKeyEd25519DSIGN \"eeee"),
            "got: {s}"
        );
        assert!(s.ends_with(":| [])"));
    }

    #[test]
    fn non_empty_vkey_rejects_empty_list() {
        let cbor = vec![0x80_u8]; // empty array
        let err = NonEmptyVKey::from_cbor(&cbor).expect_err("empty must reject");
        assert!(
            err.to_string()
                .contains("NonEmpty requires at least one entry"),
            "got: {err}"
        );
    }

    #[test]
    fn non_empty_vkey_multi_entry_renders_with_cons_separator() {
        // array(2) + 2x bytes(32)
        let mut cbor = vec![0x82_u8];
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0x11_u8; 32]);
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0x22_u8; 32]);
        let keys = NonEmptyVKey::from_cbor(&cbor).expect("two entries");
        assert_eq!(keys.entries.len(), 2);
        let s = keys.to_string();
        assert!(s.contains("VKey (VerKeyEd25519DSIGN \"1111"));
        assert!(s.contains(":| [VKey (VerKeyEd25519DSIGN \"2222"));
    }

    #[test]
    fn shelley_utxo_pred_failure_input_set_empty_decodes_tag3() {
        // Tag 3: 1-element CBOR array [0x81, 0x03]
        let cbor = [0x81_u8, 0x03];
        let f = ShelleyUtxoPredFailure::from_cbor(&cbor).expect("InputSetEmptyUTxO");
        assert_eq!(f, ShelleyUtxoPredFailure::InputSetEmptyUTxO);
        assert_eq!(f.tag(), 3);
        assert_eq!(f.to_string(), "InputSetEmptyUTxO");
    }

    #[test]
    fn shelley_utxo_pred_failure_expired_utxo_decodes_tag1() {
        // Tag 1: outer [0x82, 0x01, mismatch-array]
        // Mismatch [supplied=100, expected=99] (RelLTEQ — tx TTL was
        // less than current slot).
        let cbor = [0x82_u8, 0x01, 0x82, 0x18, 100, 0x18, 99];
        let f = ShelleyUtxoPredFailure::from_cbor(&cbor).expect("ExpiredUTxO");
        if let ShelleyUtxoPredFailure::ExpiredUTxO(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelLTEQ);
            assert_eq!(mm.supplied, 100);
            assert_eq!(mm.expected, 99);
        } else {
            panic!("expected ExpiredUTxO, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "ExpiredUTxO (Mismatch (RelLTEQ) {supplied: 100, expected: 99})"
        );
    }

    #[test]
    fn shelley_utxo_pred_failure_max_tx_size_decodes_tag2() {
        let cbor = [0x82_u8, 0x02, 0x82, 0x19, 0x40, 0x00, 0x19, 0x20, 0x00];
        // supplied=0x4000=16384, expected=0x2000=8192
        let f = ShelleyUtxoPredFailure::from_cbor(&cbor).expect("MaxTxSizeUTxO");
        if let ShelleyUtxoPredFailure::MaxTxSizeUTxO(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelLTEQ);
            assert_eq!(mm.supplied, 16384_u32);
            assert_eq!(mm.expected, 8192_u32);
        } else {
            panic!("expected MaxTxSizeUTxO, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "MaxTxSizeUTxO (Mismatch (RelLTEQ) {supplied: 16384, expected: 8192})"
        );
    }

    #[test]
    fn shelley_utxo_pred_failure_fee_too_small_decodes_tag4() {
        // Tag 4: outer [0x82, 0x04, mismatch-array]
        let cbor = [
            0x82_u8, 0x04, 0x82, 0x1A, 0x00, 0x01, 0x86, 0xA0, 0x1A, 0x00, 0x03, 0x0D, 0x40,
        ];
        // supplied=0x000186A0=100_000, expected=0x00030D40=200_000
        let f = ShelleyUtxoPredFailure::from_cbor(&cbor).expect("FeeTooSmallUTxO");
        if let ShelleyUtxoPredFailure::FeeTooSmallUTxO(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelGTEQ);
            assert_eq!(mm.supplied, 100_000);
            assert_eq!(mm.expected, 200_000);
        } else {
            panic!("expected FeeTooSmallUTxO, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "FeeTooSmallUTxO (Mismatch (RelGTEQ) {supplied: Coin 100000, expected: Coin 200000})"
        );
    }

    #[test]
    fn shelley_utxo_pred_failure_output_too_small_decodes_tag6() {
        // Tag 6 with NonEmpty [TxOut bytes]; TxOut is a 2-array
        // [addr-bytes, coin] for Shelley era.
        let mut cbor = vec![0x82_u8, 0x06];
        cbor.push(0x81); // NonEmpty array(1)
        cbor.push(0x82); // TxOut 2-array
        cbor.push(0x58); // bytes header
        cbor.push(29);
        cbor.extend_from_slice(&[0x61_u8]);
        cbor.extend_from_slice(&[0xAA_u8; 28]);
        cbor.push(0x18); // coin = 100
        cbor.push(0x64);
        let f = ShelleyUtxoPredFailure::from_cbor(&cbor).expect("OutputTooSmallUTxO");
        if let ShelleyUtxoPredFailure::OutputTooSmallUTxO(outs) = &f {
            assert_eq!(outs.entries.len(), 1);
            // The captured TxOut bytes should be the full 2-array
            // including the array header.
            assert_eq!(outs.entries[0].0[0], 0x82); // 2-array
        } else {
            panic!("expected OutputTooSmallUTxO typed, got {f:?}");
        }
        let s = f.to_string();
        assert!(s.starts_with("OutputTooSmallUTxO (TxOut <hex"), "got: {s}");
        assert!(s.ends_with("]) ") || s.ends_with("])"), "got: {s}");
    }

    #[test]
    fn non_empty_tx_out_rejects_empty_list() {
        let cbor = vec![0x80_u8];
        let err = NonEmptyTxOut::from_cbor(&cbor).expect_err("empty must reject");
        assert!(
            err.to_string()
                .contains("NonEmpty requires at least one entry"),
            "got: {err}"
        );
    }

    #[test]
    fn shelley_utxo_pred_failure_value_not_conserved_decodes_tag5() {
        // outer [0x82, 0x05, mismatch [supplied=10, expected=20]]
        let cbor = [0x82_u8, 0x05, 0x82, 0x0A, 0x14];
        let f = ShelleyUtxoPredFailure::from_cbor(&cbor).expect("ValueNotConservedUTxO");
        if let ShelleyUtxoPredFailure::ValueNotConservedUTxO(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelEQ);
            assert_eq!(mm.supplied, 10);
            assert_eq!(mm.expected, 20);
        } else {
            panic!("expected ValueNotConservedUTxO typed, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "ValueNotConservedUTxO (Mismatch (RelEQ) {supplied: Coin 10, expected: Coin 20})"
        );
    }

    #[test]
    fn shelley_utxo_pred_failure_bad_inputs_decodes_tag0() {
        // outer [0x82, 0x00, tag 258 + array(1) + TxIn]
        // TxIn = [bytes(32), Word16] — id = 0xAA*32, ix = 7
        let mut cbor = vec![0x82_u8, 0x00];
        cbor.extend_from_slice(&[0xD9, 0x01, 0x02]); // tag 258
        cbor.push(0x81); // array(1)
        cbor.push(0x82); // TxIn 2-array
        cbor.push(0x58); // bytes header
        cbor.push(32);
        cbor.extend_from_slice(&[0xAA_u8; 32]);
        cbor.push(0x07); // ix = 7
        let f = ShelleyUtxoPredFailure::from_cbor(&cbor).expect("BadInputsUTxO");
        if let ShelleyUtxoPredFailure::BadInputsUTxO(set) = &f {
            assert_eq!(set.entries.len(), 1);
            let tx_in = set.entries.iter().next().expect("one entry");
            assert_eq!(tx_in.tx_id.0, [0xAA_u8; 32]);
            assert_eq!(tx_in.tx_ix.0, 7);
        } else {
            panic!("expected BadInputsUTxO typed, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with(
                "BadInputsUTxO (NonEmptySet (fromList [TxIn (TxId {unTxId = SafeHash \"aaaa"
            ),
            "got: {s}"
        );
        assert!(s.contains("(TxIx {unTxIx = 7})"));
    }

    #[test]
    fn network_from_decoder_round_trips() {
        use yggdrasil_ledger::Decoder;
        let mut dec0 = Decoder::new(&[0x00]);
        assert_eq!(
            Network::from_decoder(&mut dec0).expect("testnet"),
            Network::Testnet
        );
        let mut dec1 = Decoder::new(&[0x01]);
        assert_eq!(
            Network::from_decoder(&mut dec1).expect("mainnet"),
            Network::Mainnet
        );
        let mut dec_n = Decoder::new(&[0x09]);
        let err = Network::from_decoder(&mut dec_n).expect_err("unknown rejects");
        assert!(
            err.to_string().contains("unknown network id 9"),
            "got: {err}"
        );
    }

    #[test]
    fn shelley_utxo_pred_failure_wrong_network_withdrawal_decodes_tag9() {
        // outer [0x83, 0x09, network=0x01 mainnet, NonEmptySet (1 entry)]
        let mut cbor = vec![0x83_u8, 0x09, 0x01];
        // tag 258 + array(1) + bytes(29) for the AccountAddress
        cbor.extend_from_slice(&[0xD9, 0x01, 0x02]);
        cbor.push(0x81);
        cbor.push(0x58);
        cbor.push(29);
        // Mainnet key-hash account: 0xE1 + 28 0x42 bytes
        cbor.push(0xE1);
        cbor.extend_from_slice(&[0x42_u8; 28]);
        let f = ShelleyUtxoPredFailure::from_cbor(&cbor).expect("WrongNetworkWithdrawal");
        if let ShelleyUtxoPredFailure::WrongNetworkWithdrawal { expected, wrongs } = &f {
            assert_eq!(*expected, Network::Mainnet);
            assert_eq!(wrongs.entries.len(), 1);
        } else {
            panic!("expected WrongNetworkWithdrawal, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with(
                "WrongNetworkWithdrawal Mainnet (NonEmptySet (fromList [AccountAddress {aaNetworkId = Mainnet, aaId = KeyHashObj"
            ),
            "got: {s}"
        );
    }

    #[test]
    fn shelley_ppup_pred_failure_tag_dispatch() {
        // Tag 0: outer [0x82, 0x00, payload]
        let mut cbor = vec![0x82_u8, 0x00];
        cbor.extend_from_slice(&[0x82, 0x80, 0x80]); // dummy mismatch
        let f = ShelleyPpupPredFailure::from_cbor(&cbor).expect("NonGenesisUpdatePPUP");
        assert_eq!(f.tag(), 0);
        assert_eq!(f.constructor(), "NonGenesisUpdatePPUP");
        assert!(matches!(f, ShelleyPpupPredFailure::NonGenesisUpdatePPUP(_)));

        // Tag 1: outer [0x84, 0x01, ce, e, vp]
        let cbor = [0x84_u8, 0x01, 0x18, 100, 0x18, 99, 0x00];
        let f = ShelleyPpupPredFailure::from_cbor(&cbor).expect("PPUpdateWrongEpoch");
        assert_eq!(f.tag(), 1);
        assert_eq!(f.constructor(), "PPUpdateWrongEpoch");

        // Tag 2: outer [0x82, 0x02, ProtVer-array]
        let cbor = [0x82_u8, 0x02, 0x82, 0x09, 0x00];
        let f = ShelleyPpupPredFailure::from_cbor(&cbor).expect("PVCannotFollowPPUP");
        assert_eq!(f.tag(), 2);
        assert_eq!(f.constructor(), "PVCannotFollowPPUP");
    }

    #[test]
    fn shelley_ppup_pred_failure_pv_cannot_follow_decodes_tag2() {
        // Tag 2: outer [0x82, 0x02, ProtVer-array]; ProtVer = [9, 0]
        let cbor = [0x82_u8, 0x02, 0x82, 0x09, 0x00];
        let f = ShelleyPpupPredFailure::from_cbor(&cbor).expect("PVCannotFollowPPUP");
        if let ShelleyPpupPredFailure::PVCannotFollowPPUP(pv) = &f {
            assert_eq!(pv.major, 9);
            assert_eq!(pv.minor, 0);
        } else {
            panic!("expected PVCannotFollowPPUP, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "PVCannotFollowPPUP (ProtVer {pvMajor = 9, pvMinor = 0})"
        );
    }

    #[test]
    fn shelley_ppup_pred_failure_pp_update_wrong_epoch_decodes_tag1() {
        // Tag 1: outer [0x84, 0x01, 100, 99, 0]
        let cbor = [0x84_u8, 0x01, 0x18, 100, 0x18, 99, 0x00];
        let f = ShelleyPpupPredFailure::from_cbor(&cbor).expect("PPUpdateWrongEpoch");
        if let ShelleyPpupPredFailure::PPUpdateWrongEpoch {
            current,
            proposed,
            period,
        } = &f
        {
            assert_eq!(*current, 100);
            assert_eq!(*proposed, 99);
            assert_eq!(*period, VotingPeriod::VoteForThisEpoch);
        } else {
            panic!("expected PPUpdateWrongEpoch, got {f:?}");
        }
        assert_eq!(f.to_string(), "PPUpdateWrongEpoch 100 99 VoteForThisEpoch");
    }

    #[test]
    fn shelley_ppup_pred_failure_non_genesis_update_decodes_tag0() {
        // Tag 0: outer [0x82, 0x00, [supplied-set, expected-set]]
        // both sets tag 258 with 1 entry each.
        let mut cbor = vec![0x82_u8, 0x00, 0x82];
        // supplied
        cbor.extend_from_slice(&[0xD9, 0x01, 0x02, 0x81, 0x58, 28]);
        cbor.extend_from_slice(&[0x11_u8; 28]);
        // expected
        cbor.extend_from_slice(&[0xD9, 0x01, 0x02, 0x81, 0x58, 28]);
        cbor.extend_from_slice(&[0x22_u8; 28]);
        let f = ShelleyPpupPredFailure::from_cbor(&cbor).expect("NonGenesisUpdatePPUP");
        if let ShelleyPpupPredFailure::NonGenesisUpdatePPUP(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelSubset);
            assert_eq!(mm.supplied.entries.len(), 1);
            assert_eq!(mm.expected.entries.len(), 1);
        } else {
            panic!("expected NonGenesisUpdatePPUP, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with("NonGenesisUpdatePPUP (Mismatch (RelSubset)"),
            "got: {s}"
        );
        assert!(s.contains("KeyHash {unKeyHash = \"1111"));
        assert!(s.contains("KeyHash {unKeyHash = \"2222"));
    }

    #[test]
    fn shelley_ppup_pred_failure_unknown_tag_rejects() {
        let cbor = vec![0x82_u8, 0x18, 42, 0x40];
        let err = ShelleyPpupPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 42"),
            "got: {err}"
        );
    }

    #[test]
    fn shelley_utxo_pred_failure_update_failure_routes_to_typed_ppup() {
        // UTXO tag 7 with inner PPUP tag 2 (PVCannotFollowPPUP).
        // Outer [0x82, 0x07, PPUP-bytes]; PPUP-bytes = [0x82, 0x02, ProtVer]
        let cbor = [0x82_u8, 0x07, 0x82, 0x02, 0x82, 0x09, 0x00];
        let f = ShelleyUtxoPredFailure::from_cbor(&cbor).expect("UpdateFailure");
        if let ShelleyUtxoPredFailure::UpdateFailure(ppup) = &f {
            assert_eq!(ppup.tag(), 2);
            assert_eq!(ppup.constructor(), "PVCannotFollowPPUP");
            if let ShelleyPpupPredFailure::PVCannotFollowPPUP(pv) = ppup {
                assert_eq!(pv.major, 9);
                assert_eq!(pv.minor, 0);
            } else {
                panic!("expected inner PVCannotFollowPPUP, got {ppup:?}");
            }
        } else {
            panic!("expected typed UpdateFailure, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "UpdateFailure (PVCannotFollowPPUP (ProtVer {pvMajor = 9, pvMinor = 0}))"
        );
    }

    #[test]
    fn shelley_utxo_pred_failure_wrong_network_decodes_tag8() {
        // outer [0x83, 0x08, network=1 mainnet, NonEmptySet (1 addr)]
        let mut cbor = vec![0x83_u8, 0x08, 0x01];
        cbor.extend_from_slice(&[0xD9, 0x01, 0x02]); // tag 258
        cbor.push(0x81); // array(1)
        // address: 29 bytes mainnet payment cred
        cbor.push(0x58);
        cbor.push(29);
        cbor.push(0x61); // mainnet payment-key shelley header
        cbor.extend_from_slice(&[0x77_u8; 28]);
        let f = ShelleyUtxoPredFailure::from_cbor(&cbor).expect("WrongNetwork");
        if let ShelleyUtxoPredFailure::WrongNetwork { expected, wrongs } = &f {
            assert_eq!(*expected, Network::Mainnet);
            assert_eq!(wrongs.entries.len(), 1);
            let addr = wrongs.entries.iter().next().expect("one addr");
            assert_eq!(addr.0.len(), 29);
            assert_eq!(addr.0[0], 0x61);
        } else {
            panic!("expected WrongNetwork, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with("WrongNetwork Mainnet (NonEmptySet (fromList [Addr <hex 29 bytes: 61"),
            "got: {s}"
        );
    }

    #[test]
    fn shelley_utxo_pred_failure_wrong_network_rejects_wrong_envelope_length() {
        let cbor = vec![0x82_u8, 0x08, 0x01];
        let err = ShelleyUtxoPredFailure::from_cbor(&cbor).expect_err("len-2 must reject");
        assert!(
            err.to_string()
                .contains("WrongNetwork: expected 3-element envelope"),
            "got: {err}"
        );
    }

    #[test]
    fn non_empty_set_addr_rejects_empty_set() {
        let cbor = vec![0xD9, 0x01, 0x02, 0x80];
        let err = NonEmptySetAddr::from_cbor(&cbor).expect_err("empty must reject");
        assert!(
            err.to_string()
                .contains("NonEmptySet requires at least one entry"),
            "got: {err}"
        );
    }

    #[test]
    fn shelley_utxo_pred_failure_wrong_network_withdrawal_rejects_wrong_envelope_length() {
        // 2-element envelope (missing wrongs) — should reject.
        let cbor = vec![0x82_u8, 0x09, 0x01];
        let err = ShelleyUtxoPredFailure::from_cbor(&cbor).expect_err("len-2 must reject");
        assert!(
            err.to_string()
                .contains("WrongNetworkWithdrawal: expected 3-element envelope"),
            "got: {err}"
        );
    }

    #[test]
    fn non_empty_set_tx_in_rejects_empty_set() {
        let cbor = vec![0xD9, 0x01, 0x02, 0x80];
        let err = NonEmptySetTxIn::from_cbor(&cbor).expect_err("empty must reject");
        assert!(
            err.to_string()
                .contains("NonEmptySet requires at least one entry"),
            "got: {err}"
        );
    }

    #[test]
    fn shelley_utxo_pred_failure_unknown_tag_rejects() {
        let cbor = vec![0x82_u8, 0x18, 99, 0x40];
        let err = ShelleyUtxoPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 99"),
            "got: {err}"
        );
    }

    #[test]
    fn shelley_utxow_pred_failure_missing_vkey_witnesses_decodes_tag1() {
        // outer [0x82, 0x01, tag 258 + array(1) + bytes(28)]
        let mut cbor = vec![0x82_u8, 0x01];
        cbor.extend_from_slice(&[0xD9, 0x01, 0x02]); // tag 258
        cbor.push(0x81); // array(1)
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0xAB_u8; 28]);
        let f = ShelleyUtxowPredFailure::from_cbor(&cbor).expect("typed tag-1");
        if let ShelleyUtxowPredFailure::MissingVKeyWitnessesUTXOW(set) = &f {
            assert_eq!(set.entries.len(), 1);
        } else {
            panic!("expected MissingVKeyWitnessesUTXOW typed, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with(
                "MissingVKeyWitnessesUTXOW (NonEmptySet (fromList [KeyHash {unKeyHash = \"abab"
            ),
            "got: {s}"
        );
    }

    #[test]
    fn shelley_utxow_pred_failure_mir_insufficient_genesis_sigs_decodes_tag5_empty_set() {
        // outer [0x82, 0x05, tag 258 + array(0)]
        let cbor = vec![0x82_u8, 0x05, 0xD9, 0x01, 0x02, 0x80];
        let f = ShelleyUtxowPredFailure::from_cbor(&cbor).expect("typed tag-5 empty");
        if let ShelleyUtxowPredFailure::MIRInsufficientGenesisSigsUTXOW(set) = &f {
            assert!(set.entries.is_empty());
        } else {
            panic!("expected MIRInsufficientGenesisSigsUTXOW empty, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "MIRInsufficientGenesisSigsUTXOW (fromList [])"
        );
    }

    #[test]
    fn shelley_utxow_pred_failure_mir_insufficient_genesis_sigs_decodes_tag5_with_keys() {
        // outer [0x82, 0x05, tag 258 + array(2) + 2x bytes(28)]
        let mut cbor = vec![0x82_u8, 0x05];
        cbor.extend_from_slice(&[0xD9, 0x01, 0x02]);
        cbor.push(0x82);
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x55_u8; 28]);
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x66_u8; 28]);
        let f = ShelleyUtxowPredFailure::from_cbor(&cbor).expect("typed tag-5 with keys");
        if let ShelleyUtxowPredFailure::MIRInsufficientGenesisSigsUTXOW(set) = &f {
            assert_eq!(set.entries.len(), 2);
        } else {
            panic!("expected MIRInsufficientGenesisSigsUTXOW with keys, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with("MIRInsufficientGenesisSigsUTXOW (fromList [KeyHash {unKeyHash = \"5555"),
            "got: {s}"
        );
        assert!(s.contains("KeyHash {unKeyHash = \"6666"));
    }

    #[test]
    fn shelley_utxow_pred_failure_unknown_tag_rejects() {
        // Tag 99 is not a valid variant.
        let cbor = vec![0x82_u8, 0x18, 99, 0x40];
        let err = ShelleyUtxowPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 99"),
            "got: {err}"
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
