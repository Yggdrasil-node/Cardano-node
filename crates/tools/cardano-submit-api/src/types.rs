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
    /// `ShelleyDelegsPredFailure era` newtype wrapping a DELPL
    /// failure (R612 wired to the typed scaffold; inner DELPL
    /// decoder still pending).
    DelegsFailure(ShelleyDelegsPredFailure),
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
            Self::DelegsFailure(delegs) => write!(f, "DelegsFailure ({delegs})"),
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
    /// Supplied is required to be < expected.
    RelLT,
    /// Supplied is required to be > expected.
    RelGT,
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
            Self::RelLT => "RelLT",
            Self::RelGT => "RelGT",
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

/// Signed coin-delta renderer matching upstream `Show DeltaCoin`:
/// `newtype DeltaCoin = DeltaCoin Integer` with `deriving (Show)
/// via Quiet`. Quiet keeps the constructor name; the inner
/// `Integer` is shown at precedence 11, so negative values are
/// parenthesised: `DeltaCoin 5` / `DeltaCoin (-5)`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DeltaCoinShow(pub i64);

impl fmt::Display for DeltaCoinShow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 < 0 {
            write!(f, "DeltaCoin ({})", self.0)
        } else {
            write!(f, "DeltaCoin {}", self.0)
        }
    }
}

/// `StrictMaybe SlotNo` renderer. Upstream `StrictMaybe` is
/// CBOR-encoded as a CBOR list: empty for `SNothing`, 1-element
/// for `SJust`. Display matches upstream stock-derived Show:
/// `SNothing` / `SJust (SlotNo {unSlotNo = <n>})`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct StrictMaybeSlot(pub Option<u64>);

impl StrictMaybeSlot {
    /// Decode a `StrictMaybe SlotNo` from an in-progress decoder
    /// (CBOR list: 0-element = SNothing, 1-element = SJust).
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let len = dec
            .array()
            .map_err(|err| DecoderError(format!("StrictMaybeSlot: expected CBOR list: {err:?}")))?;
        match len {
            0 => Ok(Self(None)),
            1 => {
                let slot = dec.unsigned().map_err(|err| {
                    DecoderError(format!("StrictMaybeSlot: expected SlotNo: {err:?}"))
                })?;
                Ok(Self(Some(slot)))
            }
            other => Err(DecoderError(format!(
                "StrictMaybeSlot: expected 0- or 1-element list, got len {other}"
            ))),
        }
    }
}

impl fmt::Display for StrictMaybeSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            None => f.write_str("SNothing"),
            Some(slot) => write!(f, "SJust (SlotNo {{unSlotNo = {slot}}})"),
        }
    }
}

/// `ValidityInterval` mirror from `Cardano.Ledger.Allegra.Scripts`
/// — a half-open transaction validity interval. CBOR wire format
/// is a 2-element record array `[invalidBefore, invalidHereafter]`
/// where each field is a `StrictMaybe SlotNo`. Display matches
/// upstream stock-derived record Show.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidityInterval {
    /// Lower bound (inclusive); `SNothing` = negative infinity.
    pub invalid_before: StrictMaybeSlot,
    /// Upper bound (exclusive); `SNothing` = positive infinity.
    pub invalid_hereafter: StrictMaybeSlot,
}

impl ValidityInterval {
    /// Decode a `ValidityInterval` from an in-progress decoder.
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let len = dec
            .array()
            .map_err(|err| DecoderError(format!("ValidityInterval: expected 2-array: {err:?}")))?;
        if len != 2 {
            return Err(DecoderError(format!(
                "ValidityInterval: expected 2-element array, got len {len}"
            )));
        }
        let invalid_before = StrictMaybeSlot::from_decoder(dec)?;
        let invalid_hereafter = StrictMaybeSlot::from_decoder(dec)?;
        Ok(Self {
            invalid_before,
            invalid_hereafter,
        })
    }
}

impl fmt::Display for ValidityInterval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ValidityInterval {{invalidBefore = {}, invalidHereafter = {}}}",
            self.invalid_before, self.invalid_hereafter
        )
    }
}

/// Plutus execution-unit budget mirroring upstream `newtype
/// ExUnits = WrapExUnits {unWrapExUnits :: ExUnits' Natural}` over
/// the record `data ExUnits' a = ExUnits' { exUnitsMem' :: a,
/// exUnitsSteps' :: a }`. CBOR wire format is a 2-element record
/// array `[mem, steps]`.
///
/// Display matches the stock-derived Show on the newtype +
/// inner record: `WrapExUnits {unWrapExUnits = ExUnits'
/// {exUnitsMem' = <n>, exUnitsSteps' = <m>}}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExUnits {
    /// Memory budget.
    pub mem: u64,
    /// CPU-step budget.
    pub steps: u64,
}

impl ExUnits {
    /// Decode an `ExUnits` from its canonical 2-element CBOR
    /// record array `[mem, steps]`.
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let len = dec
            .array()
            .map_err(|err| DecoderError(format!("ExUnits: expected 2-array: {err:?}")))?;
        if len != 2 {
            return Err(DecoderError(format!(
                "ExUnits: expected 2-element array, got len {len}"
            )));
        }
        let mem = dec
            .unsigned()
            .map_err(|err| DecoderError(format!("ExUnits: expected mem: {err:?}")))?;
        let steps = dec
            .unsigned()
            .map_err(|err| DecoderError(format!("ExUnits: expected steps: {err:?}")))?;
        Ok(Self { mem, steps })
    }
}

impl fmt::Display for ExUnits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WrapExUnits {{unWrapExUnits = ExUnits' {{exUnitsMem' = {}, exUnitsSteps' = {}}}}}",
            self.mem, self.steps
        )
    }
}

/// Minting-policy identifier mirroring upstream `newtype PolicyID
/// = PolicyID {policyID :: ScriptHash}`. A 28-byte script hash.
/// Display matches the stock-derived record Show:
/// `PolicyID {policyID = ScriptHash "<hex>"}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct PolicyId(pub [u8; 28]);

impl fmt::Display for PolicyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PolicyID {{policyID = ScriptHash \"{}\"}}",
            hex::encode(self.0)
        )
    }
}

/// Native-asset name mirroring upstream `newtype AssetName =
/// AssetName {assetNameBytes :: ShortByteString}` — a
/// variable-length byte string (≤ 32 bytes). Display matches
/// upstream `Show AssetName = show . assetNameToBytesAsHex`:
/// the hex of the bytes, quoted.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct AssetName(pub Vec<u8>);

impl fmt::Display for AssetName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{}\"", hex::encode(&self.0))
    }
}

/// Multi-asset bundle mirroring upstream `newtype MultiAsset =
/// MultiAsset (Map PolicyID (Map AssetName Integer))`. CBOR wire
/// format is a nested CBOR map `{PolicyID: {AssetName: amount}}`.
/// Entries are kept in wire order. Display matches the
/// stock-derived Show: `MultiAsset (fromList [(<PolicyID>,
/// fromList [(<AssetName>, <amount>)]), ...])`.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct MultiAsset {
    /// Per-policy asset bundles in wire order.
    pub policies: Vec<(PolicyId, Vec<(AssetName, i64)>)>,
}

impl MultiAsset {
    /// Decode a `MultiAsset` from an in-progress decoder (a
    /// nested CBOR map).
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let policy_count = dec.map().map_err(|err| {
            DecoderError(format!("MultiAsset: expected policy CBOR map: {err:?}"))
        })?;
        let mut policies = Vec::with_capacity(policy_count as usize);
        for _ in 0..policy_count {
            let pid_bytes = dec.bytes().map_err(|err| {
                DecoderError(format!("MultiAsset: expected PolicyID bytes: {err:?}"))
            })?;
            let pid: [u8; 28] = pid_bytes
                .try_into()
                .map_err(|_| DecoderError("MultiAsset: PolicyID must be 28 bytes".to_string()))?;
            let asset_count = dec.map().map_err(|err| {
                DecoderError(format!("MultiAsset: expected asset CBOR map: {err:?}"))
            })?;
            let mut assets = Vec::with_capacity(asset_count as usize);
            for _ in 0..asset_count {
                let name = dec.bytes().map_err(|err| {
                    DecoderError(format!("MultiAsset: expected AssetName bytes: {err:?}"))
                })?;
                let amount = dec.signed().map_err(|err| {
                    DecoderError(format!("MultiAsset: expected asset amount: {err:?}"))
                })?;
                assets.push((AssetName(name.to_vec()), amount));
            }
            policies.push((PolicyId(pid), assets));
        }
        Ok(Self { policies })
    }
}

impl fmt::Display for MultiAsset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MultiAsset (fromList [")?;
        let mut first_policy = true;
        for (pid, assets) in &self.policies {
            if !first_policy {
                f.write_str(",")?;
            }
            first_policy = false;
            write!(f, "({pid},fromList [")?;
            let mut first_asset = true;
            for (name, amount) in assets {
                if !first_asset {
                    f.write_str(",")?;
                }
                first_asset = false;
                write!(f, "({name},{amount})")?;
            }
            f.write_str("])")?;
        }
        f.write_str("])")
    }
}

/// Mary-era transaction value mirroring upstream `data MaryValue
/// = MaryValue !Coin !MultiAsset`. CBOR wire format is era-aware
/// (per upstream `EncCBOR MaryValue`): a bare CBOR integer when
/// the multi-asset bundle is empty (ADA-only), otherwise a
/// 2-element array `[coin, multiasset]`.
///
/// Display matches the stock-derived Show: `MaryValue (Coin <n>)
/// (<MultiAsset>)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaryValue {
    /// The ADA (lovelace) component.
    pub coin: u64,
    /// The native-asset bundle (empty for ADA-only values).
    pub assets: MultiAsset,
}

impl MaryValue {
    /// Decode a `MaryValue` from an in-progress decoder. Accepts
    /// both the bare-integer (ADA-only) and 2-element-array
    /// (with multi-asset) encodings.
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let major = dec
            .peek_major()
            .map_err(|err| DecoderError(format!("MaryValue: peek: {err:?}")))?;
        if major == 4 {
            let len = dec
                .array()
                .map_err(|err| DecoderError(format!("MaryValue: expected 2-array: {err:?}")))?;
            if len != 2 {
                return Err(DecoderError(format!(
                    "MaryValue: expected 2-element array, got len {len}"
                )));
            }
            let coin = dec
                .unsigned()
                .map_err(|err| DecoderError(format!("MaryValue: expected coin: {err:?}")))?;
            let assets = MultiAsset::from_decoder(dec)?;
            Ok(Self { coin, assets })
        } else {
            // Bare-integer ADA-only value.
            let coin = dec
                .unsigned()
                .map_err(|err| DecoderError(format!("MaryValue: expected bare coin: {err:?}")))?;
            Ok(Self {
                coin,
                assets: MultiAsset::default(),
            })
        }
    }
}

impl fmt::Display for MaryValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MaryValue ({}) ({})", CoinShow(self.coin), self.assets)
    }
}

/// `StrictMaybe ScriptIntegrityHash` renderer.
/// `ScriptIntegrityHash` is upstream `type ScriptIntegrityHash =
/// SafeHash EraIndependentScriptIntegrity` — a 32-byte SafeHash.
/// `StrictMaybe` is CBOR-encoded as a list (0-element =
/// `SNothing`, 1-element = `SJust`). Display matches upstream
/// stock-derived Show: `SNothing` / `SJust (SafeHash "<hex>")`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StrictMaybeScriptIntegrityHash(pub Option<[u8; 32]>);

impl StrictMaybeScriptIntegrityHash {
    /// Decode from an in-progress decoder (CBOR list: 0-element =
    /// SNothing, 1-element = SJust 32-byte hash).
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "StrictMaybeScriptIntegrityHash: expected CBOR list: {err:?}"
            ))
        })?;
        match len {
            0 => Ok(Self(None)),
            1 => {
                let hash_bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!(
                        "StrictMaybeScriptIntegrityHash: expected hash bytes: {err:?}"
                    ))
                })?;
                let arr: [u8; 32] = hash_bytes.try_into().map_err(|_| {
                    DecoderError(
                        "StrictMaybeScriptIntegrityHash: hash must be 32 bytes".to_string(),
                    )
                })?;
                Ok(Self(Some(arr)))
            }
            other => Err(DecoderError(format!(
                "StrictMaybeScriptIntegrityHash: expected 0- or 1-element list, got len {other}"
            ))),
        }
    }
}

impl fmt::Display for StrictMaybeScriptIntegrityHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            None => f.write_str("SNothing"),
            Some(hash) => write!(f, "SJust (SafeHash \"{}\")", hex::encode(hash)),
        }
    }
}

/// `StrictMaybe ByteString` renderer. `StrictMaybe` is
/// CBOR-encoded as a list (0-element = `SNothing`, 1-element =
/// `SJust`). Display matches upstream stock-derived Show:
/// `SNothing` / `SJust <bytestring>` where the ByteString is
/// rendered as a hex marker (cardano-submit-api does not carry
/// the full mnemonic-escape Show helper).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrictMaybeBytes(pub Option<Vec<u8>>);

impl StrictMaybeBytes {
    /// Decode a `StrictMaybe ByteString` from an in-progress
    /// decoder (CBOR list: 0-element = SNothing, 1-element =
    /// SJust bytes).
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let len = dec.array().map_err(|err| {
            DecoderError(format!("StrictMaybeBytes: expected CBOR list: {err:?}"))
        })?;
        match len {
            0 => Ok(Self(None)),
            1 => {
                let bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!("StrictMaybeBytes: expected bytes: {err:?}"))
                })?;
                Ok(Self(Some(bytes.to_vec())))
            }
            other => Err(DecoderError(format!(
                "StrictMaybeBytes: expected 0- or 1-element list, got len {other}"
            ))),
        }
    }
}

impl fmt::Display for StrictMaybeBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            None => f.write_str("SNothing"),
            Some(bytes) => write!(f, "SJust <bytestring {} bytes>", bytes.len()),
        }
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

/// 32-byte data hash mirroring upstream `type DataHash = SafeHash
/// EraIndependentData` from `Cardano.Ledger.Hashes`. As a type
/// alias over `SafeHash`, its stock Show is just the SafeHash
/// Show: `SafeHash "<hex>"`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DataHash(pub [u8; 32]);

impl fmt::Display for DataHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SafeHash \"{}\"", hex::encode(self.0))
    }
}

/// Set of 32-byte data hashes mirroring upstream `Set DataHash`.
/// CBOR shape: optional CBOR tag 258 followed by an array of
/// 32-byte byte-strings. Unlike [`NonEmptySetDataHash`], the
/// empty set is permitted (mirrors `Set` rather than
/// `NonEmptySet`).
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct SetDataHash {
    /// Decoded set entries (possibly empty).
    pub entries: std::collections::BTreeSet<DataHash>,
}

/// Decode a `Set`/`NonEmptySet` of 32-byte data hashes from an
/// in-progress decoder, accepting the optional tag-258 prefix.
fn decode_data_hash_set(
    dec: &mut yggdrasil_ledger::Decoder<'_>,
    label: &str,
) -> Result<std::collections::BTreeSet<DataHash>, DecoderError> {
    let major = dec
        .peek_major()
        .map_err(|err| DecoderError(format!("{label}: peek: {err:?}")))?;
    if major == 6 {
        let tag = dec
            .tag()
            .map_err(|err| DecoderError(format!("{label}: tag: {err:?}")))?;
        if tag != 258 {
            return Err(DecoderError(format!(
                "{label}: expected tag 258, got {tag}"
            )));
        }
    }
    let count = dec
        .array()
        .map_err(|err| DecoderError(format!("{label}: expected CBOR array: {err:?}")))?;
    let mut entries = std::collections::BTreeSet::new();
    for _ in 0..count {
        let hash_bytes = dec
            .bytes()
            .map_err(|err| DecoderError(format!("{label}: expected DataHash bytes: {err:?}")))?;
        let arr: [u8; 32] = hash_bytes
            .try_into()
            .map_err(|_| DecoderError(format!("{label}: DataHash must be 32 bytes")))?;
        entries.insert(DataHash(arr));
    }
    Ok(entries)
}

impl SetDataHash {
    /// Decode a `Set DataHash` from an in-progress decoder.
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        Ok(Self {
            entries: decode_data_hash_set(dec, "SetDataHash")?,
        })
    }
}

impl fmt::Display for SetDataHash {
    /// Render upstream `Show (Set DataHash)`: `fromList [...]`.
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
        f.write_str("]")
    }
}

/// Non-empty set of 32-byte data hashes mirroring upstream
/// `NonEmptySet DataHash`. Empty sets are rejected at decode
/// time to honour the NonEmpty invariant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptySetDataHash {
    /// Decoded set entries. Guaranteed non-empty by `from_decoder`.
    pub entries: std::collections::BTreeSet<DataHash>,
}

impl NonEmptySetDataHash {
    /// Decode a `NonEmptySet DataHash` from an in-progress
    /// decoder.
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let entries = decode_data_hash_set(dec, "NonEmptySetDataHash")?;
        if entries.is_empty() {
            return Err(DecoderError(
                "NonEmptySetDataHash: NonEmptySet requires at least one entry".to_string(),
            ));
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptySetDataHash {
    /// Render upstream `Show (NonEmptySet DataHash)`:
    /// `NonEmptySet (fromList [...])`.
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
        f.write_str("])")
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
                let outs = NonEmptyTxOut::from_decoder(&mut dec)?;
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

/// Non-empty list of transaction inputs mirroring upstream
/// `NonEmpty TxIn` (`Data.List.NonEmpty`). Unlike
/// [`NonEmptySetTxIn`], this preserves wire order and permits
/// duplicates — the CBOR wire format is a plain CBOR array (no
/// tag-258 prefix). Empty arrays are rejected at decode time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptyTxIn {
    /// Decoded entries in wire order. Guaranteed non-empty by
    /// `from_cbor`.
    pub entries: Vec<TxIn>,
}

impl NonEmptyTxIn {
    /// Decode a `NonEmpty TxIn` from canonical CBOR bytes.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let count = dec
            .array()
            .map_err(|err| DecoderError(format!("NonEmptyTxIn: expected CBOR array: {err:?}")))?;
        if count == 0 {
            return Err(DecoderError(
                "NonEmptyTxIn: NonEmpty requires at least one entry".to_string(),
            ));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(TxIn::from_decoder(&mut dec)?);
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptyTxIn {
    /// Render upstream `Show (NonEmpty TxIn)`: `<head> :| [<tail>...]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (head, tail) = self
            .entries
            .split_first()
            .expect("NonEmptyTxIn enforces ≥1 entry at decode time");
        write!(f, "{head} :| [")?;
        let mut first = true;
        for t in tail {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "{t}")?;
        }
        f.write_str("]")
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
/// CBOR wire format is a single bytestring whose first byte encodes
/// the address type in the high nibble (per upstream `putAddr`):
///
/// | Header bits | Type | Body |
/// |-------------|------|------|
/// | `0000_NNNN` | Base addr (key/key) | 28-byte payment hash + 28-byte stake hash |
/// | `0001_NNNN` | Base addr (script/key) | 28+28 |
/// | `0010_NNNN` | Base addr (key/script) | 28+28 |
/// | `0011_NNNN` | Base addr (script/script) | 28+28 |
/// | `0100_NNNN` | Pointer (key) | 28 + variable Ptr |
/// | `0101_NNNN` | Pointer (script) | 28 + variable Ptr |
/// | `0110_NNNN` | Enterprise (key) | 28 |
/// | `0111_NNNN` | Enterprise (script) | 28 |
/// | `1xxx_xxxx` | Byron bootstrap | variable |
///
/// `NNNN` is the network id (Testnet=0, Mainnet=1).
///
/// The struct stores the raw on-wire bytes; `Display` parses the
/// header byte and renders the typed structure (base/pointer/
/// enterprise/bootstrap) matching upstream's stock-derived Show.
/// Pointer-address typed body decoding (variable-length encoded
/// `Ptr` values) is deferred — pointer addresses render with a
/// typed payment credential plus a raw hex pointer tail.
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

    /// Network ID extracted from the address header byte (low
    /// nibble for Shelley addresses; for Byron bootstrap, returns
    /// Mainnet as a default — upstream `getNetwork` inspects the
    /// Byron attribute payload, which yggdrasil doesn't yet
    /// decode).
    fn network_from_header(&self) -> Network {
        let header = self.0[0];
        if header & 0x80 != 0 {
            // Byron bootstrap — full Byron addr decode required to
            // recover the network attribute. Default to Mainnet
            // for the marker render.
            Network::Mainnet
        } else if header & 0x0F == 0 {
            Network::Testnet
        } else {
            Network::Mainnet
        }
    }
}

impl fmt::Display for Addr {
    /// Render the typed Shelley / Bootstrap address shape matching
    /// upstream's stock-derived `Show Addr`:
    /// - Shelley: `Addr <Network> (<PaymentCredential>) (<StakeReference>)`.
    /// - Bootstrap: `AddrBootstrap <hex N bytes>` (full Byron
    ///   typed parse pending).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("Addr <empty>");
        }
        let header = self.0[0];
        if header & 0x80 != 0 {
            // Byron bootstrap — typed split pending.
            return write!(
                f,
                "AddrBootstrap <hex {} bytes: {}>",
                self.0.len(),
                hex::encode(&self.0)
            );
        }
        let payment_is_script = header & 0x10 != 0;
        let payment_label = if payment_is_script {
            "ScriptHashObj"
        } else {
            "KeyHashObj"
        };
        let payment_hash_label = if payment_is_script {
            "ScriptHash"
        } else {
            "KeyHash"
        };
        let network = self.network_from_header();
        // Body starts at byte 1.
        let body = &self.0[1..];
        if body.len() < 28 {
            return write!(
                f,
                "Addr <malformed: header {:#04X}, body {} bytes>",
                header,
                body.len()
            );
        }
        let pay_hex = hex::encode(&body[..28]);
        let payment = if payment_is_script {
            format!("{payment_label} ({payment_hash_label} \"{pay_hex}\")")
        } else {
            format!("{payment_label} ({payment_hash_label} {{unKeyHash = \"{pay_hex}\"}})")
        };
        let type_bits = header & 0xF0;
        let stake_ref_render = match type_bits {
            0x00 | 0x10 => {
                // Base address — 28-byte stake-key credential.
                if body.len() < 56 {
                    return write!(f, "Addr {network} ({payment}) <truncated base stake hash>");
                }
                let stake_hex = hex::encode(&body[28..56]);
                format!("StakeRefBase (KeyHashObj (KeyHash {{unKeyHash = \"{stake_hex}\"}}))")
            }
            0x20 | 0x30 => {
                if body.len() < 56 {
                    return write!(
                        f,
                        "Addr {network} ({payment}) <truncated base stake script hash>"
                    );
                }
                let stake_hex = hex::encode(&body[28..56]);
                format!("StakeRefBase (ScriptHashObj (ScriptHash \"{stake_hex}\"))")
            }
            0x40 | 0x50 => {
                // Pointer address — variable-length Ptr tail (3
                // VLQ-encoded Word64s: slot, tx_ix, cert_ix per
                // upstream `putPtr`).
                match decode_addr_ptr(&body[28..]) {
                    Some((slot, tx_ix, cert_ix)) => {
                        format!(
                            "StakeRefPtr (Ptr (SlotNo32 {slot}) (TxIx {{unTxIx = {tx_ix}}}) (CertIx {{unCertIx = {cert_ix}}}))"
                        )
                    }
                    None => {
                        let ptr_hex = hex::encode(&body[28..]);
                        format!(
                            "StakeRefPtr <malformed-ptr hex {} bytes: {ptr_hex}>",
                            body.len() - 28
                        )
                    }
                }
            }
            0x60 | 0x70 => "StakeRefNull".to_string(),
            other => format!("<unknown stake type {other:#04X}>"),
        };
        write!(f, "Addr {network} ({payment}) ({stake_ref_render})")
    }
}

/// Decode a Cardano pointer-address tail into `(slot, tx_ix,
/// cert_ix)`. The encoding is upstream's variable-length Word64
/// per `putVariableLengthWord64`: each byte contributes 7 data
/// bits MSB-first; the high bit is a continuation flag (1 = more
/// bytes follow). Returns `None` when the tail is malformed or
/// truncated.
fn decode_addr_ptr(tail: &[u8]) -> Option<(u64, u64, u64)> {
    let mut cursor = 0;
    let slot = decode_addr_vlq_word64(tail, &mut cursor)?;
    let tx_ix = decode_addr_vlq_word64(tail, &mut cursor)?;
    let cert_ix = decode_addr_vlq_word64(tail, &mut cursor)?;
    Some((slot, tx_ix, cert_ix))
}

fn decode_addr_vlq_word64(bytes: &[u8], cursor: &mut usize) -> Option<u64> {
    let mut value: u64 = 0;
    // Up to 10 bytes (7 bits each = 70 bits) suffice for Word64;
    // upstream's encoder always emits at most 10 bytes for the
    // 64-bit case.
    for _ in 0..10 {
        let byte = *bytes.get(*cursor)?;
        *cursor += 1;
        value = value.checked_shl(7)?.checked_add(u64::from(byte & 0x7F))?;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
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

/// Shelley-era transaction-output mirroring upstream
/// `data ShelleyTxOut era` (Shelley/Allegra/Mary). Wire format is
/// the canonical 2-element CBOR array `[address_bytes, coin]`
/// where for Shelley era `Value = Coin = Word64`.
///
/// Upstream `Show ShelleyTxOut = show . viewCompactTxOut` renders
/// as a Haskell tuple `(<Addr>, Coin <n>)` — the `viewCompactTxOut`
/// helper returns `(Addr, Value)` and the tuple Show wraps each
/// element in its own Show.
///
/// Alonzo (3-array) and Babbage+ (CBOR map) outputs are not yet
/// covered — the `ShelleyUtxoPredFailure` enum is era-tagged at
/// the outer envelope so its variant payloads inherit Shelley-era
/// shape.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ShelleyTxOut {
    /// Output address bytes (29-byte tagged form for Shelley).
    pub addr: Addr,
    /// Output value (Coin = Word64 for Shelley era).
    pub coin: u64,
}

impl ShelleyTxOut {
    /// Decode a single Shelley-era TxOut as the canonical
    /// 2-element CBOR array `[bytes(addr), coin]`.
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let len = dec
            .array()
            .map_err(|err| DecoderError(format!("ShelleyTxOut: expected 2-array: {err:?}")))?;
        if len != 2 {
            return Err(DecoderError(format!(
                "ShelleyTxOut: expected 2-array, got len {len}"
            )));
        }
        let addr = Addr::from_decoder(dec)
            .map_err(|err| DecoderError(format!("ShelleyTxOut: {}", err.0)))?;
        let coin = dec
            .unsigned()
            .map_err(|err| DecoderError(format!("ShelleyTxOut: expected coin: {err:?}")))?;
        Ok(Self { addr, coin })
    }
}

impl fmt::Display for ShelleyTxOut {
    /// Render upstream `Show ShelleyTxOut = show .
    /// viewCompactTxOut`: `(<Addr>, Coin <coin>)`. The Haskell tuple
    /// Show inlines each element at p=0 with comma separation;
    /// the outer parens are part of the tuple syntax.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.addr, CoinShow(self.coin))
    }
}

/// Non-empty list of `(TxOut, Coin)` pairs mirroring upstream
/// `NonEmpty (TxOut era, Coin)` — the payload of
/// `BabbageOutputTooSmallUTxO`. Each pair binds a Shelley-era
/// output to its minimum-value requirement. CBOR wire format is
/// a regular CBOR array of 2-element `[TxOut, Coin]` arrays;
/// empty arrays are rejected at decode time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptyTxOutCoinPair {
    /// Decoded `(output, min-value)` pairs in wire order.
    /// Guaranteed non-empty by `from_cbor`.
    pub entries: Vec<(ShelleyTxOut, u64)>,
}

impl NonEmptyTxOutCoinPair {
    /// Decode a `NonEmpty (TxOut, Coin)` from canonical CBOR
    /// bytes.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let count = dec.array().map_err(|err| {
            DecoderError(format!(
                "NonEmptyTxOutCoinPair: expected CBOR array: {err:?}"
            ))
        })?;
        if count == 0 {
            return Err(DecoderError(
                "NonEmptyTxOutCoinPair: NonEmpty requires at least one entry".to_string(),
            ));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let pair_len = dec.array().map_err(|err| {
                DecoderError(format!(
                    "NonEmptyTxOutCoinPair: expected 2-element pair: {err:?}"
                ))
            })?;
            if pair_len != 2 {
                return Err(DecoderError(format!(
                    "NonEmptyTxOutCoinPair: expected 2-element pair, got len {pair_len}"
                )));
            }
            let tx_out = ShelleyTxOut::from_decoder(&mut dec)?;
            let coin = dec.unsigned().map_err(|err| {
                DecoderError(format!("NonEmptyTxOutCoinPair: expected Coin: {err:?}"))
            })?;
            entries.push((tx_out, coin));
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptyTxOutCoinPair {
    /// Render upstream `Show (NonEmpty (TxOut, Coin))`:
    /// `<head> :| [<tail>...]` where each pair is a Haskell tuple
    /// `(<TxOut>, Coin <n>)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (head, tail) = self
            .entries
            .split_first()
            .expect("NonEmptyTxOutCoinPair enforces ≥1 entry at decode time");
        write!(f, "({}, {}) :| [", head.0, CoinShow(head.1))?;
        let mut first = true;
        for (tx_out, coin) in tail {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "({tx_out}, {})", CoinShow(*coin))?;
        }
        f.write_str("]")
    }
}

/// Non-empty map from transaction inputs to outputs mirroring
/// upstream `NonEmptyMap TxIn (TxOut era)` (`newtype NonEmptyMap
/// k v = NonEmptyMap (Map k v)`). CBOR wire format is a CBOR map
/// (`TxIn` key → Shelley `TxOut` value); empty maps are rejected
/// at decode time. Entries are kept in wire order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptyMapTxInTxOut {
    /// Decoded `(input, output)` entries in wire order.
    /// Guaranteed non-empty by `from_cbor`.
    pub entries: Vec<(TxIn, ShelleyTxOut)>,
}

impl NonEmptyMapTxInTxOut {
    /// Decode a `NonEmptyMap TxIn (TxOut era)` from canonical
    /// CBOR bytes (a CBOR map).
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let count = dec.map().map_err(|err| {
            DecoderError(format!("NonEmptyMapTxInTxOut: expected CBOR map: {err:?}"))
        })?;
        if count == 0 {
            return Err(DecoderError(
                "NonEmptyMapTxInTxOut: NonEmptyMap requires at least one entry".to_string(),
            ));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let tx_in = TxIn::from_decoder(&mut dec)?;
            let tx_out = ShelleyTxOut::from_decoder(&mut dec)?;
            entries.push((tx_in, tx_out));
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptyMapTxInTxOut {
    /// Render upstream stock-derived `Show (NonEmptyMap k v)`:
    /// `NonEmptyMap (fromList [(<k>, <v>), ...])`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NonEmptyMap (fromList [")?;
        let mut first = true;
        for (tx_in, tx_out) in &self.entries {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "({tx_in}, {tx_out})")?;
        }
        f.write_str("])")
    }
}

/// Non-empty list of transaction outputs mirroring upstream
/// `NonEmpty (TxOut era)`. CBOR wire format is a regular CBOR
/// array with ≥1 entry. NonEmpty invariant enforced at decode time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptyTxOut {
    /// Decoded Shelley-era entries (typed Addr + Coin). Guaranteed
    /// non-empty by `from_cbor` / `from_decoder`.
    pub entries: Vec<ShelleyTxOut>,
}

impl NonEmptyTxOut {
    /// Decode `NonEmpty (TxOut era)` from canonical CBOR bytes
    /// using Shelley-era TxOut shape.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        Self::from_decoder(&mut dec)
    }

    /// Decode from an in-progress `Decoder` (Shelley-era TxOut).
    pub fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
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
            entries.push(ShelleyTxOut::from_decoder(dec)?);
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

/// `ShelleyDelegsPredFailure` mirror — DELEGS sub-rule under
/// `ShelleyLedgerPredFailure::DelegsFailure` (LEDGER tag 1).
///
/// Upstream: `newtype ShelleyDelegsPredFailure era = DelplFailure
/// (PredicateFailure (EraRule "DELPL" era))` from
/// `Cardano.Ledger.Shelley.Rules.Delegs`. CBOR wire format wraps the
/// single variant in a 2-element array `[1, DELPL-failure]`.
///
/// R612 ships the scaffold with the single variant carrying raw
/// inner CBOR. The nested DELPL sub-rule decoder
/// (`ShelleyDelplPredFailure` — itself dispatching into
/// POOL/DELEG) lands in a follow-on round.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShelleyDelegsPredFailure {
    /// Tag 1: nested DELPL sub-rule failure (R613 wired to the
    /// typed `ShelleyDelplPredFailure` scaffold).
    DelplFailure(ShelleyDelplPredFailure),
}

impl ShelleyDelegsPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::DelplFailure(_) => 1,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::DelplFailure(_) => "DelplFailure",
        }
    }

    /// Decode the full `ShelleyDelegsPredFailure` outer envelope
    /// from CBOR bytes. Upstream encoding wraps the single
    /// `DelplFailure` variant in a CBOR list `[1, DELPL-failure]`.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ShelleyDelegsPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if len != 2 {
            return Err(DecoderError(format!(
                "ShelleyDelegsPredFailure: expected 2-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ShelleyDelegsPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        let payload_offset = dec.position();
        match tag {
            1 => {
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError(
                        "ShelleyDelegsPredFailure: payload offset out of bounds".to_string(),
                    )
                })?;
                let delpl = ShelleyDelplPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::DelplFailure(delpl))
            }
            other => Err(DecoderError(format!(
                "ShelleyDelegsPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ShelleyDelegsPredFailure {
    /// Render upstream stock-derived `Show
    /// (ShelleyDelegsPredFailure era)`: `<Constructor>
    /// (<inner-DELPL>)` — R613 wires the typed DELPL payload.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DelplFailure(delpl) => write!(f, "DelplFailure ({delpl})"),
        }
    }
}

/// `ShelleyDelplPredFailure` mirror — nested sub-rule under
/// `ShelleyDelegsPredFailure::DelplFailure`.
///
/// Upstream: `data ShelleyDelplPredFailure era` from
/// `Cardano.Ledger.Shelley.Rules.Delpl` with 2 variants:
///
/// ```text
/// data ShelleyDelplPredFailure era
///   = PoolFailure (PredicateFailure (EraRule "POOL" era))
///   | DelegFailure (PredicateFailure (EraRule "DELEG" era))
/// ```
///
/// CBOR wire format wraps each variant in a 2-element array
/// `[tag, payload]`: tag 0 = PoolFailure, tag 1 = DelegFailure.
///
/// R613 ships the scaffold with both variants carrying raw inner
/// CBOR. The POOL/DELEG sub-rule decoders land in follow-on
/// rounds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShelleyDelplPredFailure {
    /// Tag 0: nested POOL sub-rule failure (R614 wired to typed
    /// `ShelleyPoolPredFailure`).
    PoolFailure(ShelleyPoolPredFailure),
    /// Tag 1: nested DELEG sub-rule failure (R615 wired to typed
    /// `ShelleyDelegPredFailure`).
    DelegFailure(ShelleyDelegPredFailure),
}

impl ShelleyDelplPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::PoolFailure(_) => 0,
            Self::DelegFailure(_) => 1,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::PoolFailure(_) => "PoolFailure",
            Self::DelegFailure(_) => "DelegFailure",
        }
    }

    /// Decode the full `ShelleyDelplPredFailure` outer envelope
    /// from CBOR bytes. Upstream encoding wraps each variant in a
    /// CBOR list `[tag, payload]`.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ShelleyDelplPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if len != 2 {
            return Err(DecoderError(format!(
                "ShelleyDelplPredFailure: expected 2-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ShelleyDelplPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        let payload_offset = dec.position();
        let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
            DecoderError("ShelleyDelplPredFailure: payload offset out of bounds".to_string())
        })?;
        match tag {
            // Tag 0: typed POOL sub-rule (R614).
            0 => {
                let pool = ShelleyPoolPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::PoolFailure(pool))
            }
            // Tag 1: typed DELEG sub-rule (R615).
            1 => {
                let deleg = ShelleyDelegPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::DelegFailure(deleg))
            }
            other => Err(DecoderError(format!(
                "ShelleyDelplPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ShelleyDelplPredFailure {
    /// Render upstream stock-derived `Show
    /// (ShelleyDelplPredFailure era)`: `<Constructor> <payload>`.
    /// PoolFailure routes through the typed POOL Display (R614);
    /// DelegFailure emits a raw-cbor marker pending its decoder.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PoolFailure(pool) => write!(f, "PoolFailure ({pool})"),
            Self::DelegFailure(deleg) => write!(f, "DelegFailure ({deleg})"),
        }
    }
}

/// `ShelleyPoolPredFailure` mirror — nested sub-rule under
/// `ShelleyDelplPredFailure::PoolFailure`.
///
/// Upstream: `data ShelleyPoolPredFailure era` from
/// `Cardano.Ledger.Shelley.Rules.Pool` with 6 variants encoded
/// via CBOR `Sum` tags 0/1/3/4/5/6 (tag 2 is skipped — upstream
/// reserved for a retired variant). R614 ships the enum + outer
/// envelope decoder; only the simplest tag-0
/// `StakePoolNotRegisteredOnKeyPOOL` carries a typed payload
/// (`KeyHash`). Tags 1/3/4/5/6 carry raw inner CBOR pending the
/// Mismatch-RelGT/Mismatch-RelGTEQ/Mismatch-RelEQ + KeyHash-Int
/// per-variant payload ports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShelleyPoolPredFailure {
    /// Tag 0: pool-key-hash not registered (R614 typed payload).
    StakePoolNotRegisteredOnKeyPOOL(KeyHash),
    /// Tag 1: pool retirement targets wrong epoch — payload is
    /// `(Mismatch RelGT EpochNo, Mismatch RelLTEQ EpochNo)` per
    /// upstream, flattened on the wire to 3 EpochNos
    /// `[1, gtExpected, ltSupplied, ltExpected]` because the
    /// two Mismatches share the same `supplied` field (R619 typed).
    StakePoolRetirementWrongEpochPOOL {
        /// The operator's submitted retirement epoch (shared
        /// `supplied` across both Mismatches).
        supplied: u64,
        /// First Mismatch's `expected` — supplied must be `>`
        /// this current epoch (RelGT relation).
        gt_expected: u64,
        /// Second Mismatch's `expected` — supplied must be `≤`
        /// this max retirement epoch (RelLTEQ relation).
        lt_expected: u64,
    },
    /// Tag 3: pool cost below minimum — `Mismatch RelGTEQ Coin`
    /// encoded as 3-element `[3, supplied, expected]` (R616 typed).
    StakePoolCostTooLowPOOL(Mismatch<u64>),
    /// Tag 4: pool registered on wrong network — `Mismatch RelEQ
    /// Network` + `KeyHash StakePool` encoded as 4-element
    /// `[4, expected, supplied, kh]` (R616 typed).
    WrongNetworkPOOL {
        /// Expected (ledger) network.
        expected: Network,
        /// Supplied (operator) network.
        supplied: Network,
        /// Pool ID with the wrong network.
        pool_id: KeyHash,
    },
    /// Tag 5: pool metadata hash too big — `KeyHash + Int` encoded
    /// as 3-element `[5, kh, size]` (R616 typed; Int narrowed to
    /// u32 at decode time).
    PoolMedataHashTooBig {
        /// Pool ID.
        pool_id: KeyHash,
        /// Size of the offending metadata hash in bytes.
        size: u32,
    },
    /// Tag 6: VRF key hash already registered — `KeyHash StakePool` +
    /// `VRFVerKeyHash StakePoolVRF` encoded as 3-element `[6, kh,
    /// vrfkh]` (R616 typed).
    VRFKeyHashAlreadyRegistered {
        /// Pool ID.
        pool_id: KeyHash,
        /// The VRF key hash that is already registered.
        vrf_key_hash: VrfVerKeyHash,
    },
}

impl ShelleyPoolPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::StakePoolNotRegisteredOnKeyPOOL(_) => 0,
            Self::StakePoolRetirementWrongEpochPOOL { .. } => 1,
            Self::StakePoolCostTooLowPOOL(_) => 3,
            Self::WrongNetworkPOOL { .. } => 4,
            Self::PoolMedataHashTooBig { .. } => 5,
            Self::VRFKeyHashAlreadyRegistered { .. } => 6,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::StakePoolNotRegisteredOnKeyPOOL(_) => "StakePoolNotRegisteredOnKeyPOOL",
            Self::StakePoolRetirementWrongEpochPOOL { .. } => "StakePoolRetirementWrongEpochPOOL",
            Self::StakePoolCostTooLowPOOL(_) => "StakePoolCostTooLowPOOL",
            Self::WrongNetworkPOOL { .. } => "WrongNetworkPOOL",
            Self::PoolMedataHashTooBig { .. } => "PoolMedataHashTooBig",
            Self::VRFKeyHashAlreadyRegistered { .. } => "VRFKeyHashAlreadyRegistered",
        }
    }

    /// Decode the full `ShelleyPoolPredFailure` outer envelope
    /// from CBOR bytes. Upstream encoding wraps each variant in a
    /// CBOR list whose first element is the Word8 tag and
    /// remaining elements are payload parts (envelope length
    /// varies per variant: 2, 4, 3, 4, 3, 3 for tags 0/1/3/4/5/6).
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ShelleyPoolPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if !(2..=4).contains(&len) {
            return Err(DecoderError(format!(
                "ShelleyPoolPredFailure: expected 2- to 4-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ShelleyPoolPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        match tag {
            // Tag 0: typed KeyHash payload.
            0 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "StakePoolNotRegisteredOnKeyPOOL: expected 2-element envelope, got len {len}"
                    )));
                }
                let hash_bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!(
                        "StakePoolNotRegisteredOnKeyPOOL: expected KeyHash bytes: {err:?}"
                    ))
                })?;
                let arr: [u8; 28] = hash_bytes.try_into().map_err(|_| {
                    DecoderError(format!(
                        "StakePoolNotRegisteredOnKeyPOOL: KeyHash must be 28 bytes, got {}",
                        hash_bytes.len()
                    ))
                })?;
                Ok(Self::StakePoolNotRegisteredOnKeyPOOL(KeyHash(arr)))
            }
            // Tag 1: 4-element envelope `[1, gtExpected,
            // ltSupplied, ltExpected]` — flattened pair of
            // Mismatch EpochNo per upstream's bespoke encoding.
            // Raw payload pending dedicated decoder (would need to
            // reconstruct two Mismatches with shared supplied
            // field).
            1 => {
                if len != 4 {
                    return Err(DecoderError(format!(
                        "StakePoolRetirementWrongEpochPOOL: expected 4-element envelope, got len {len}"
                    )));
                }
                let gt_expected = dec.unsigned().map_err(|err| {
                    DecoderError(format!(
                        "StakePoolRetirementWrongEpochPOOL: gtExpected: {err:?}"
                    ))
                })?;
                let supplied = dec.unsigned().map_err(|err| {
                    DecoderError(format!(
                        "StakePoolRetirementWrongEpochPOOL: ltSupplied: {err:?}"
                    ))
                })?;
                let lt_expected = dec.unsigned().map_err(|err| {
                    DecoderError(format!(
                        "StakePoolRetirementWrongEpochPOOL: ltExpected: {err:?}"
                    ))
                })?;
                Ok(Self::StakePoolRetirementWrongEpochPOOL {
                    supplied,
                    gt_expected,
                    lt_expected,
                })
            }
            // Tag 3: 3-element envelope `[3, supplied, expected]`
            // — `Mismatch RelGTEQ Coin` (R616 typed).
            3 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "StakePoolCostTooLowPOOL: expected 3-element envelope, got len {len}"
                    )));
                }
                let supplied = dec.unsigned().map_err(|err| {
                    DecoderError(format!("StakePoolCostTooLowPOOL: supplied: {err:?}"))
                })?;
                let expected = dec.unsigned().map_err(|err| {
                    DecoderError(format!("StakePoolCostTooLowPOOL: expected: {err:?}"))
                })?;
                Ok(Self::StakePoolCostTooLowPOOL(Mismatch {
                    relation: MismatchRelation::RelGTEQ,
                    supplied,
                    expected,
                }))
            }
            // Tag 4: 4-element envelope `[4, expected, supplied,
            // kh]` — `Mismatch RelEQ Network + KeyHash StakePool`
            // (R616 typed).
            4 => {
                if len != 4 {
                    return Err(DecoderError(format!(
                        "WrongNetworkPOOL: expected 4-element envelope, got len {len}"
                    )));
                }
                let expected = Network::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("WrongNetworkPOOL: {}", err.0)))?;
                let supplied = Network::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("WrongNetworkPOOL: {}", err.0)))?;
                let kh_bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!("WrongNetworkPOOL: expected KeyHash bytes: {err:?}"))
                })?;
                let arr: [u8; 28] = kh_bytes.try_into().map_err(|_| {
                    DecoderError(format!(
                        "WrongNetworkPOOL: KeyHash must be 28 bytes, got {}",
                        kh_bytes.len()
                    ))
                })?;
                Ok(Self::WrongNetworkPOOL {
                    expected,
                    supplied,
                    pool_id: KeyHash(arr),
                })
            }
            // Tag 5: 3-element envelope `[5, kh, size]` — `KeyHash
            // + Int` (R616 typed; Int narrowed to u32).
            5 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "PoolMedataHashTooBig: expected 3-element envelope, got len {len}"
                    )));
                }
                let kh_bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!(
                        "PoolMedataHashTooBig: expected KeyHash bytes: {err:?}"
                    ))
                })?;
                let arr: [u8; 28] = kh_bytes.try_into().map_err(|_| {
                    DecoderError(format!(
                        "PoolMedataHashTooBig: KeyHash must be 28 bytes, got {}",
                        kh_bytes.len()
                    ))
                })?;
                let size = dec
                    .unsigned()
                    .map_err(|err| DecoderError(format!("PoolMedataHashTooBig: size: {err:?}")))?;
                let size = u32::try_from(size).map_err(|_| {
                    DecoderError(format!(
                        "PoolMedataHashTooBig: size {size} does not fit Word32"
                    ))
                })?;
                Ok(Self::PoolMedataHashTooBig {
                    pool_id: KeyHash(arr),
                    size,
                })
            }
            // Tag 6: 3-element envelope `[6, kh, vrfkh]` —
            // `KeyHash + VRFVerKeyHash` (R616 typed).
            6 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "VRFKeyHashAlreadyRegistered: expected 3-element envelope, got len {len}"
                    )));
                }
                let kh_bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!(
                        "VRFKeyHashAlreadyRegistered: expected KeyHash bytes: {err:?}"
                    ))
                })?;
                let kh_arr: [u8; 28] = kh_bytes.try_into().map_err(|_| {
                    DecoderError(format!(
                        "VRFKeyHashAlreadyRegistered: KeyHash must be 28 bytes, got {}",
                        kh_bytes.len()
                    ))
                })?;
                let vrf_bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!(
                        "VRFKeyHashAlreadyRegistered: expected VRFVerKeyHash bytes: {err:?}"
                    ))
                })?;
                let vrf_arr: [u8; 32] = vrf_bytes.try_into().map_err(|_| {
                    DecoderError(format!(
                        "VRFKeyHashAlreadyRegistered: VRFVerKeyHash must be 32 bytes, got {}",
                        vrf_bytes.len()
                    ))
                })?;
                Ok(Self::VRFKeyHashAlreadyRegistered {
                    pool_id: KeyHash(kh_arr),
                    vrf_key_hash: VrfVerKeyHash(vrf_arr),
                })
            }
            other => Err(DecoderError(format!(
                "ShelleyPoolPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ShelleyPoolPredFailure {
    /// Render upstream stock-derived `Show
    /// (ShelleyPoolPredFailure era)`: `<Constructor> <payload>`.
    /// Tag 0 routes through typed `KeyHash` Display; other
    /// variants emit a raw-cbor marker.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StakePoolNotRegisteredOnKeyPOOL(kh) => {
                write!(f, "StakePoolNotRegisteredOnKeyPOOL ({kh})")
            }
            Self::StakePoolRetirementWrongEpochPOOL {
                supplied,
                gt_expected,
                lt_expected,
            } => {
                let gt = Mismatch {
                    relation: MismatchRelation::RelGT,
                    supplied: *supplied,
                    expected: *gt_expected,
                };
                let lt = Mismatch {
                    relation: MismatchRelation::RelLTEQ,
                    supplied: *supplied,
                    expected: *lt_expected,
                };
                write!(f, "StakePoolRetirementWrongEpochPOOL ({gt}) ({lt})")
            }
            Self::StakePoolCostTooLowPOOL(mm) => {
                let typed = Mismatch {
                    relation: mm.relation,
                    supplied: CoinShow(mm.supplied),
                    expected: CoinShow(mm.expected),
                };
                write!(f, "StakePoolCostTooLowPOOL ({typed})")
            }
            Self::WrongNetworkPOOL {
                expected,
                supplied,
                pool_id,
            } => {
                let typed = Mismatch {
                    relation: MismatchRelation::RelEQ,
                    supplied: *supplied,
                    expected: *expected,
                };
                write!(f, "WrongNetworkPOOL ({typed}) ({pool_id})")
            }
            Self::PoolMedataHashTooBig { pool_id, size } => {
                write!(f, "PoolMedataHashTooBig ({pool_id}) {size}")
            }
            Self::VRFKeyHashAlreadyRegistered {
                pool_id,
                vrf_key_hash,
            } => {
                write!(
                    f,
                    "VRFKeyHashAlreadyRegistered ({pool_id}) ({vrf_key_hash})"
                )
            }
        }
    }
}

/// `ShelleyDelegPredFailure` mirror — nested sub-rule under
/// `ShelleyDelplPredFailure::DelegFailure` (DELPL tag 1).
///
/// Upstream: `data ShelleyDelegPredFailure era` from
/// `Cardano.Ledger.Shelley.Rules.Deleg` with 16 variants encoded
/// via CBOR `Sum` tags 0..9 + 11..16 (tag 10 deliberately
/// skipped per upstream's `encCBOR`).
///
/// R615 ships the enum + decoders for the simplest variants
/// (no-payload tags 4/11/12/14 + tag-2 Coin + KeyHash tags
/// 5/6/16 + VRFVerKeyHash tag 9). Variants carrying Credential
/// (0/1/3), MIRPot + Mismatch/Coin (7/13/15), and SlotNo
/// Mismatch (8) keep raw payloads pending typed decoders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShelleyDelegPredFailure {
    /// Tag 0: stake-key credential already registered (R618
    /// typed via [`Credential`]).
    StakeKeyAlreadyRegisteredDELEG(Credential),
    /// Tag 1: stake-key credential not registered (R618 typed).
    StakeKeyNotRegisteredDELEG(Credential),
    /// Tag 2: stake-key has non-zero account balance — `Coin`
    /// (R615 typed).
    StakeKeyNonZeroAccountBalanceDELEG(u64),
    /// Tag 3: stake-key credential not registered for delegation
    /// (R618 typed).
    StakeDelegationImpossibleDELEG(Credential),
    /// Tag 4: wrong cert type — no payload.
    WrongCertificateTypeDELEG,
    /// Tag 5: genesis key not in mapping — `KeyHash GenesisRole`
    /// (R615 typed via [`KeyHash`] — phantom role doesn't affect
    /// wire format).
    GenesisKeyNotInMappingDELEG(KeyHash),
    /// Tag 6: duplicate genesis delegate — `KeyHash
    /// GenesisDelegate` (R615 typed).
    DuplicateGenesisDelegateDELEG(KeyHash),
    /// Tag 7: insufficient instantaneous rewards — `MIRPot +
    /// Mismatch RelLTEQ Coin` (R617 typed).
    InsufficientForInstantaneousRewardsDELEG {
        /// Which pot the rewards were drawn from.
        pot: MirPot,
        /// Supplied vs expected coin mismatch.
        mismatch: Mismatch<u64>,
    },
    /// Tag 8: MIR cert too late in epoch — `Mismatch RelLT
    /// SlotNo` (R617 typed).
    MIRCertificateTooLateinEpochDELEG(Mismatch<u64>),
    /// Tag 9: duplicate genesis VRF — `VRFVerKeyHash GenDelegVRF`
    /// (32 bytes, R615 typed via raw bytes wrapper).
    DuplicateGenesisVRFDELEG(VrfVerKeyHash),
    // Tag 10 deliberately skipped per upstream.
    /// Tag 11: MIR transfer not currently allowed — no payload.
    MIRTransferNotCurrentlyAllowed,
    /// Tag 12: MIR negatives not currently allowed — no payload.
    MIRNegativesNotCurrentlyAllowed,
    /// Tag 13: insufficient for transfer — `MIRPot + Mismatch
    /// RelLTEQ Coin` (R617 typed).
    InsufficientForTransferDELEG {
        /// Which pot the transfer was attempted from.
        pot: MirPot,
        /// Supplied vs expected coin mismatch.
        mismatch: Mismatch<u64>,
    },
    /// Tag 14: MIR produces negative update — no payload.
    MIRProducesNegativeUpdate,
    /// Tag 15: MIR negative transfer — `MIRPot + Coin` (R617 typed).
    MIRNegativeTransfer {
        /// Which pot the negative transfer targets.
        pot: MirPot,
        /// Attempted transfer amount.
        amount: u64,
    },
    /// Tag 16: delegatee pool not registered — `KeyHash
    /// StakePool` (R615 typed).
    DelegateeNotRegisteredDELEG(KeyHash),
}

/// 32-byte VRF verification-key hash newtype mirroring upstream
/// `newtype VRFVerKeyHash (r :: KeyRoleVRF) = VRFVerKeyHash
/// {unVRFVerKeyHash :: Hash HASH (VerKeyVRF VRF)}`. Display
/// matches upstream stock-derived Show:
/// `VRFVerKeyHash {unVRFVerKeyHash = "<hex>"}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct VrfVerKeyHash(pub [u8; 32]);

impl fmt::Display for VrfVerKeyHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "VRFVerKeyHash {{unVRFVerKeyHash = \"{}\"}}",
            hex::encode(self.0)
        )
    }
}

/// MIR (Move Instantaneous Rewards) pot mirroring upstream
/// `data MIRPot = ReservesMIR | TreasuryMIR` from
/// `Cardano.Ledger.Shelley.TxCert`. CBOR encoding is a Word8:
/// 0 = ReservesMIR, 1 = TreasuryMIR.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum MirPot {
    /// Word8 = 0.
    ReservesMIR,
    /// Word8 = 1.
    TreasuryMIR,
}

impl MirPot {
    /// Decode `MIRPot` from the next CBOR Word8.
    pub fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let n = dec
            .unsigned()
            .map_err(|err| DecoderError(format!("MIRPot: expected Word8: {err:?}")))?;
        match n {
            0 => Ok(Self::ReservesMIR),
            1 => Ok(Self::TreasuryMIR),
            other => Err(DecoderError(format!(
                "MIRPot: unknown pot {other} (expected 0 or 1)"
            ))),
        }
    }
}

impl fmt::Display for MirPot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ReservesMIR => "ReservesMIR",
            Self::TreasuryMIR => "TreasuryMIR",
        })
    }
}

/// Cardano credential mirroring upstream `data Credential (kr ::
/// KeyRole) = ScriptHashObj !ScriptHash | KeyHashObj !(KeyHash kr)`
/// from `Cardano.Ledger.Credential`. CBOR wire format is a
/// 2-element array `[tag, hash]` where tag 0 = KeyHashObj, tag 1
/// = ScriptHashObj (per upstream's `EncCBOR (Credential kr)`).
/// Display matches upstream stock-derived constructor Show wrapped
/// in the appropriate hash newtype's record / value Show.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Credential {
    /// Tag 0: 28-byte key hash.
    KeyHashObj(KeyHash),
    /// Tag 1: 28-byte script hash.
    ScriptHashObj(ScriptHash),
}

impl Credential {
    /// Decode a Credential from a 2-element CBOR array
    /// `[tag, bytes(28)]`.
    pub fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let len = dec
            .array()
            .map_err(|err| DecoderError(format!("Credential: expected 2-array: {err:?}")))?;
        if len != 2 {
            return Err(DecoderError(format!(
                "Credential: expected 2-array, got len {len}"
            )));
        }
        let tag = dec
            .unsigned()
            .map_err(|err| DecoderError(format!("Credential: expected Word8 tag: {err:?}")))?;
        let hash_bytes = dec
            .bytes()
            .map_err(|err| DecoderError(format!("Credential: expected hash bytes: {err:?}")))?;
        let arr: [u8; 28] = hash_bytes.try_into().map_err(|_| {
            DecoderError(format!(
                "Credential: hash must be 28 bytes, got {}",
                hash_bytes.len()
            ))
        })?;
        match tag {
            0 => Ok(Self::KeyHashObj(KeyHash(arr))),
            1 => Ok(Self::ScriptHashObj(ScriptHash(arr))),
            other => Err(DecoderError(format!(
                "Credential: unknown tag {other} (expected 0 or 1)"
            ))),
        }
    }
}

impl fmt::Display for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyHashObj(kh) => write!(f, "KeyHashObj ({kh})"),
            Self::ScriptHashObj(sh) => write!(f, "ScriptHashObj ({sh})"),
        }
    }
}

impl ShelleyDelegPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::StakeKeyAlreadyRegisteredDELEG(_) => 0,
            Self::StakeKeyNotRegisteredDELEG(_) => 1,
            Self::StakeKeyNonZeroAccountBalanceDELEG(_) => 2,
            Self::StakeDelegationImpossibleDELEG(_) => 3,
            Self::WrongCertificateTypeDELEG => 4,
            Self::GenesisKeyNotInMappingDELEG(_) => 5,
            Self::DuplicateGenesisDelegateDELEG(_) => 6,
            Self::InsufficientForInstantaneousRewardsDELEG { .. } => 7,
            Self::MIRCertificateTooLateinEpochDELEG(_) => 8,
            Self::DuplicateGenesisVRFDELEG(_) => 9,
            Self::MIRTransferNotCurrentlyAllowed => 11,
            Self::MIRNegativesNotCurrentlyAllowed => 12,
            Self::InsufficientForTransferDELEG { .. } => 13,
            Self::MIRProducesNegativeUpdate => 14,
            Self::MIRNegativeTransfer { .. } => 15,
            Self::DelegateeNotRegisteredDELEG(_) => 16,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::StakeKeyAlreadyRegisteredDELEG(_) => "StakeKeyAlreadyRegisteredDELEG",
            Self::StakeKeyNotRegisteredDELEG(_) => "StakeKeyNotRegisteredDELEG",
            Self::StakeKeyNonZeroAccountBalanceDELEG(_) => "StakeKeyNonZeroAccountBalanceDELEG",
            Self::StakeDelegationImpossibleDELEG(_) => "StakeDelegationImpossibleDELEG",
            Self::WrongCertificateTypeDELEG => "WrongCertificateTypeDELEG",
            Self::GenesisKeyNotInMappingDELEG(_) => "GenesisKeyNotInMappingDELEG",
            Self::DuplicateGenesisDelegateDELEG(_) => "DuplicateGenesisDelegateDELEG",
            Self::InsufficientForInstantaneousRewardsDELEG { .. } => {
                "InsufficientForInstantaneousRewardsDELEG"
            }
            Self::MIRCertificateTooLateinEpochDELEG(_) => "MIRCertificateTooLateinEpochDELEG",
            Self::DuplicateGenesisVRFDELEG(_) => "DuplicateGenesisVRFDELEG",
            Self::MIRTransferNotCurrentlyAllowed => "MIRTransferNotCurrentlyAllowed",
            Self::MIRNegativesNotCurrentlyAllowed => "MIRNegativesNotCurrentlyAllowed",
            Self::InsufficientForTransferDELEG { .. } => "InsufficientForTransferDELEG",
            Self::MIRProducesNegativeUpdate => "MIRProducesNegativeUpdate",
            Self::MIRNegativeTransfer { .. } => "MIRNegativeTransfer",
            Self::DelegateeNotRegisteredDELEG(_) => "DelegateeNotRegisteredDELEG",
        }
    }

    /// Decode the full `ShelleyDelegPredFailure` outer envelope
    /// from CBOR bytes. Upstream encoding (via `Sum`) uses 1-, 2-,
    /// or 3-element envelopes depending on the payload. R615
    /// decodes the simplest variants typed; complex variants keep
    /// raw bytes pending Credential / MIRPot / Mismatch ports.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ShelleyDelegPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if !(1..=3).contains(&len) {
            return Err(DecoderError(format!(
                "ShelleyDelegPredFailure: expected 1- to 3-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ShelleyDelegPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        let no_payload_check = |actual: u64, label: &str| {
            if actual != 1 {
                Err(DecoderError(format!(
                    "{label}: expected 1-element envelope, got len {actual}"
                )))
            } else {
                Ok(())
            }
        };
        let two_element_check = |actual: u64, label: &str| {
            if actual != 2 {
                Err(DecoderError(format!(
                    "{label}: expected 2-element envelope, got len {actual}"
                )))
            } else {
                Ok(())
            }
        };
        let read_keyhash = |dec: &mut Decoder<'_>, label: &str| -> Result<KeyHash, DecoderError> {
            let bytes = dec
                .bytes()
                .map_err(|err| DecoderError(format!("{label}: expected KeyHash bytes: {err:?}")))?;
            let arr: [u8; 28] = bytes.try_into().map_err(|_| {
                DecoderError(format!(
                    "{label}: KeyHash must be 28 bytes, got {}",
                    bytes.len()
                ))
            })?;
            Ok(KeyHash(arr))
        };
        match tag {
            // No-payload variants.
            4 => {
                no_payload_check(len, "WrongCertificateTypeDELEG")?;
                Ok(Self::WrongCertificateTypeDELEG)
            }
            11 => {
                no_payload_check(len, "MIRTransferNotCurrentlyAllowed")?;
                Ok(Self::MIRTransferNotCurrentlyAllowed)
            }
            12 => {
                no_payload_check(len, "MIRNegativesNotCurrentlyAllowed")?;
                Ok(Self::MIRNegativesNotCurrentlyAllowed)
            }
            14 => {
                no_payload_check(len, "MIRProducesNegativeUpdate")?;
                Ok(Self::MIRProducesNegativeUpdate)
            }
            // Tag 2: Coin (Word64).
            2 => {
                two_element_check(len, "StakeKeyNonZeroAccountBalanceDELEG")?;
                let coin = dec.unsigned().map_err(|err| {
                    DecoderError(format!("StakeKeyNonZeroAccountBalanceDELEG: coin: {err:?}"))
                })?;
                Ok(Self::StakeKeyNonZeroAccountBalanceDELEG(coin))
            }
            // Tags 5/6/16: KeyHash (28 bytes).
            5 => {
                two_element_check(len, "GenesisKeyNotInMappingDELEG")?;
                let kh = read_keyhash(&mut dec, "GenesisKeyNotInMappingDELEG")?;
                Ok(Self::GenesisKeyNotInMappingDELEG(kh))
            }
            6 => {
                two_element_check(len, "DuplicateGenesisDelegateDELEG")?;
                let kh = read_keyhash(&mut dec, "DuplicateGenesisDelegateDELEG")?;
                Ok(Self::DuplicateGenesisDelegateDELEG(kh))
            }
            16 => {
                two_element_check(len, "DelegateeNotRegisteredDELEG")?;
                let kh = read_keyhash(&mut dec, "DelegateeNotRegisteredDELEG")?;
                Ok(Self::DelegateeNotRegisteredDELEG(kh))
            }
            // Tag 9: VRFVerKeyHash (32 bytes).
            9 => {
                two_element_check(len, "DuplicateGenesisVRFDELEG")?;
                let bytes_payload = dec.bytes().map_err(|err| {
                    DecoderError(format!(
                        "DuplicateGenesisVRFDELEG: expected VRFVerKeyHash bytes: {err:?}"
                    ))
                })?;
                let arr: [u8; 32] = bytes_payload.try_into().map_err(|_| {
                    DecoderError(format!(
                        "DuplicateGenesisVRFDELEG: VRFVerKeyHash must be 32 bytes, got {}",
                        bytes_payload.len()
                    ))
                })?;
                Ok(Self::DuplicateGenesisVRFDELEG(VrfVerKeyHash(arr)))
            }
            // Tag 7: MIRPot + Mismatch RelLTEQ Coin (R617 typed).
            7 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "InsufficientForInstantaneousRewardsDELEG: expected 3-element envelope, got len {len}"
                    )));
                }
                let pot = MirPot::from_decoder(&mut dec).map_err(|err| {
                    DecoderError(format!(
                        "InsufficientForInstantaneousRewardsDELEG: {}",
                        err.0
                    ))
                })?;
                let mismatch =
                    decode_mismatch_u64(&mut dec, MismatchRelation::RelLTEQ).map_err(|err| {
                        DecoderError(format!(
                            "InsufficientForInstantaneousRewardsDELEG: {}",
                            err.0
                        ))
                    })?;
                Ok(Self::InsufficientForInstantaneousRewardsDELEG { pot, mismatch })
            }
            // Tag 8: Mismatch RelLT SlotNo (R617 typed).
            8 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "MIRCertificateTooLateinEpochDELEG: expected 2-element envelope, got len {len}"
                    )));
                }
                let mismatch =
                    decode_mismatch_u64(&mut dec, MismatchRelation::RelLT).map_err(|err| {
                        DecoderError(format!("MIRCertificateTooLateinEpochDELEG: {}", err.0))
                    })?;
                Ok(Self::MIRCertificateTooLateinEpochDELEG(mismatch))
            }
            // Tag 13: MIRPot + Mismatch RelLTEQ Coin (R617 typed).
            13 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "InsufficientForTransferDELEG: expected 3-element envelope, got len {len}"
                    )));
                }
                let pot = MirPot::from_decoder(&mut dec).map_err(|err| {
                    DecoderError(format!("InsufficientForTransferDELEG: {}", err.0))
                })?;
                let mismatch =
                    decode_mismatch_u64(&mut dec, MismatchRelation::RelLTEQ).map_err(|err| {
                        DecoderError(format!("InsufficientForTransferDELEG: {}", err.0))
                    })?;
                Ok(Self::InsufficientForTransferDELEG { pot, mismatch })
            }
            // Tag 15: MIRPot + Coin (R617 typed).
            15 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "MIRNegativeTransfer: expected 3-element envelope, got len {len}"
                    )));
                }
                let pot = MirPot::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("MIRNegativeTransfer: {}", err.0)))?;
                let amount = dec
                    .unsigned()
                    .map_err(|err| DecoderError(format!("MIRNegativeTransfer: amount: {err:?}")))?;
                Ok(Self::MIRNegativeTransfer { pot, amount })
            }
            // Tags 0/1/3: Credential payload (R618 typed).
            0 | 1 | 3 => {
                two_element_check(len, "Credential-bearing DELEG variant")?;
                let cred = Credential::from_decoder(&mut dec)?;
                Ok(match tag {
                    0 => Self::StakeKeyAlreadyRegisteredDELEG(cred),
                    1 => Self::StakeKeyNotRegisteredDELEG(cred),
                    3 => Self::StakeDelegationImpossibleDELEG(cred),
                    _ => unreachable!("tag set above"),
                })
            }
            other => Err(DecoderError(format!(
                "ShelleyDelegPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ShelleyDelegPredFailure {
    /// Render upstream stock-derived `Show
    /// (ShelleyDelegPredFailure era)`. Typed variants route
    /// through their typed inner Display; raw variants emit a
    /// `<raw-cbor N bytes>` marker.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StakeKeyAlreadyRegisteredDELEG(cred) => {
                write!(f, "StakeKeyAlreadyRegisteredDELEG ({cred})")
            }
            Self::StakeKeyNotRegisteredDELEG(cred) => {
                write!(f, "StakeKeyNotRegisteredDELEG ({cred})")
            }
            Self::StakeDelegationImpossibleDELEG(cred) => {
                write!(f, "StakeDelegationImpossibleDELEG ({cred})")
            }
            Self::InsufficientForInstantaneousRewardsDELEG { pot, mismatch } => {
                let typed = Mismatch {
                    relation: mismatch.relation,
                    supplied: CoinShow(mismatch.supplied),
                    expected: CoinShow(mismatch.expected),
                };
                write!(
                    f,
                    "InsufficientForInstantaneousRewardsDELEG {pot} ({typed})"
                )
            }
            Self::MIRCertificateTooLateinEpochDELEG(mismatch) => {
                write!(f, "MIRCertificateTooLateinEpochDELEG ({mismatch})")
            }
            Self::InsufficientForTransferDELEG { pot, mismatch } => {
                let typed = Mismatch {
                    relation: mismatch.relation,
                    supplied: CoinShow(mismatch.supplied),
                    expected: CoinShow(mismatch.expected),
                };
                write!(f, "InsufficientForTransferDELEG {pot} ({typed})")
            }
            Self::MIRNegativeTransfer { pot, amount } => {
                write!(f, "MIRNegativeTransfer {pot} ({})", CoinShow(*amount))
            }
            Self::StakeKeyNonZeroAccountBalanceDELEG(coin) => {
                write!(
                    f,
                    "StakeKeyNonZeroAccountBalanceDELEG ({})",
                    CoinShow(*coin)
                )
            }
            Self::WrongCertificateTypeDELEG => f.write_str("WrongCertificateTypeDELEG"),
            Self::GenesisKeyNotInMappingDELEG(kh) => {
                write!(f, "GenesisKeyNotInMappingDELEG ({kh})")
            }
            Self::DuplicateGenesisDelegateDELEG(kh) => {
                write!(f, "DuplicateGenesisDelegateDELEG ({kh})")
            }
            Self::DuplicateGenesisVRFDELEG(vrf) => {
                write!(f, "DuplicateGenesisVRFDELEG ({vrf})")
            }
            Self::MIRTransferNotCurrentlyAllowed => f.write_str("MIRTransferNotCurrentlyAllowed"),
            Self::MIRNegativesNotCurrentlyAllowed => f.write_str("MIRNegativesNotCurrentlyAllowed"),
            Self::MIRProducesNegativeUpdate => f.write_str("MIRProducesNegativeUpdate"),
            Self::DelegateeNotRegisteredDELEG(kh) => {
                write!(f, "DelegateeNotRegisteredDELEG ({kh})")
            }
        }
    }
}

/// Non-empty list of staking-role key hashes mirroring upstream
/// `NonEmpty (KeyHash Staking)`. Wire format is a regular CBOR
/// array with ≥ 1 entry; KeyHash items are 28-byte bytestrings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptyKeyHash {
    /// Decoded KeyHash entries. Guaranteed non-empty by `from_cbor`.
    pub entries: Vec<KeyHash>,
}

impl NonEmptyKeyHash {
    /// Decode from canonical CBOR bytes (regular array of bytes(28)).
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        Self::from_decoder(&mut dec)
    }

    /// Decode from an in-progress `Decoder`.
    pub fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let count = dec.array().map_err(|err| {
            DecoderError(format!("NonEmptyKeyHash: expected CBOR array: {err:?}"))
        })?;
        if count == 0 {
            return Err(DecoderError(
                "NonEmptyKeyHash: NonEmpty requires at least one entry".to_string(),
            ));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let hash_bytes = dec.bytes().map_err(|err| {
                DecoderError(format!("NonEmptyKeyHash: expected KeyHash bytes: {err:?}"))
            })?;
            let arr: [u8; 28] = hash_bytes.try_into().map_err(|_| {
                DecoderError(format!(
                    "NonEmptyKeyHash: KeyHash must be 28 bytes, got {}",
                    hash_bytes.len()
                ))
            })?;
            entries.push(KeyHash(arr));
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptyKeyHash {
    /// Render upstream `Show (NonEmpty a)`: `<head> :| [<tail>...]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (head, tail) = self
            .entries
            .split_first()
            .expect("NonEmptyKeyHash enforces ≥1 entry at decode time");
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

/// `ConwayLedgerPredFailure` mirror — Conway-era LEDGER root
/// predicate failure (replaces Shelley's
/// `ShelleyLedgerPredFailure` for ConwayEra only;
/// Shelley/Allegra/Mary/Alonzo/Babbage all reuse the Shelley type
/// per `type instance EraRuleFailure "LEDGER" <Era> =
/// ShelleyLedgerPredFailure <Era>`).
///
/// Upstream: `data ConwayLedgerPredFailure era` from
/// `Cardano.Ledger.Conway.Rules.Ledger` with 9 variants encoded
/// via CBOR `Sum` tags 1..9 (no tag 0). Conway swaps DELEGS for
/// CERTS and adds the new GOV sub-rule for governance.
///
/// R623 ships the scaffold + typed payloads for the simple
/// variants (4 NonEmptyKeyHash, 5/6 Mismatch via ToGroup, 7 Text,
/// 8 Withdrawals, 9 IncompleteWithdrawals). The 3 sub-rule
/// variants (UtxowFailure / CertsFailure / GovFailure) carry raw
/// payloads pending the era-specific UTXOW / CERTS / GOV decoder
/// ports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConwayLedgerPredFailure {
    /// Tag 1: nested Conway UTXOW failure (R624 wired to the
    /// typed `ConwayUtxowPredFailure` 19-variant scaffold).
    ConwayUtxowFailure(ConwayUtxowPredFailure),
    /// Tag 2: nested Conway CERTS failure (R625 wired to typed
    /// `ConwayCertsPredFailure`). Replaces Shelley's DELEGS path.
    ConwayCertsFailure(ConwayCertsPredFailure),
    /// Tag 3: nested Conway GOV failure (R626 wired to typed
    /// `ConwayGovPredFailure`). New in Conway for governance
    /// actions.
    ConwayGovFailure(ConwayGovPredFailure),
    /// Tag 4: withdrawal target was not delegated to a DRep —
    /// `NonEmpty (KeyHash Staking)` (R623 typed).
    ConwayWdrlNotDelegatedToDRep(NonEmptyKeyHash),
    /// Tag 5: treasury value mismatch — `Mismatch RelEQ Coin`
    /// (R623 typed). Encoded as ToGroup-flattened with
    /// expected-first ordering per upstream.
    ConwayTreasuryValueMismatch(Mismatch<u64>),
    /// Tag 6: tx ref scripts total size too big —
    /// `Mismatch RelLTEQ Int` (R623 typed). Encoded as
    /// ToGroup-flattened.
    ConwayTxRefScriptsSizeTooBig(Mismatch<u64>),
    /// Tag 7: free-form mempool reject reason — `Text` (R623
    /// typed).
    ConwayMempoolFailure(String),
    /// Tag 8: withdrawals reference unknown accounts —
    /// `Withdrawals` (R596 typed; reused from Shelley path).
    ConwayWithdrawalsMissingAccounts(Withdrawals),
    /// Tag 9: incomplete withdrawals — `NonEmptyMap
    /// AccountAddress (Mismatch RelEQ Coin)` (R597 typed; reused).
    ConwayIncompleteWithdrawals(IncompleteWithdrawals),
}

impl ConwayLedgerPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::ConwayUtxowFailure(_) => 1,
            Self::ConwayCertsFailure(_) => 2,
            Self::ConwayGovFailure(_) => 3,
            Self::ConwayWdrlNotDelegatedToDRep(_) => 4,
            Self::ConwayTreasuryValueMismatch(_) => 5,
            Self::ConwayTxRefScriptsSizeTooBig(_) => 6,
            Self::ConwayMempoolFailure(_) => 7,
            Self::ConwayWithdrawalsMissingAccounts(_) => 8,
            Self::ConwayIncompleteWithdrawals(_) => 9,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::ConwayUtxowFailure(_) => "ConwayUtxowFailure",
            Self::ConwayCertsFailure(_) => "ConwayCertsFailure",
            Self::ConwayGovFailure(_) => "ConwayGovFailure",
            Self::ConwayWdrlNotDelegatedToDRep(_) => "ConwayWdrlNotDelegatedToDRep",
            Self::ConwayTreasuryValueMismatch(_) => "ConwayTreasuryValueMismatch",
            Self::ConwayTxRefScriptsSizeTooBig(_) => "ConwayTxRefScriptsSizeTooBig",
            Self::ConwayMempoolFailure(_) => "ConwayMempoolFailure",
            Self::ConwayWithdrawalsMissingAccounts(_) => "ConwayWithdrawalsMissingAccounts",
            Self::ConwayIncompleteWithdrawals(_) => "ConwayIncompleteWithdrawals",
        }
    }

    /// Decode the full `ConwayLedgerPredFailure` outer envelope
    /// from CBOR bytes. Upstream uses `Sum`-tag encoding starting
    /// at tag 1 (no tag 0).
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ConwayLedgerPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if !(2..=4).contains(&len) {
            return Err(DecoderError(format!(
                "ConwayLedgerPredFailure: expected 2- to 4-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ConwayLedgerPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        let payload_offset = dec.position();
        match tag {
            // Tag 1: typed Conway UTXOW sub-rule (R624).
            1 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "ConwayUtxowFailure: expected 2-element envelope, got len {len}"
                    )));
                }
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError(
                        "ConwayLedgerPredFailure: payload offset out of bounds".to_string(),
                    )
                })?;
                let utxow = ConwayUtxowPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::ConwayUtxowFailure(utxow))
            }
            // Tag 2: typed Conway CERTS sub-rule (R625).
            2 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "ConwayCertsFailure: expected 2-element envelope, got len {len}"
                    )));
                }
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError(
                        "ConwayLedgerPredFailure: payload offset out of bounds".to_string(),
                    )
                })?;
                let certs = ConwayCertsPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::ConwayCertsFailure(certs))
            }
            // Tag 3: typed Conway GOV sub-rule (R626).
            3 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "ConwayGovFailure: expected 2-element envelope, got len {len}"
                    )));
                }
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError(
                        "ConwayLedgerPredFailure: payload offset out of bounds".to_string(),
                    )
                })?;
                let gov = ConwayGovPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::ConwayGovFailure(gov))
            }
            // Tag 4: NonEmpty (KeyHash Staking).
            4 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "ConwayWdrlNotDelegatedToDRep: expected 2-element envelope, got len {len}"
                    )));
                }
                let keys = NonEmptyKeyHash::from_decoder(&mut dec).map_err(|err| {
                    DecoderError(format!("ConwayWdrlNotDelegatedToDRep: {}", err.0))
                })?;
                Ok(Self::ConwayWdrlNotDelegatedToDRep(keys))
            }
            // Tag 5: Mismatch RelEQ Coin (ToGroup, expected-first).
            5 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "ConwayTreasuryValueMismatch: expected 3-element envelope, got len {len}"
                    )));
                }
                let expected = dec.unsigned().map_err(|err| {
                    DecoderError(format!("ConwayTreasuryValueMismatch: expected: {err:?}"))
                })?;
                let supplied = dec.unsigned().map_err(|err| {
                    DecoderError(format!("ConwayTreasuryValueMismatch: supplied: {err:?}"))
                })?;
                Ok(Self::ConwayTreasuryValueMismatch(Mismatch {
                    relation: MismatchRelation::RelEQ,
                    supplied,
                    expected,
                }))
            }
            // Tag 6: Mismatch RelLTEQ Int (ToGroup, expected-first).
            6 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "ConwayTxRefScriptsSizeTooBig: expected 3-element envelope, got len {len}"
                    )));
                }
                let supplied = dec.unsigned().map_err(|err| {
                    DecoderError(format!("ConwayTxRefScriptsSizeTooBig: supplied: {err:?}"))
                })?;
                let expected = dec.unsigned().map_err(|err| {
                    DecoderError(format!("ConwayTxRefScriptsSizeTooBig: expected: {err:?}"))
                })?;
                Ok(Self::ConwayTxRefScriptsSizeTooBig(Mismatch {
                    relation: MismatchRelation::RelLTEQ,
                    supplied,
                    expected,
                }))
            }
            // Tag 7: Text mempool reject reason — CBOR text-string
            // (major type 3) per upstream `encCBOR` on `Text`.
            7 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "ConwayMempoolFailure: expected 2-element envelope, got len {len}"
                    )));
                }
                let s = dec.text_owned().map_err(|err| {
                    DecoderError(format!("ConwayMempoolFailure: expected text: {err:?}"))
                })?;
                Ok(Self::ConwayMempoolFailure(s))
            }
            // Tag 8: Withdrawals (R596 typed).
            8 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "ConwayWithdrawalsMissingAccounts: expected 2-element envelope, got len {len}"
                    )));
                }
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError(
                        "ConwayLedgerPredFailure: payload offset out of bounds".to_string(),
                    )
                })?;
                let withdrawals = Withdrawals::from_cbor(payload_bytes).map_err(|err| {
                    DecoderError(format!("ConwayWithdrawalsMissingAccounts: {}", err.0))
                })?;
                Ok(Self::ConwayWithdrawalsMissingAccounts(withdrawals))
            }
            // Tag 9: NonEmptyMap AccountAddress (Mismatch RelEQ
            // Coin) (R597 typed).
            9 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "ConwayIncompleteWithdrawals: expected 2-element envelope, got len {len}"
                    )));
                }
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError(
                        "ConwayLedgerPredFailure: payload offset out of bounds".to_string(),
                    )
                })?;
                let iw = IncompleteWithdrawals::from_cbor(payload_bytes).map_err(|err| {
                    DecoderError(format!("ConwayIncompleteWithdrawals: {}", err.0))
                })?;
                Ok(Self::ConwayIncompleteWithdrawals(iw))
            }
            other => Err(DecoderError(format!(
                "ConwayLedgerPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ConwayLedgerPredFailure {
    /// Render upstream stock-derived `Show
    /// (ConwayLedgerPredFailure era)`: `<Constructor> <payload>`.
    /// Sub-rule variants emit raw-cbor markers; typed variants
    /// route through their typed inner Display.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConwayUtxowFailure(utxow) => {
                write!(f, "ConwayUtxowFailure ({utxow})")
            }
            Self::ConwayCertsFailure(certs) => {
                write!(f, "ConwayCertsFailure ({certs})")
            }
            Self::ConwayGovFailure(gov) => {
                write!(f, "ConwayGovFailure ({gov})")
            }
            Self::ConwayWdrlNotDelegatedToDRep(keys) => {
                write!(f, "ConwayWdrlNotDelegatedToDRep ({keys})")
            }
            Self::ConwayTreasuryValueMismatch(mm) => {
                let typed = Mismatch {
                    relation: mm.relation,
                    supplied: CoinShow(mm.supplied),
                    expected: CoinShow(mm.expected),
                };
                write!(f, "ConwayTreasuryValueMismatch ({typed})")
            }
            Self::ConwayTxRefScriptsSizeTooBig(mm) => {
                write!(f, "ConwayTxRefScriptsSizeTooBig ({mm})")
            }
            Self::ConwayMempoolFailure(s) => {
                // Upstream Text Show wraps with quotes via Show
                // String (which uses Haskell-style escaping). The
                // GHC `Show (ByteString)` mnemonic escapes from R589
                // are the closest analog.
                let escaped = show_haskell_bytestring_like(s);
                write!(f, "ConwayMempoolFailure {escaped}")
            }
            Self::ConwayWithdrawalsMissingAccounts(w) => {
                write!(f, "ConwayWithdrawalsMissingAccounts ({w})")
            }
            Self::ConwayIncompleteWithdrawals(iw) => {
                write!(f, "ConwayIncompleteWithdrawals (fromList [{iw}])")
            }
        }
    }
}

/// Render a Rust string using Haskell `Show String` escapes.
/// Simplified subset: only the common control chars + backslash +
/// quote. Sufficient for typical mempool-reject messages which
/// are ASCII operator-facing strings.
fn show_haskell_bytestring_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\{}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `ConwayPlutusPurpose AsIx` mirror — the index-only form of a
/// Conway Plutus script purpose (`ConwayPlutusPurpose f era` with
/// `f = AsIx`). Each variant carries an `AsIx Word32` redeemer
/// pointer. CBOR wire format is a 2-element `CBORGroup`
/// `[word8-tag, word32-index]` per upstream `EncCBORGroup
/// (ConwayPlutusPurpose f era)` (tags 0-5).
///
/// Display matches upstream stock-derived `Show (ConwayPlutusPurpose
/// AsIx era)`: `<Constructor> (AsIx {unAsIx = <n>})`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ConwayPlutusPurposeIx {
    /// Tag 0: spending a transaction input.
    ConwaySpending(u32),
    /// Tag 1: minting under a policy.
    ConwayMinting(u32),
    /// Tag 2: certifying a tx certificate.
    ConwayCertifying(u32),
    /// Tag 3: rewarding a reward account.
    ConwayRewarding(u32),
    /// Tag 4: voting (Conway governance).
    ConwayVoting(u32),
    /// Tag 5: proposing a governance action (Conway).
    ConwayProposing(u32),
}

impl ConwayPlutusPurposeIx {
    /// Upstream stock-derived `Show` constructor name.
    fn constructor(self) -> &'static str {
        match self {
            Self::ConwaySpending(_) => "ConwaySpending",
            Self::ConwayMinting(_) => "ConwayMinting",
            Self::ConwayCertifying(_) => "ConwayCertifying",
            Self::ConwayRewarding(_) => "ConwayRewarding",
            Self::ConwayVoting(_) => "ConwayVoting",
            Self::ConwayProposing(_) => "ConwayProposing",
        }
    }

    /// Decode a single `ConwayPlutusPurpose AsIx` from its
    /// 2-element CBORGroup envelope `[word8-tag, word32-index]`.
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ConwayPlutusPurposeIx: expected 2-element group: {err:?}"
            ))
        })?;
        if len != 2 {
            return Err(DecoderError(format!(
                "ConwayPlutusPurposeIx: expected 2-element group, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ConwayPlutusPurposeIx: expected Word8 tag: {err:?}"
            ))
        })?;
        let raw_ix = dec.unsigned().map_err(|err| {
            DecoderError(format!("ConwayPlutusPurposeIx: expected index: {err:?}"))
        })?;
        let ix = u32::try_from(raw_ix).map_err(|_| {
            DecoderError(format!(
                "ConwayPlutusPurposeIx: index {raw_ix} does not fit Word32"
            ))
        })?;
        match tag {
            0 => Ok(Self::ConwaySpending(ix)),
            1 => Ok(Self::ConwayMinting(ix)),
            2 => Ok(Self::ConwayCertifying(ix)),
            3 => Ok(Self::ConwayRewarding(ix)),
            4 => Ok(Self::ConwayVoting(ix)),
            5 => Ok(Self::ConwayProposing(ix)),
            other => Err(DecoderError(format!(
                "ConwayPlutusPurposeIx: unknown purpose tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ConwayPlutusPurposeIx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ix = match self {
            Self::ConwaySpending(i)
            | Self::ConwayMinting(i)
            | Self::ConwayCertifying(i)
            | Self::ConwayRewarding(i)
            | Self::ConwayVoting(i)
            | Self::ConwayProposing(i) => *i,
        };
        write!(f, "{} (AsIx {{unAsIx = {ix}}})", self.constructor())
    }
}

/// Non-empty list of `ConwayPlutusPurpose AsIx` mirroring upstream
/// `NonEmpty (PlutusPurpose AsIx era)`. CBOR wire format is a
/// plain array of 2-element CBORGroup envelopes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptyPlutusPurposeIx {
    /// Decoded entries in wire order. Guaranteed non-empty by
    /// `from_cbor`.
    pub entries: Vec<ConwayPlutusPurposeIx>,
}

impl NonEmptyPlutusPurposeIx {
    /// Decode a `NonEmpty (PlutusPurpose AsIx era)` from canonical
    /// CBOR bytes.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let count = dec.array().map_err(|err| {
            DecoderError(format!(
                "NonEmptyPlutusPurposeIx: expected CBOR array: {err:?}"
            ))
        })?;
        if count == 0 {
            return Err(DecoderError(
                "NonEmptyPlutusPurposeIx: NonEmpty requires at least one entry".to_string(),
            ));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(ConwayPlutusPurposeIx::from_decoder(&mut dec)?);
        }
        Ok(Self { entries })
    }
}

impl fmt::Display for NonEmptyPlutusPurposeIx {
    /// Render upstream `Show (NonEmpty a)`: `<head> :| [<tail>...]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (head, tail) = self
            .entries
            .split_first()
            .expect("NonEmptyPlutusPurposeIx enforces ≥1 entry at decode time");
        write!(f, "{head} :| [")?;
        let mut first = true;
        for p in tail {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "{p}")?;
        }
        f.write_str("]")
    }
}

/// `ConwayUtxowPredFailure` mirror — Conway-era UTXOW sub-rule
/// failure (under `ConwayLedgerPredFailure::ConwayUtxowFailure`).
///
/// Upstream: `data ConwayUtxowPredFailure era` from
/// `Cardano.Ledger.Conway.Rules.Utxow` with 19 variants encoded
/// via CBOR `Sum` tags 0-18. The Conway variant set extends the
/// Babbage/Alonzo UTXOW set with Plutus-V3 + governance-related
/// failure modes (MissingRedeemers, MissingRequiredDatums, etc.).
///
/// R624 ships the scaffold with typed payloads for 12 of the 19
/// variants (reusing existing carriers from the Shelley path).
/// The remaining 7 variants (0/10/11/12/13/15/18) keep raw inner
/// CBOR pending nested-rule / Plutus-purpose / DataHash /
/// ScriptIntegrityHash decoders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConwayUtxowPredFailure {
    /// Tag 0: nested Conway UTXO failure (R630 wired to typed
    /// `ConwayUtxoPredFailure`).
    UtxoFailure(ConwayUtxoPredFailure),
    /// Tag 1: witnesses that failed verification —
    /// `NonEmpty (VKey Witness)` (R601 typed).
    InvalidWitnessesUTXOW(NonEmptyVKey),
    /// Tag 2: vkey witnesses needed but not supplied —
    /// `NonEmptySet (KeyHash Witness)` (R600 typed).
    MissingVKeyWitnessesUTXOW(NonEmptySetKeyHash),
    /// Tag 3: missing scripts — `NonEmptySet ScriptHash` (R599
    /// typed).
    MissingScriptWitnessesUTXOW(NonEmptySetScriptHash),
    /// Tag 4: failed scripts — `NonEmptySet ScriptHash` (R599
    /// typed).
    ScriptWitnessNotValidatingUTXOW(NonEmptySetScriptHash),
    /// Tag 5: tx body claims metadata but metadata-hash field
    /// missing — `TxAuxDataHash` (R598 typed).
    MissingTxBodyMetadataHash(TxAuxDataHash),
    /// Tag 6: metadata-hash present but the metadata itself was
    /// not supplied — `TxAuxDataHash` (R598 typed).
    MissingTxMetadata(TxAuxDataHash),
    /// Tag 7: metadata-hash mismatch — `Mismatch RelEQ
    /// TxAuxDataHash`. ToGroup-flattened wire encoding (supplied
    /// then expected). (R624 typed.)
    ConflictingMetadataHash(Mismatch<TxAuxDataHash>),
    /// Tag 8: invalid metadata — no payload.
    InvalidMetadata,
    /// Tag 9: extraneous scripts supplied beyond what the tx
    /// required — `NonEmptySet ScriptHash` (R599 typed).
    ExtraneousScriptWitnessesUTXOW(NonEmptySetScriptHash),
    /// Tag 10: missing redeemers — `NonEmpty (PlutusPurpose
    /// AsItem era, ScriptHash)`. Raw pending PlutusPurpose
    /// decoder.
    MissingRedeemers(Vec<u8>),
    /// Tag 11: missing required datums — `NonEmptySet DataHash +
    /// Set DataHash` (R632 typed).
    MissingRequiredDatums {
        /// Data hashes that were required but not supplied.
        missing: NonEmptySetDataHash,
        /// Data hashes that were received with the transaction.
        received: SetDataHash,
    },
    /// Tag 12: not-allowed supplemental datums — `NonEmptySet
    /// DataHash + Set DataHash` (R632 typed).
    NotAllowedSupplementalDatums {
        /// Supplied data hashes that are not allowed.
        unallowed: NonEmptySetDataHash,
        /// Data hashes that would be acceptable as supplemental.
        acceptable: SetDataHash,
    },
    /// Tag 13: protocol-params-view hash mismatch —
    /// `Mismatch RelEQ (StrictMaybe ScriptIntegrityHash)` via
    /// ToGroup flattened (R638 typed).
    PPViewHashesDontMatch(Mismatch<StrictMaybeScriptIntegrityHash>),
    /// Tag 14: TxIns missing required DataHash —
    /// `NonEmptySet TxIn` (R603 typed).
    UnspendableUTxONoDatumHash(NonEmptySetTxIn),
    /// Tag 15: extra redeemers — `NonEmpty (PlutusPurpose AsIx
    /// era)` (R636 typed).
    ExtraRedeemers(NonEmptyPlutusPurposeIx),
    /// Tag 16: malformed script witnesses —
    /// `NonEmptySet ScriptHash` (R599 typed).
    MalformedScriptWitnesses(NonEmptySetScriptHash),
    /// Tag 17: malformed reference scripts —
    /// `NonEmptySet ScriptHash` (R599 typed).
    MalformedReferenceScripts(NonEmptySetScriptHash),
    /// Tag 18: script integrity hash mismatch — `Mismatch RelEQ
    /// (StrictMaybe ScriptIntegrityHash) + StrictMaybe
    /// ByteString` (R639 typed).
    ScriptIntegrityHashMismatch {
        /// Computed-vs-provided script-integrity-hash mismatch.
        mismatch: Mismatch<StrictMaybeScriptIntegrityHash>,
        /// The provided script-integrity bytes, if any.
        provided: StrictMaybeBytes,
    },
}

impl ConwayUtxowPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::UtxoFailure(_) => 0,
            Self::InvalidWitnessesUTXOW(_) => 1,
            Self::MissingVKeyWitnessesUTXOW(_) => 2,
            Self::MissingScriptWitnessesUTXOW(_) => 3,
            Self::ScriptWitnessNotValidatingUTXOW(_) => 4,
            Self::MissingTxBodyMetadataHash(_) => 5,
            Self::MissingTxMetadata(_) => 6,
            Self::ConflictingMetadataHash(_) => 7,
            Self::InvalidMetadata => 8,
            Self::ExtraneousScriptWitnessesUTXOW(_) => 9,
            Self::MissingRedeemers(_) => 10,
            Self::MissingRequiredDatums { .. } => 11,
            Self::NotAllowedSupplementalDatums { .. } => 12,
            Self::PPViewHashesDontMatch(_) => 13,
            Self::UnspendableUTxONoDatumHash(_) => 14,
            Self::ExtraRedeemers(_) => 15,
            Self::MalformedScriptWitnesses(_) => 16,
            Self::MalformedReferenceScripts(_) => 17,
            Self::ScriptIntegrityHashMismatch { .. } => 18,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::UtxoFailure(_) => "UtxoFailure",
            Self::InvalidWitnessesUTXOW(_) => "InvalidWitnessesUTXOW",
            Self::MissingVKeyWitnessesUTXOW(_) => "MissingVKeyWitnessesUTXOW",
            Self::MissingScriptWitnessesUTXOW(_) => "MissingScriptWitnessesUTXOW",
            Self::ScriptWitnessNotValidatingUTXOW(_) => "ScriptWitnessNotValidatingUTXOW",
            Self::MissingTxBodyMetadataHash(_) => "MissingTxBodyMetadataHash",
            Self::MissingTxMetadata(_) => "MissingTxMetadata",
            Self::ConflictingMetadataHash(_) => "ConflictingMetadataHash",
            Self::InvalidMetadata => "InvalidMetadata",
            Self::ExtraneousScriptWitnessesUTXOW(_) => "ExtraneousScriptWitnessesUTXOW",
            Self::MissingRedeemers(_) => "MissingRedeemers",
            Self::MissingRequiredDatums { .. } => "MissingRequiredDatums",
            Self::NotAllowedSupplementalDatums { .. } => "NotAllowedSupplementalDatums",
            Self::PPViewHashesDontMatch(_) => "PPViewHashesDontMatch",
            Self::UnspendableUTxONoDatumHash(_) => "UnspendableUTxONoDatumHash",
            Self::ExtraRedeemers(_) => "ExtraRedeemers",
            Self::MalformedScriptWitnesses(_) => "MalformedScriptWitnesses",
            Self::MalformedReferenceScripts(_) => "MalformedReferenceScripts",
            Self::ScriptIntegrityHashMismatch { .. } => "ScriptIntegrityHashMismatch",
        }
    }

    /// Decode the full `ConwayUtxowPredFailure` outer envelope.
    /// Length varies: 1 (tag 8 no-payload) / 2 (most tags) / 3
    /// (tags 7/11/12/13 multi-arg or ToGroup-flattened) / 4 (tag
    /// 18 + tag 11/12 with NonEmptySet+Set).
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ConwayUtxowPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if !(1..=4).contains(&len) {
            return Err(DecoderError(format!(
                "ConwayUtxowPredFailure: expected 1- to 4-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ConwayUtxowPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        let payload_offset = dec.position();
        // Helper: capture remaining bytes as raw payload for the
        // not-yet-typed variants.
        let capture_raw = |label: &str, expected_len: u64| -> Result<Vec<u8>, DecoderError> {
            if len != expected_len {
                return Err(DecoderError(format!(
                    "{label}: expected {expected_len}-element envelope, got len {len}"
                )));
            }
            bytes
                .get(payload_offset..)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| {
                    DecoderError("ConwayUtxowPredFailure: payload offset out of bounds".to_string())
                })
        };
        match tag {
            0 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "UtxoFailure: expected 2-element envelope, got len {len}"
                    )));
                }
                let utxo_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError("ConwayUtxowPredFailure: payload offset out of bounds".to_string())
                })?;
                Ok(Self::UtxoFailure(ConwayUtxoPredFailure::from_cbor(
                    utxo_bytes,
                )?))
            }
            1 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "InvalidWitnessesUTXOW: expected 2-element envelope, got len {len}"
                    )));
                }
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError("ConwayUtxowPredFailure: payload offset out of bounds".to_string())
                })?;
                Ok(Self::InvalidWitnessesUTXOW(NonEmptyVKey::from_cbor(
                    payload_bytes,
                )?))
            }
            2 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "MissingVKeyWitnessesUTXOW: expected 2-element envelope, got len {len}"
                    )));
                }
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError("ConwayUtxowPredFailure: payload offset out of bounds".to_string())
                })?;
                Ok(Self::MissingVKeyWitnessesUTXOW(
                    NonEmptySetKeyHash::from_cbor(payload_bytes)?,
                ))
            }
            3 | 4 | 9 | 16 | 17 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "ConwayUtxowPredFailure tag {tag}: expected 2-element envelope, got len {len}"
                    )));
                }
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError("ConwayUtxowPredFailure: payload offset out of bounds".to_string())
                })?;
                let set = NonEmptySetScriptHash::from_cbor(payload_bytes)?;
                Ok(match tag {
                    3 => Self::MissingScriptWitnessesUTXOW(set),
                    4 => Self::ScriptWitnessNotValidatingUTXOW(set),
                    9 => Self::ExtraneousScriptWitnessesUTXOW(set),
                    16 => Self::MalformedScriptWitnesses(set),
                    17 => Self::MalformedReferenceScripts(set),
                    _ => unreachable!("tag set above"),
                })
            }
            5 | 6 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "ConwayUtxowPredFailure tag {tag}: expected 2-element envelope, got len {len}"
                    )));
                }
                let hash_bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!(
                        "ConwayUtxowPredFailure tag {tag}: expected TxAuxDataHash: {err:?}"
                    ))
                })?;
                let arr: [u8; 32] = hash_bytes.try_into().map_err(|_| {
                    DecoderError(format!(
                        "ConwayUtxowPredFailure tag {tag}: TxAuxDataHash must be 32 bytes, got {}",
                        hash_bytes.len()
                    ))
                })?;
                Ok(match tag {
                    5 => Self::MissingTxBodyMetadataHash(TxAuxDataHash(arr)),
                    6 => Self::MissingTxMetadata(TxAuxDataHash(arr)),
                    _ => unreachable!("tag set above"),
                })
            }
            7 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "ConflictingMetadataHash: expected 3-element envelope, got len {len}"
                    )));
                }
                let supplied = dec.bytes().map_err(|err| {
                    DecoderError(format!("ConflictingMetadataHash: supplied hash: {err:?}"))
                })?;
                let s_arr: [u8; 32] = supplied.try_into().map_err(|_| {
                    DecoderError("ConflictingMetadataHash: supplied not 32 bytes".to_string())
                })?;
                let expected = dec.bytes().map_err(|err| {
                    DecoderError(format!("ConflictingMetadataHash: expected hash: {err:?}"))
                })?;
                let e_arr: [u8; 32] = expected.try_into().map_err(|_| {
                    DecoderError("ConflictingMetadataHash: expected not 32 bytes".to_string())
                })?;
                Ok(Self::ConflictingMetadataHash(Mismatch {
                    relation: MismatchRelation::RelEQ,
                    supplied: TxAuxDataHash(s_arr),
                    expected: TxAuxDataHash(e_arr),
                }))
            }
            8 => {
                if len != 1 {
                    return Err(DecoderError(format!(
                        "InvalidMetadata: expected 1-element envelope, got len {len}"
                    )));
                }
                Ok(Self::InvalidMetadata)
            }
            14 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "UnspendableUTxONoDatumHash: expected 2-element envelope, got len {len}"
                    )));
                }
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError("ConwayUtxowPredFailure: payload offset out of bounds".to_string())
                })?;
                Ok(Self::UnspendableUTxONoDatumHash(
                    NonEmptySetTxIn::from_cbor(payload_bytes)?,
                ))
            }
            // Pending typed decoders: capture raw bytes.
            10 => Ok(Self::MissingRedeemers(capture_raw("MissingRedeemers", 2)?)),
            // Tags 11/12: NonEmptySet DataHash + Set DataHash
            // (R632 typed).
            11 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "MissingRequiredDatums: expected 3-element envelope, got len {len}"
                    )));
                }
                let missing = NonEmptySetDataHash::from_decoder(&mut dec)?;
                let received = SetDataHash::from_decoder(&mut dec)?;
                Ok(Self::MissingRequiredDatums { missing, received })
            }
            12 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "NotAllowedSupplementalDatums: expected 3-element envelope, got len {len}"
                    )));
                }
                let unallowed = NonEmptySetDataHash::from_decoder(&mut dec)?;
                let acceptable = SetDataHash::from_decoder(&mut dec)?;
                Ok(Self::NotAllowedSupplementalDatums {
                    unallowed,
                    acceptable,
                })
            }
            13 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "PPViewHashesDontMatch: expected 3-element envelope, got len {len}"
                    )));
                }
                // ToGroup flattens the Mismatch: supplied then
                // expected.
                let supplied = StrictMaybeScriptIntegrityHash::from_decoder(&mut dec)?;
                let expected = StrictMaybeScriptIntegrityHash::from_decoder(&mut dec)?;
                Ok(Self::PPViewHashesDontMatch(Mismatch {
                    relation: MismatchRelation::RelEQ,
                    supplied,
                    expected,
                }))
            }
            15 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "ExtraRedeemers: expected 2-element envelope, got len {len}"
                    )));
                }
                let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
                    DecoderError("ConwayUtxowPredFailure: payload offset out of bounds".to_string())
                })?;
                Ok(Self::ExtraRedeemers(NonEmptyPlutusPurposeIx::from_cbor(
                    payload_bytes,
                )?))
            }
            18 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "ScriptIntegrityHashMismatch: expected 3-element envelope, got len {len}"
                    )));
                }
                // x: Mismatch encoded as a nested 2-array
                // [supplied, expected].
                let mm_len = dec.array().map_err(|err| {
                    DecoderError(format!(
                        "ScriptIntegrityHashMismatch: expected Mismatch 2-array: {err:?}"
                    ))
                })?;
                if mm_len != 2 {
                    return Err(DecoderError(format!(
                        "ScriptIntegrityHashMismatch: expected 2-element Mismatch, got len {mm_len}"
                    )));
                }
                let supplied = StrictMaybeScriptIntegrityHash::from_decoder(&mut dec)?;
                let expected = StrictMaybeScriptIntegrityHash::from_decoder(&mut dec)?;
                let provided = StrictMaybeBytes::from_decoder(&mut dec)?;
                Ok(Self::ScriptIntegrityHashMismatch {
                    mismatch: Mismatch {
                        relation: MismatchRelation::RelEQ,
                        supplied,
                        expected,
                    },
                    provided,
                })
            }
            other => Err(DecoderError(format!(
                "ConwayUtxowPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ConwayUtxowPredFailure {
    /// Render upstream stock-derived `Show
    /// (ConwayUtxowPredFailure era)`: `<Constructor> <payload>`.
    /// Typed variants route through their typed Display; raw
    /// variants emit a `<raw-cbor N bytes>` marker.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UtxoFailure(utxo) => write!(f, "UtxoFailure ({utxo})"),
            Self::MissingRedeemers(b) => {
                write!(f, "MissingRedeemers <raw-cbor {} bytes>", b.len())
            }
            Self::ScriptIntegrityHashMismatch { mismatch, provided } => {
                write!(f, "ScriptIntegrityHashMismatch ({mismatch}) ({provided})")
            }
            Self::PPViewHashesDontMatch(mm) => {
                write!(f, "PPViewHashesDontMatch ({mm})")
            }
            Self::ExtraRedeemers(purposes) => {
                write!(f, "ExtraRedeemers ({purposes})")
            }
            Self::MissingRequiredDatums { missing, received } => {
                write!(f, "MissingRequiredDatums ({missing}) ({received})")
            }
            Self::NotAllowedSupplementalDatums {
                unallowed,
                acceptable,
            } => {
                write!(
                    f,
                    "NotAllowedSupplementalDatums ({unallowed}) ({acceptable})"
                )
            }
            Self::InvalidWitnessesUTXOW(keys) => {
                write!(f, "InvalidWitnessesUTXOW ({keys})")
            }
            Self::MissingVKeyWitnessesUTXOW(set) => {
                write!(f, "MissingVKeyWitnessesUTXOW ({set})")
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
            Self::MalformedScriptWitnesses(set) => {
                write!(f, "MalformedScriptWitnesses ({set})")
            }
            Self::MalformedReferenceScripts(set) => {
                write!(f, "MalformedReferenceScripts ({set})")
            }
            Self::MissingTxBodyMetadataHash(h) => {
                write!(f, "MissingTxBodyMetadataHash ({h})")
            }
            Self::MissingTxMetadata(h) => write!(f, "MissingTxMetadata ({h})"),
            Self::ConflictingMetadataHash(mm) => {
                write!(f, "ConflictingMetadataHash ({mm})")
            }
            Self::InvalidMetadata => f.write_str("InvalidMetadata"),
            Self::UnspendableUTxONoDatumHash(set) => {
                write!(f, "UnspendableUTxONoDatumHash ({set})")
            }
        }
    }
}

/// `ConwayCertsPredFailure` mirror — Conway-era CERTS sub-rule
/// failure (under `ConwayLedgerPredFailure::ConwayCertsFailure`).
///
/// Upstream: `data ConwayCertsPredFailure era` from
/// `Cardano.Ledger.Conway.Rules.Certs` with 2 variants encoded
/// via CBOR `Sum` tags 0/1. CERTS replaces Shelley's DELEGS at
/// the Conway era.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConwayCertsPredFailure {
    /// Tag 0: withdrawals refer to non-rewarded accounts or
    /// partial-withdraw an amount — `Withdrawals` (R596 typed).
    /// Only emitted at protocol-version < 11 per upstream
    /// comment.
    WithdrawalsNotInRewardsCERTS(Withdrawals),
    /// Tag 1: nested CERT sub-rule failure (R627 wired to typed
    /// `ConwayCertPredFailure`).
    CertFailure(ConwayCertPredFailure),
}

impl ConwayCertsPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::WithdrawalsNotInRewardsCERTS(_) => 0,
            Self::CertFailure(_) => 1,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::WithdrawalsNotInRewardsCERTS(_) => "WithdrawalsNotInRewardsCERTS",
            Self::CertFailure(_) => "CertFailure",
        }
    }

    /// Decode the full `ConwayCertsPredFailure` outer envelope.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ConwayCertsPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if len != 2 {
            return Err(DecoderError(format!(
                "ConwayCertsPredFailure: expected 2-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ConwayCertsPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        let payload_offset = dec.position();
        let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
            DecoderError("ConwayCertsPredFailure: payload offset out of bounds".to_string())
        })?;
        match tag {
            0 => {
                let w = Withdrawals::from_cbor(payload_bytes).map_err(|err| {
                    DecoderError(format!("WithdrawalsNotInRewardsCERTS: {}", err.0))
                })?;
                Ok(Self::WithdrawalsNotInRewardsCERTS(w))
            }
            1 => {
                let cert = ConwayCertPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::CertFailure(cert))
            }
            other => Err(DecoderError(format!(
                "ConwayCertsPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ConwayCertsPredFailure {
    /// Render upstream stock-derived `Show
    /// (ConwayCertsPredFailure era)`. Typed payloads route
    /// through their typed Display; raw variants emit
    /// `<raw-cbor N bytes>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WithdrawalsNotInRewardsCERTS(w) => {
                write!(f, "WithdrawalsNotInRewardsCERTS ({w})")
            }
            Self::CertFailure(b) => {
                write!(f, "CertFailure ({b})")
            }
        }
    }
}

/// `ConwayCertPredFailure` mirror — Conway-era CERT sub-rule
/// failure (nested under `ConwayCertsPredFailure::CertFailure`).
///
/// Upstream: `data ConwayCertPredFailure era` from
/// `Cardano.Ledger.Conway.Rules.Cert` with 3 variants encoded
/// via CBOR `Sum` tags 1/2/3 (upstream skips tag 0). Dispatches
/// into DELEG / POOL / GOVCERT sub-rules.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConwayCertPredFailure {
    /// Tag 1: nested DELEG failure (R628 wired to typed
    /// `ConwayDelegPredFailure`).
    DelegFailure(ConwayDelegPredFailure),
    /// Tag 2: nested POOL failure (R627 wired to typed
    /// `ShelleyPoolPredFailure` — upstream reuses Shelley's POOL
    /// type at the Conway era).
    PoolFailure(ShelleyPoolPredFailure),
    /// Tag 3: nested GOVCERT failure (R629 wired to typed
    /// `ConwayGovCertPredFailure`).
    GovCertFailure(ConwayGovCertPredFailure),
}

impl ConwayCertPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::DelegFailure(_) => 1,
            Self::PoolFailure(_) => 2,
            Self::GovCertFailure(_) => 3,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::DelegFailure(_) => "DelegFailure",
            Self::PoolFailure(_) => "PoolFailure",
            Self::GovCertFailure(_) => "GovCertFailure",
        }
    }

    /// Decode the full `ConwayCertPredFailure` outer envelope.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ConwayCertPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if len != 2 {
            return Err(DecoderError(format!(
                "ConwayCertPredFailure: expected 2-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ConwayCertPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        let payload_offset = dec.position();
        let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
            DecoderError("ConwayCertPredFailure: payload offset out of bounds".to_string())
        })?;
        match tag {
            1 => {
                let deleg = ConwayDelegPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::DelegFailure(deleg))
            }
            2 => {
                let pool = ShelleyPoolPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::PoolFailure(pool))
            }
            3 => {
                let govcert = ConwayGovCertPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::GovCertFailure(govcert))
            }
            other => Err(DecoderError(format!(
                "ConwayCertPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ConwayCertPredFailure {
    /// Render upstream stock-derived `Show
    /// (ConwayCertPredFailure era)`. Tag 2 routes through typed
    /// `ShelleyPoolPredFailure`; tags 1/3 emit raw-cbor markers
    /// pending typed DELEG / GOVCERT decoders.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DelegFailure(deleg) => write!(f, "DelegFailure ({deleg})"),
            Self::PoolFailure(pool) => write!(f, "PoolFailure ({pool})"),
            Self::GovCertFailure(govcert) => {
                write!(f, "GovCertFailure ({govcert})")
            }
        }
    }
}

/// `ConwayDelegPredFailure` mirror — Conway-era DELEG sub-rule
/// failure (under `ConwayCertPredFailure::DelegFailure`).
///
/// Upstream: `data ConwayDelegPredFailure era` from
/// `Cardano.Ledger.Conway.Rules.Deleg` with 8 variants encoded
/// via CBOR `Sum` tags 1-8 (upstream skips tag 0). Conway DELEG
/// differs from Shelley DELEG: it adds DRep delegation, removes
/// MIR-related variants, and uses the nested 2-array Mismatch
/// encoding (not ToGroup-flattened) for tags 7/8.
///
/// R628 ships **all 8 variants fully typed** by reusing existing
/// carriers (Credential, KeyHash, Mismatch<u64>).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConwayDelegPredFailure {
    /// Tag 1: deposit amount in cert mismatches PParams deposit
    /// — `Coin` (R628 typed).
    IncorrectDepositDELEG(u64),
    /// Tag 2: stake key already registered —
    /// `Credential Staking` (R628 typed).
    StakeKeyRegisteredDELEG(Credential),
    /// Tag 3: stake key not registered — `Credential Staking`.
    StakeKeyNotRegisteredDELEG(Credential),
    /// Tag 4: stake key has non-zero account balance — `Coin`.
    StakeKeyHasNonZeroAccountBalanceDELEG(u64),
    /// Tag 5: delegatee DRep not registered —
    /// `Credential DRepRole`.
    DelegateeDRepNotRegisteredDELEG(Credential),
    /// Tag 6: delegatee stake pool not registered —
    /// `KeyHash StakePool`.
    DelegateeStakePoolNotRegisteredDELEG(KeyHash),
    /// Tag 7: deposit mismatch on registration —
    /// `Mismatch RelEQ Coin` (nested 2-array, not
    /// ToGroup-flattened per upstream's `To mm`).
    DepositIncorrectDELEG(Mismatch<u64>),
    /// Tag 8: refund mismatch on unregistration —
    /// `Mismatch RelEQ Coin`.
    RefundIncorrectDELEG(Mismatch<u64>),
}

impl ConwayDelegPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::IncorrectDepositDELEG(_) => 1,
            Self::StakeKeyRegisteredDELEG(_) => 2,
            Self::StakeKeyNotRegisteredDELEG(_) => 3,
            Self::StakeKeyHasNonZeroAccountBalanceDELEG(_) => 4,
            Self::DelegateeDRepNotRegisteredDELEG(_) => 5,
            Self::DelegateeStakePoolNotRegisteredDELEG(_) => 6,
            Self::DepositIncorrectDELEG(_) => 7,
            Self::RefundIncorrectDELEG(_) => 8,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::IncorrectDepositDELEG(_) => "IncorrectDepositDELEG",
            Self::StakeKeyRegisteredDELEG(_) => "StakeKeyRegisteredDELEG",
            Self::StakeKeyNotRegisteredDELEG(_) => "StakeKeyNotRegisteredDELEG",
            Self::StakeKeyHasNonZeroAccountBalanceDELEG(_) => {
                "StakeKeyHasNonZeroAccountBalanceDELEG"
            }
            Self::DelegateeDRepNotRegisteredDELEG(_) => "DelegateeDRepNotRegisteredDELEG",
            Self::DelegateeStakePoolNotRegisteredDELEG(_) => "DelegateeStakePoolNotRegisteredDELEG",
            Self::DepositIncorrectDELEG(_) => "DepositIncorrectDELEG",
            Self::RefundIncorrectDELEG(_) => "RefundIncorrectDELEG",
        }
    }

    /// Decode the full `ConwayDelegPredFailure` outer envelope.
    /// All variants use a 2-element envelope `[tag, payload]`.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ConwayDelegPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if len != 2 {
            return Err(DecoderError(format!(
                "ConwayDelegPredFailure: expected 2-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ConwayDelegPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        match tag {
            1 => {
                let coin = dec
                    .unsigned()
                    .map_err(|err| DecoderError(format!("IncorrectDepositDELEG: coin: {err:?}")))?;
                Ok(Self::IncorrectDepositDELEG(coin))
            }
            2 => {
                let cred = Credential::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("StakeKeyRegisteredDELEG: {}", err.0)))?;
                Ok(Self::StakeKeyRegisteredDELEG(cred))
            }
            3 => {
                let cred = Credential::from_decoder(&mut dec).map_err(|err| {
                    DecoderError(format!("StakeKeyNotRegisteredDELEG: {}", err.0))
                })?;
                Ok(Self::StakeKeyNotRegisteredDELEG(cred))
            }
            4 => {
                let coin = dec.unsigned().map_err(|err| {
                    DecoderError(format!(
                        "StakeKeyHasNonZeroAccountBalanceDELEG: coin: {err:?}"
                    ))
                })?;
                Ok(Self::StakeKeyHasNonZeroAccountBalanceDELEG(coin))
            }
            5 => {
                let cred = Credential::from_decoder(&mut dec).map_err(|err| {
                    DecoderError(format!("DelegateeDRepNotRegisteredDELEG: {}", err.0))
                })?;
                Ok(Self::DelegateeDRepNotRegisteredDELEG(cred))
            }
            6 => {
                let kh_bytes = dec.bytes().map_err(|err| {
                    DecoderError(format!(
                        "DelegateeStakePoolNotRegisteredDELEG: KeyHash bytes: {err:?}"
                    ))
                })?;
                let arr: [u8; 28] = kh_bytes.try_into().map_err(|_| {
                    DecoderError(
                        "DelegateeStakePoolNotRegisteredDELEG: KeyHash must be 28 bytes"
                            .to_string(),
                    )
                })?;
                Ok(Self::DelegateeStakePoolNotRegisteredDELEG(KeyHash(arr)))
            }
            7 => {
                let mm = decode_mismatch_u64(&mut dec, MismatchRelation::RelEQ)
                    .map_err(|err| DecoderError(format!("DepositIncorrectDELEG: {}", err.0)))?;
                Ok(Self::DepositIncorrectDELEG(mm))
            }
            8 => {
                let mm = decode_mismatch_u64(&mut dec, MismatchRelation::RelEQ)
                    .map_err(|err| DecoderError(format!("RefundIncorrectDELEG: {}", err.0)))?;
                Ok(Self::RefundIncorrectDELEG(mm))
            }
            other => Err(DecoderError(format!(
                "ConwayDelegPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ConwayDelegPredFailure {
    /// Render upstream stock-derived `Show
    /// (ConwayDelegPredFailure era)`. All variants typed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncorrectDepositDELEG(c) => {
                write!(f, "IncorrectDepositDELEG ({})", CoinShow(*c))
            }
            Self::StakeKeyRegisteredDELEG(cred) => {
                write!(f, "StakeKeyRegisteredDELEG ({cred})")
            }
            Self::StakeKeyNotRegisteredDELEG(cred) => {
                write!(f, "StakeKeyNotRegisteredDELEG ({cred})")
            }
            Self::StakeKeyHasNonZeroAccountBalanceDELEG(c) => {
                write!(
                    f,
                    "StakeKeyHasNonZeroAccountBalanceDELEG ({})",
                    CoinShow(*c)
                )
            }
            Self::DelegateeDRepNotRegisteredDELEG(cred) => {
                write!(f, "DelegateeDRepNotRegisteredDELEG ({cred})")
            }
            Self::DelegateeStakePoolNotRegisteredDELEG(kh) => {
                write!(f, "DelegateeStakePoolNotRegisteredDELEG ({kh})")
            }
            Self::DepositIncorrectDELEG(mm) => {
                let typed = Mismatch {
                    relation: mm.relation,
                    supplied: CoinShow(mm.supplied),
                    expected: CoinShow(mm.expected),
                };
                write!(f, "DepositIncorrectDELEG ({typed})")
            }
            Self::RefundIncorrectDELEG(mm) => {
                let typed = Mismatch {
                    relation: mm.relation,
                    supplied: CoinShow(mm.supplied),
                    expected: CoinShow(mm.expected),
                };
                write!(f, "RefundIncorrectDELEG ({typed})")
            }
        }
    }
}

/// `ConwayGovCertPredFailure` mirror — Conway-era GOVCERT
/// sub-rule failure (under `ConwayCertPredFailure::GovCertFailure`).
///
/// Upstream: `data ConwayGovCertPredFailure era` from
/// `Cardano.Ledger.Conway.Rules.GovCert` with 6 variants encoded
/// via CBOR `Sum` tags 0-5. Covers DRep registration/refund
/// predicates and committee hot/cold authorization checks.
///
/// R629 ships **all 6 variants fully typed** by reusing existing
/// carriers (Credential, Mismatch<u64>).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConwayGovCertPredFailure {
    /// Tag 0: DRep credential is already registered —
    /// `Credential DRepRole`.
    ConwayDRepAlreadyRegistered(Credential),
    /// Tag 1: DRep credential is not registered —
    /// `Credential DRepRole`.
    ConwayDRepNotRegistered(Credential),
    /// Tag 2: DRep deposit mismatch — `Mismatch RelEQ Coin` via
    /// `ToGroup` flattened encoding (3-element envelope with
    /// supplied-then-expected).
    ConwayDRepIncorrectDeposit(Mismatch<u64>),
    /// Tag 3: committee cold credential previously resigned —
    /// `Credential ColdCommitteeRole`.
    ConwayCommitteeHasPreviouslyResigned(Credential),
    /// Tag 4: DRep refund mismatch — `Mismatch RelEQ Coin` via
    /// `ToGroup` flattened.
    ConwayDRepIncorrectRefund(Mismatch<u64>),
    /// Tag 5: committee cold credential not known —
    /// `Credential ColdCommitteeRole`.
    ConwayCommitteeIsUnknown(Credential),
}

impl ConwayGovCertPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::ConwayDRepAlreadyRegistered(_) => 0,
            Self::ConwayDRepNotRegistered(_) => 1,
            Self::ConwayDRepIncorrectDeposit(_) => 2,
            Self::ConwayCommitteeHasPreviouslyResigned(_) => 3,
            Self::ConwayDRepIncorrectRefund(_) => 4,
            Self::ConwayCommitteeIsUnknown(_) => 5,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::ConwayDRepAlreadyRegistered(_) => "ConwayDRepAlreadyRegistered",
            Self::ConwayDRepNotRegistered(_) => "ConwayDRepNotRegistered",
            Self::ConwayDRepIncorrectDeposit(_) => "ConwayDRepIncorrectDeposit",
            Self::ConwayCommitteeHasPreviouslyResigned(_) => "ConwayCommitteeHasPreviouslyResigned",
            Self::ConwayDRepIncorrectRefund(_) => "ConwayDRepIncorrectRefund",
            Self::ConwayCommitteeIsUnknown(_) => "ConwayCommitteeIsUnknown",
        }
    }

    /// Decode the full `ConwayGovCertPredFailure` outer envelope.
    /// Tags 0/1/3/5 use 2-element envelopes; tags 2/4 use
    /// 3-element ToGroup-flattened envelopes.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ConwayGovCertPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if !(2..=3).contains(&len) {
            return Err(DecoderError(format!(
                "ConwayGovCertPredFailure: expected 2- or 3-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ConwayGovCertPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        let credential_variant = |dec: &mut Decoder<'_>,
                                  label: &str|
         -> Result<Credential, DecoderError> {
            if len != 2 {
                return Err(DecoderError(format!(
                    "{label}: expected 2-element envelope, got len {len}"
                )));
            }
            Credential::from_decoder(dec).map_err(|err| DecoderError(format!("{label}: {}", err.0)))
        };
        let togroup_coin_mismatch =
            |dec: &mut Decoder<'_>, label: &str| -> Result<Mismatch<u64>, DecoderError> {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "{label}: expected 3-element envelope, got len {len}"
                    )));
                }
                let supplied = dec
                    .unsigned()
                    .map_err(|err| DecoderError(format!("{label}: supplied: {err:?}")))?;
                let expected = dec
                    .unsigned()
                    .map_err(|err| DecoderError(format!("{label}: expected: {err:?}")))?;
                Ok(Mismatch {
                    relation: MismatchRelation::RelEQ,
                    supplied,
                    expected,
                })
            };
        match tag {
            0 => Ok(Self::ConwayDRepAlreadyRegistered(credential_variant(
                &mut dec,
                "ConwayDRepAlreadyRegistered",
            )?)),
            1 => Ok(Self::ConwayDRepNotRegistered(credential_variant(
                &mut dec,
                "ConwayDRepNotRegistered",
            )?)),
            2 => Ok(Self::ConwayDRepIncorrectDeposit(togroup_coin_mismatch(
                &mut dec,
                "ConwayDRepIncorrectDeposit",
            )?)),
            3 => Ok(Self::ConwayCommitteeHasPreviouslyResigned(
                credential_variant(&mut dec, "ConwayCommitteeHasPreviouslyResigned")?,
            )),
            4 => Ok(Self::ConwayDRepIncorrectRefund(togroup_coin_mismatch(
                &mut dec,
                "ConwayDRepIncorrectRefund",
            )?)),
            5 => Ok(Self::ConwayCommitteeIsUnknown(credential_variant(
                &mut dec,
                "ConwayCommitteeIsUnknown",
            )?)),
            other => Err(DecoderError(format!(
                "ConwayGovCertPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ConwayGovCertPredFailure {
    /// Render upstream stock-derived `Show
    /// (ConwayGovCertPredFailure era)`. All variants typed —
    /// Credential and Mismatch<CoinShow> route through their
    /// typed Display.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConwayDRepAlreadyRegistered(cred) => {
                write!(f, "ConwayDRepAlreadyRegistered ({cred})")
            }
            Self::ConwayDRepNotRegistered(cred) => {
                write!(f, "ConwayDRepNotRegistered ({cred})")
            }
            Self::ConwayDRepIncorrectDeposit(mm) => {
                let typed = Mismatch {
                    relation: mm.relation,
                    supplied: CoinShow(mm.supplied),
                    expected: CoinShow(mm.expected),
                };
                write!(f, "ConwayDRepIncorrectDeposit ({typed})")
            }
            Self::ConwayCommitteeHasPreviouslyResigned(cred) => {
                write!(f, "ConwayCommitteeHasPreviouslyResigned ({cred})")
            }
            Self::ConwayDRepIncorrectRefund(mm) => {
                let typed = Mismatch {
                    relation: mm.relation,
                    supplied: CoinShow(mm.supplied),
                    expected: CoinShow(mm.expected),
                };
                write!(f, "ConwayDRepIncorrectRefund ({typed})")
            }
            Self::ConwayCommitteeIsUnknown(cred) => {
                write!(f, "ConwayCommitteeIsUnknown ({cred})")
            }
        }
    }
}

/// `ConwayUtxoPredFailure` mirror — Conway-era UTXO sub-rule
/// failure (under `ConwayUtxowPredFailure::UtxoFailure`).
///
/// Upstream: `data ConwayUtxoPredFailure era` from
/// `Cardano.Ledger.Conway.Rules.Utxo` with 23 variants encoded
/// via CBOR `Sum` tags 0-22. The largest sub-rule enum — covers
/// the full UTxO acceptance check (inputs, outputs, fee, value
/// conservation, collateral, network IDs).
///
/// R630 ships the scaffold with typed payloads for the 12
/// variants that reuse existing carriers; the 11 remaining
/// variants (0/2/6/11/12/13/14/15/20/21/22) keep raw inner CBOR
/// pending UTXOS / ValidityInterval / Value / ExUnits /
/// DeltaCoin / NonEmptyMap decoders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConwayUtxoPredFailure {
    /// Tag 0: nested UTXOS sub-rule failure (R631 wired to typed
    /// `ConwayUtxosPredFailure`).
    UtxosFailure(ConwayUtxosPredFailure),
    /// Tag 1: bad transaction inputs — `NonEmptySet TxIn` (R603
    /// typed).
    BadInputsUTxO(NonEmptySetTxIn),
    /// Tag 2: tx validity interval excludes the current slot —
    /// `ValidityInterval + SlotNo` (R634 typed).
    OutsideValidityIntervalUTxO {
        /// The transaction's declared validity interval.
        interval: ValidityInterval,
        /// The current slot at validation time.
        current_slot: u64,
    },
    /// Tag 3: tx size exceeds the maximum — `Mismatch RelLTEQ
    /// Word32` via ToGroup flattened (R630 typed).
    MaxTxSizeUTxO(Mismatch<u64>),
    /// Tag 4: tx input set is empty — no payload.
    InputSetEmptyUTxO,
    /// Tag 5: fee below the minimum — `Mismatch RelGTEQ Coin`
    /// via ToGroup flattened, expected-first per `swapMismatch`
    /// (R630 typed).
    FeeTooSmallUTxO(Mismatch<u64>),
    /// Tag 6: value not conserved (consumed != produced) —
    /// `Mismatch RelEQ (Value era)` via ToGroup flattened,
    /// consumed-first (R642 typed).
    ValueNotConservedUTxO(Mismatch<MaryValue>),
    /// Tag 7: addresses with the wrong network id —
    /// `Network + NonEmptySet Addr` (R630 typed).
    WrongNetwork {
        /// Expected (ledger) network id.
        expected: Network,
        /// Addresses carrying the wrong network id.
        wrongs: NonEmptySetAddr,
    },
    /// Tag 8: reward addresses with the wrong network id —
    /// `Network + NonEmptySet AccountAddress` (R630 typed).
    WrongNetworkWithdrawal {
        /// Expected (ledger) network id.
        expected: Network,
        /// Reward addresses carrying the wrong network id.
        wrongs: NonEmptySetAccountAddress,
    },
    /// Tag 9: outputs below the minimum value — `NonEmpty
    /// (TxOut era)` (R620 typed via NonEmptyTxOut).
    OutputTooSmallUTxO(NonEmptyTxOut),
    /// Tag 10: bootstrap-address outputs with attributes too
    /// big — `NonEmpty (TxOut era)` (R620 typed).
    OutputBootAddrAttrsTooBig(NonEmptyTxOut),
    /// Tag 11: outputs too big — `NonEmpty (Int, Int, TxOut
    /// era)`. Raw pending triple decoder.
    OutputTooBigUTxO(Vec<u8>),
    /// Tag 12: insufficient collateral — `DeltaCoin + Coin`
    /// (R633 typed).
    InsufficientCollateral {
        /// Collateral balance computed (signed delta).
        balance: i64,
        /// Collateral required for the given fee.
        required: u64,
    },
    /// Tag 13: UTxO entries with the wrong script kind —
    /// `NonEmptyMap TxIn (TxOut era)` (R641 typed).
    ScriptsNotPaidUTxO(NonEmptyMapTxInTxOut),
    /// Tag 14: tx execution units exceed the maximum —
    /// `Mismatch RelLTEQ ExUnits` via ToGroup flattened,
    /// expected-first per `swapMismatch` (R637 typed).
    ExUnitsTooBigUTxO(Mismatch<ExUnits>),
    /// Tag 15: collateral inputs contain non-ADA tokens —
    /// `Value era` (R642 typed).
    CollateralContainsNonADA(MaryValue),
    /// Tag 16: wrong network id in tx body — `Mismatch RelEQ
    /// Network` via ToGroup flattened, expected-first per
    /// `swapMismatch` (R630 typed).
    WrongNetworkInTxBody(Mismatch<Network>),
    /// Tag 17: slot outside the consensus forecast range —
    /// `SlotNo` (R630 typed).
    OutsideForecast(u64),
    /// Tag 18: too many collateral inputs — `Mismatch RelLTEQ
    /// Word16` via ToGroup flattened, expected-first per
    /// `swapMismatch` (R630 typed).
    TooManyCollateralInputs(Mismatch<u64>),
    /// Tag 19: no collateral inputs supplied — no payload.
    NoCollateralInputs,
    /// Tag 20: total-collateral field mismatch — `DeltaCoin +
    /// Coin` (R633 typed).
    IncorrectTotalCollateralField {
        /// Collateral provided (signed delta).
        provided: i64,
        /// Collateral amount declared in the transaction body.
        declared: u64,
    },
    /// Tag 21: outputs below the minimum value (Babbage form) —
    /// `NonEmpty (TxOut era, Coin)` (R640 typed).
    BabbageOutputTooSmallUTxO(NonEmptyTxOutCoinPair),
    /// Tag 22: TxIns appearing in both inputs and reference
    /// inputs — `NonEmpty TxIn` (R635 typed).
    BabbageNonDisjointRefInputs(NonEmptyTxIn),
}

impl ConwayUtxoPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::UtxosFailure(_) => 0,
            Self::BadInputsUTxO(_) => 1,
            Self::OutsideValidityIntervalUTxO { .. } => 2,
            Self::MaxTxSizeUTxO(_) => 3,
            Self::InputSetEmptyUTxO => 4,
            Self::FeeTooSmallUTxO(_) => 5,
            Self::ValueNotConservedUTxO(_) => 6,
            Self::WrongNetwork { .. } => 7,
            Self::WrongNetworkWithdrawal { .. } => 8,
            Self::OutputTooSmallUTxO(_) => 9,
            Self::OutputBootAddrAttrsTooBig(_) => 10,
            Self::OutputTooBigUTxO(_) => 11,
            Self::InsufficientCollateral { .. } => 12,
            Self::ScriptsNotPaidUTxO(_) => 13,
            Self::ExUnitsTooBigUTxO(_) => 14,
            Self::CollateralContainsNonADA(_) => 15,
            Self::WrongNetworkInTxBody(_) => 16,
            Self::OutsideForecast(_) => 17,
            Self::TooManyCollateralInputs(_) => 18,
            Self::NoCollateralInputs => 19,
            Self::IncorrectTotalCollateralField { .. } => 20,
            Self::BabbageOutputTooSmallUTxO(_) => 21,
            Self::BabbageNonDisjointRefInputs(_) => 22,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::UtxosFailure(_) => "UtxosFailure",
            Self::BadInputsUTxO(_) => "BadInputsUTxO",
            Self::OutsideValidityIntervalUTxO { .. } => "OutsideValidityIntervalUTxO",
            Self::MaxTxSizeUTxO(_) => "MaxTxSizeUTxO",
            Self::InputSetEmptyUTxO => "InputSetEmptyUTxO",
            Self::FeeTooSmallUTxO(_) => "FeeTooSmallUTxO",
            Self::ValueNotConservedUTxO(_) => "ValueNotConservedUTxO",
            Self::WrongNetwork { .. } => "WrongNetwork",
            Self::WrongNetworkWithdrawal { .. } => "WrongNetworkWithdrawal",
            Self::OutputTooSmallUTxO(_) => "OutputTooSmallUTxO",
            Self::OutputBootAddrAttrsTooBig(_) => "OutputBootAddrAttrsTooBig",
            Self::OutputTooBigUTxO(_) => "OutputTooBigUTxO",
            Self::InsufficientCollateral { .. } => "InsufficientCollateral",
            Self::ScriptsNotPaidUTxO(_) => "ScriptsNotPaidUTxO",
            Self::ExUnitsTooBigUTxO(_) => "ExUnitsTooBigUTxO",
            Self::CollateralContainsNonADA(_) => "CollateralContainsNonADA",
            Self::WrongNetworkInTxBody(_) => "WrongNetworkInTxBody",
            Self::OutsideForecast(_) => "OutsideForecast",
            Self::TooManyCollateralInputs(_) => "TooManyCollateralInputs",
            Self::NoCollateralInputs => "NoCollateralInputs",
            Self::IncorrectTotalCollateralField { .. } => "IncorrectTotalCollateralField",
            Self::BabbageOutputTooSmallUTxO(_) => "BabbageOutputTooSmallUTxO",
            Self::BabbageNonDisjointRefInputs(_) => "BabbageNonDisjointRefInputs",
        }
    }

    /// Decode the full `ConwayUtxoPredFailure` outer envelope.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ConwayUtxoPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if !(1..=4).contains(&len) {
            return Err(DecoderError(format!(
                "ConwayUtxoPredFailure: expected 1- to 4-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ConwayUtxoPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        let payload_offset = dec.position();
        let payload_bytes = bytes.get(payload_offset..).ok_or_else(|| {
            DecoderError("ConwayUtxoPredFailure: payload offset out of bounds".to_string())
        })?;
        let capture_raw = |label: &str, expected_len: u64| -> Result<Vec<u8>, DecoderError> {
            if len != expected_len {
                return Err(DecoderError(format!(
                    "{label}: expected {expected_len}-element envelope, got len {len}"
                )));
            }
            Ok(payload_bytes.to_vec())
        };
        match tag {
            0 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "UtxosFailure: expected 2-element envelope, got len {len}"
                    )));
                }
                let utxos = ConwayUtxosPredFailure::from_cbor(payload_bytes)?;
                Ok(Self::UtxosFailure(utxos))
            }
            1 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "BadInputsUTxO: expected 2-element envelope, got len {len}"
                    )));
                }
                Ok(Self::BadInputsUTxO(NonEmptySetTxIn::from_cbor(
                    payload_bytes,
                )?))
            }
            2 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "OutsideValidityIntervalUTxO: expected 3-element envelope, got len {len}"
                    )));
                }
                let interval = ValidityInterval::from_decoder(&mut dec)?;
                let current_slot = dec.unsigned().map_err(|err| {
                    DecoderError(format!(
                        "OutsideValidityIntervalUTxO: current slot: {err:?}"
                    ))
                })?;
                Ok(Self::OutsideValidityIntervalUTxO {
                    interval,
                    current_slot,
                })
            }
            3 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "MaxTxSizeUTxO: expected 3-element envelope, got len {len}"
                    )));
                }
                let supplied = dec
                    .unsigned()
                    .map_err(|err| DecoderError(format!("MaxTxSizeUTxO: supplied: {err:?}")))?;
                let expected = dec
                    .unsigned()
                    .map_err(|err| DecoderError(format!("MaxTxSizeUTxO: expected: {err:?}")))?;
                Ok(Self::MaxTxSizeUTxO(Mismatch {
                    relation: MismatchRelation::RelLTEQ,
                    supplied,
                    expected,
                }))
            }
            4 => {
                if len != 1 {
                    return Err(DecoderError(format!(
                        "InputSetEmptyUTxO: expected 1-element envelope, got len {len}"
                    )));
                }
                Ok(Self::InputSetEmptyUTxO)
            }
            5 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "FeeTooSmallUTxO: expected 3-element envelope, got len {len}"
                    )));
                }
                // swapMismatch: wire order is expected-then-supplied.
                let expected = dec
                    .unsigned()
                    .map_err(|err| DecoderError(format!("FeeTooSmallUTxO: expected: {err:?}")))?;
                let supplied = dec
                    .unsigned()
                    .map_err(|err| DecoderError(format!("FeeTooSmallUTxO: supplied: {err:?}")))?;
                Ok(Self::FeeTooSmallUTxO(Mismatch {
                    relation: MismatchRelation::RelGTEQ,
                    supplied,
                    expected,
                }))
            }
            6 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "ValueNotConservedUTxO: expected 3-element envelope, got len {len}"
                    )));
                }
                // ToGroup flattened: consumed (supplied) then
                // produced (expected).
                let supplied = MaryValue::from_decoder(&mut dec)?;
                let expected = MaryValue::from_decoder(&mut dec)?;
                Ok(Self::ValueNotConservedUTxO(Mismatch {
                    relation: MismatchRelation::RelEQ,
                    supplied,
                    expected,
                }))
            }
            7 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "WrongNetwork: expected 3-element envelope, got len {len}"
                    )));
                }
                let expected = Network::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("WrongNetwork: {}", err.0)))?;
                let rest = bytes.get(dec.position()..).ok_or_else(|| {
                    DecoderError("WrongNetwork: addr-set offset out of bounds".to_string())
                })?;
                let wrongs = NonEmptySetAddr::from_cbor(rest)?;
                Ok(Self::WrongNetwork { expected, wrongs })
            }
            8 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "WrongNetworkWithdrawal: expected 3-element envelope, got len {len}"
                    )));
                }
                let expected = Network::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("WrongNetworkWithdrawal: {}", err.0)))?;
                let rest = bytes.get(dec.position()..).ok_or_else(|| {
                    DecoderError(
                        "WrongNetworkWithdrawal: acct-set offset out of bounds".to_string(),
                    )
                })?;
                let wrongs = NonEmptySetAccountAddress::from_cbor(rest)?;
                Ok(Self::WrongNetworkWithdrawal { expected, wrongs })
            }
            9 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "OutputTooSmallUTxO: expected 2-element envelope, got len {len}"
                    )));
                }
                Ok(Self::OutputTooSmallUTxO(NonEmptyTxOut::from_cbor(
                    payload_bytes,
                )?))
            }
            10 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "OutputBootAddrAttrsTooBig: expected 2-element envelope, got len {len}"
                    )));
                }
                Ok(Self::OutputBootAddrAttrsTooBig(NonEmptyTxOut::from_cbor(
                    payload_bytes,
                )?))
            }
            11 => Ok(Self::OutputTooBigUTxO(capture_raw("OutputTooBigUTxO", 2)?)),
            12 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "InsufficientCollateral: expected 3-element envelope, got len {len}"
                    )));
                }
                let balance = dec.signed().map_err(|err| {
                    DecoderError(format!("InsufficientCollateral: balance: {err:?}"))
                })?;
                let required = dec.unsigned().map_err(|err| {
                    DecoderError(format!("InsufficientCollateral: required: {err:?}"))
                })?;
                Ok(Self::InsufficientCollateral { balance, required })
            }
            13 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "ScriptsNotPaidUTxO: expected 2-element envelope, got len {len}"
                    )));
                }
                Ok(Self::ScriptsNotPaidUTxO(NonEmptyMapTxInTxOut::from_cbor(
                    payload_bytes,
                )?))
            }
            14 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "ExUnitsTooBigUTxO: expected 3-element envelope, got len {len}"
                    )));
                }
                // swapMismatch: wire order is expected-then-supplied.
                let expected = ExUnits::from_decoder(&mut dec)?;
                let supplied = ExUnits::from_decoder(&mut dec)?;
                Ok(Self::ExUnitsTooBigUTxO(Mismatch {
                    relation: MismatchRelation::RelLTEQ,
                    supplied,
                    expected,
                }))
            }
            15 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "CollateralContainsNonADA: expected 2-element envelope, got len {len}"
                    )));
                }
                Ok(Self::CollateralContainsNonADA(MaryValue::from_decoder(
                    &mut dec,
                )?))
            }
            16 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "WrongNetworkInTxBody: expected 3-element envelope, got len {len}"
                    )));
                }
                // swapMismatch: wire order is expected-then-supplied.
                let expected = Network::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("WrongNetworkInTxBody: {}", err.0)))?;
                let supplied = Network::from_decoder(&mut dec)
                    .map_err(|err| DecoderError(format!("WrongNetworkInTxBody: {}", err.0)))?;
                Ok(Self::WrongNetworkInTxBody(Mismatch {
                    relation: MismatchRelation::RelEQ,
                    supplied,
                    expected,
                }))
            }
            17 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "OutsideForecast: expected 2-element envelope, got len {len}"
                    )));
                }
                let slot = dec
                    .unsigned()
                    .map_err(|err| DecoderError(format!("OutsideForecast: slot: {err:?}")))?;
                Ok(Self::OutsideForecast(slot))
            }
            18 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "TooManyCollateralInputs: expected 3-element envelope, got len {len}"
                    )));
                }
                // swapMismatch: wire order is expected-then-supplied.
                let expected = dec.unsigned().map_err(|err| {
                    DecoderError(format!("TooManyCollateralInputs: expected: {err:?}"))
                })?;
                let supplied = dec.unsigned().map_err(|err| {
                    DecoderError(format!("TooManyCollateralInputs: supplied: {err:?}"))
                })?;
                Ok(Self::TooManyCollateralInputs(Mismatch {
                    relation: MismatchRelation::RelLTEQ,
                    supplied,
                    expected,
                }))
            }
            19 => {
                if len != 1 {
                    return Err(DecoderError(format!(
                        "NoCollateralInputs: expected 1-element envelope, got len {len}"
                    )));
                }
                Ok(Self::NoCollateralInputs)
            }
            20 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "IncorrectTotalCollateralField: expected 3-element envelope, got len {len}"
                    )));
                }
                let provided = dec.signed().map_err(|err| {
                    DecoderError(format!("IncorrectTotalCollateralField: provided: {err:?}"))
                })?;
                let declared = dec.unsigned().map_err(|err| {
                    DecoderError(format!("IncorrectTotalCollateralField: declared: {err:?}"))
                })?;
                Ok(Self::IncorrectTotalCollateralField { provided, declared })
            }
            21 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "BabbageOutputTooSmallUTxO: expected 2-element envelope, got len {len}"
                    )));
                }
                Ok(Self::BabbageOutputTooSmallUTxO(
                    NonEmptyTxOutCoinPair::from_cbor(payload_bytes)?,
                ))
            }
            22 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "BabbageNonDisjointRefInputs: expected 2-element envelope, got len {len}"
                    )));
                }
                Ok(Self::BabbageNonDisjointRefInputs(NonEmptyTxIn::from_cbor(
                    payload_bytes,
                )?))
            }
            other => Err(DecoderError(format!(
                "ConwayUtxoPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ConwayUtxoPredFailure {
    /// Render upstream stock-derived `Show
    /// (ConwayUtxoPredFailure era)`. Typed variants route through
    /// their typed Display; raw variants emit `<raw-cbor N
    /// bytes>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UtxosFailure(utxos) => write!(f, "UtxosFailure ({utxos})"),
            Self::OutputTooBigUTxO(b) => {
                write!(f, "OutputTooBigUTxO <raw-cbor {} bytes>", b.len())
            }
            Self::ValueNotConservedUTxO(mm) => {
                write!(f, "ValueNotConservedUTxO ({mm})")
            }
            Self::CollateralContainsNonADA(value) => {
                write!(f, "CollateralContainsNonADA ({value})")
            }
            Self::ScriptsNotPaidUTxO(map) => {
                write!(f, "ScriptsNotPaidUTxO ({map})")
            }
            Self::BabbageOutputTooSmallUTxO(pairs) => {
                write!(f, "BabbageOutputTooSmallUTxO ({pairs})")
            }
            Self::ExUnitsTooBigUTxO(mm) => write!(f, "ExUnitsTooBigUTxO ({mm})"),
            Self::BabbageNonDisjointRefInputs(ins) => {
                write!(f, "BabbageNonDisjointRefInputs ({ins})")
            }
            Self::OutsideValidityIntervalUTxO {
                interval,
                current_slot,
            } => {
                write!(
                    f,
                    "OutsideValidityIntervalUTxO ({interval}) (SlotNo {{unSlotNo = {current_slot}}})"
                )
            }
            Self::InsufficientCollateral { balance, required } => {
                write!(
                    f,
                    "InsufficientCollateral ({}) ({})",
                    DeltaCoinShow(*balance),
                    CoinShow(*required)
                )
            }
            Self::IncorrectTotalCollateralField { provided, declared } => {
                write!(
                    f,
                    "IncorrectTotalCollateralField ({}) ({})",
                    DeltaCoinShow(*provided),
                    CoinShow(*declared)
                )
            }
            Self::BadInputsUTxO(set) => write!(f, "BadInputsUTxO ({set})"),
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
            Self::WrongNetwork { expected, wrongs } => {
                write!(f, "WrongNetwork {expected} ({wrongs})")
            }
            Self::WrongNetworkWithdrawal { expected, wrongs } => {
                write!(f, "WrongNetworkWithdrawal {expected} ({wrongs})")
            }
            Self::OutputTooSmallUTxO(outs) => {
                write!(f, "OutputTooSmallUTxO ({outs})")
            }
            Self::OutputBootAddrAttrsTooBig(outs) => {
                write!(f, "OutputBootAddrAttrsTooBig ({outs})")
            }
            Self::WrongNetworkInTxBody(mm) => {
                write!(f, "WrongNetworkInTxBody ({mm})")
            }
            Self::OutsideForecast(slot) => {
                write!(f, "OutsideForecast (SlotNo {slot})")
            }
            Self::TooManyCollateralInputs(mm) => {
                write!(f, "TooManyCollateralInputs ({mm})")
            }
            Self::NoCollateralInputs => f.write_str("NoCollateralInputs"),
        }
    }
}

/// `FailureDescription` mirror from
/// `Cardano.Ledger.Alonzo.Rules.Utxos`. Single variant
/// `PlutusFailure Text ByteString`; CBOR `Sum` tag 1 (upstream
/// deliberately skips tag 0 — a removed legacy constructor).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureDescription {
    /// Human-readable failure explanation.
    pub message: String,
    /// Base64-encoded reconstruction context (raw bytes).
    pub context: Vec<u8>,
}

impl FailureDescription {
    /// Decode `FailureDescription` from the canonical 3-element
    /// CBOR `Sum` envelope `[1, text, bytes]`.
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let len = dec.array().map_err(|err| {
            DecoderError(format!("FailureDescription: expected CBOR array: {err:?}"))
        })?;
        if len != 3 {
            return Err(DecoderError(format!(
                "FailureDescription: expected 3-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!("FailureDescription: expected Word8 tag: {err:?}"))
        })?;
        if tag != 1 {
            return Err(DecoderError(format!(
                "FailureDescription: unknown tag {tag} (only tag 1 PlutusFailure is valid)"
            )));
        }
        let message = dec
            .text_owned()
            .map_err(|err| DecoderError(format!("FailureDescription: expected text: {err:?}")))?;
        let context = dec
            .bytes()
            .map_err(|err| DecoderError(format!("FailureDescription: expected bytes: {err:?}")))?
            .to_vec();
        Ok(Self { message, context })
    }
}

impl fmt::Display for FailureDescription {
    /// Render upstream stock-derived `Show FailureDescription`:
    /// `PlutusFailure <text> <bytestring>`. The Text is rendered
    /// with Haskell `Show String` escapes; the reconstruction
    /// ByteString (often a large base64 blob) is rendered as a
    /// hex marker.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PlutusFailure {} <bytestring {} bytes>",
            show_haskell_bytestring_like(&self.message),
            self.context.len()
        )
    }
}

/// `TagMismatchDescription` mirror from
/// `Cardano.Ledger.Alonzo.Rules.Utxos`. CBOR `Sum` tags 0/1.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TagMismatchDescription {
    /// Tag 0: scripts passed when the tx claimed they would fail.
    PassedUnexpectedly,
    /// Tag 1: scripts failed when the tx claimed they would pass
    /// — `NonEmpty FailureDescription`.
    FailedUnexpectedly(Vec<FailureDescription>),
}

impl TagMismatchDescription {
    /// Decode `TagMismatchDescription` from its CBOR `Sum`
    /// envelope (1-element for tag 0, 2-element for tag 1).
    fn from_decoder(dec: &mut yggdrasil_ledger::Decoder<'_>) -> Result<Self, DecoderError> {
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "TagMismatchDescription: expected CBOR array: {err:?}"
            ))
        })?;
        if !(1..=2).contains(&len) {
            return Err(DecoderError(format!(
                "TagMismatchDescription: expected 1- or 2-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "TagMismatchDescription: expected Word8 tag: {err:?}"
            ))
        })?;
        match tag {
            0 => {
                if len != 1 {
                    return Err(DecoderError(format!(
                        "PassedUnexpectedly: expected 1-element envelope, got len {len}"
                    )));
                }
                Ok(Self::PassedUnexpectedly)
            }
            1 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "FailedUnexpectedly: expected 2-element envelope, got len {len}"
                    )));
                }
                let count = dec.array().map_err(|err| {
                    DecoderError(format!(
                        "FailedUnexpectedly: expected NonEmpty array: {err:?}"
                    ))
                })?;
                if count == 0 {
                    return Err(DecoderError(
                        "FailedUnexpectedly: NonEmpty requires at least one entry".to_string(),
                    ));
                }
                let mut entries = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    entries.push(FailureDescription::from_decoder(dec)?);
                }
                Ok(Self::FailedUnexpectedly(entries))
            }
            other => Err(DecoderError(format!(
                "TagMismatchDescription: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for TagMismatchDescription {
    /// Render upstream stock-derived `Show TagMismatchDescription`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PassedUnexpectedly => f.write_str("PassedUnexpectedly"),
            Self::FailedUnexpectedly(entries) => {
                let (head, tail) = entries
                    .split_first()
                    .expect("FailedUnexpectedly enforces ≥1 entry at decode time");
                write!(f, "FailedUnexpectedly ({head} :| [")?;
                let mut first = true;
                for e in tail {
                    if !first {
                        f.write_str(",")?;
                    }
                    first = false;
                    write!(f, "{e}")?;
                }
                f.write_str("])")
            }
        }
    }
}

/// `ConwayUtxosPredFailure` mirror — Conway-era UTXOS sub-rule
/// failure (under `ConwayUtxoPredFailure::UtxosFailure`).
///
/// Upstream: `data ConwayUtxosPredFailure era` from
/// `Cardano.Ledger.Conway.Rules.Utxos` with 2 variants encoded
/// via CBOR `Sum` tags 0/1. The UTXOS rule is the Plutus
/// script-evaluation phase of UTxO validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConwayUtxosPredFailure {
    /// Tag 0: the tx `isValid` tag disagrees with script
    /// evaluation — `IsValid + TagMismatchDescription` (R631
    /// typed).
    ValidationTagMismatch {
        /// The `isValid` flag the transaction declared.
        is_valid: bool,
        /// Why the script evaluation disagreed.
        description: TagMismatchDescription,
    },
    /// Tag 1: could not collect all Plutus script inputs —
    /// `NonEmpty (CollectError era)`. Raw pending CollectError
    /// decoder.
    CollectErrors(Vec<u8>),
}

impl ConwayUtxosPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::ValidationTagMismatch { .. } => 0,
            Self::CollectErrors(_) => 1,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::ValidationTagMismatch { .. } => "ValidationTagMismatch",
            Self::CollectErrors(_) => "CollectErrors",
        }
    }

    /// Decode the full `ConwayUtxosPredFailure` outer envelope.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ConwayUtxosPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if !(2..=3).contains(&len) {
            return Err(DecoderError(format!(
                "ConwayUtxosPredFailure: expected 2- or 3-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!(
                "ConwayUtxosPredFailure: expected Word8 tag: {err:?}"
            ))
        })?;
        match tag {
            0 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "ValidationTagMismatch: expected 3-element envelope, got len {len}"
                    )));
                }
                let is_valid = dec.bool().map_err(|err| {
                    DecoderError(format!("ValidationTagMismatch: isValid: {err:?}"))
                })?;
                let description = TagMismatchDescription::from_decoder(&mut dec)?;
                Ok(Self::ValidationTagMismatch {
                    is_valid,
                    description,
                })
            }
            1 => {
                if len != 2 {
                    return Err(DecoderError(format!(
                        "CollectErrors: expected 2-element envelope, got len {len}"
                    )));
                }
                let payload_offset = dec.position();
                let raw = bytes
                    .get(payload_offset..)
                    .ok_or_else(|| {
                        DecoderError(
                            "ConwayUtxosPredFailure: payload offset out of bounds".to_string(),
                        )
                    })?
                    .to_vec();
                Ok(Self::CollectErrors(raw))
            }
            other => Err(DecoderError(format!(
                "ConwayUtxosPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ConwayUtxosPredFailure {
    /// Render upstream stock-derived `Show
    /// (ConwayUtxosPredFailure era)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ValidationTagMismatch {
                is_valid,
                description,
            } => {
                write!(
                    f,
                    "ValidationTagMismatch (IsValid {}) ({description})",
                    if *is_valid { "True" } else { "False" }
                )
            }
            Self::CollectErrors(b) => {
                write!(f, "CollectErrors <raw-cbor {} bytes>", b.len())
            }
        }
    }
}

/// `ConwayGovPredFailure` mirror — Conway-era GOV sub-rule
/// failure (under `ConwayLedgerPredFailure::ConwayGovFailure`).
///
/// Upstream: `data ConwayGovPredFailure era` from
/// `Cardano.Ledger.Conway.Rules.Gov` with 19 variants encoded
/// via CBOR `Sum` tags 0-18. The variants encode every governance
/// rule failure (proposal validation, voting eligibility,
/// committee updates, etc.).
///
/// R626 ships the scaffold + typed payload for tag 4
/// (ProposalDepositIncorrect — `Mismatch RelEQ Coin` via
/// `ToGroup`). The remaining 18 variants keep raw inner CBOR
/// pending typed governance-specific decoders (GovActionId,
/// GovAction, Voter, ProposalProcedure, ProtVer, Credential
/// roles, etc.).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConwayGovPredFailure {
    /// Tag 0: governance actions referenced by votes do not exist
    /// — `NonEmpty GovActionId`. Raw pending GovActionId decoder.
    GovActionsDoNotExist(Vec<u8>),
    /// Tag 1: proposal is malformed — `GovAction era`. Raw
    /// pending GovAction decoder.
    MalformedProposal(Vec<u8>),
    /// Tag 2: proposal account address is on wrong network —
    /// `AccountAddress + Network`. Raw pending AccountAddress
    /// decoder.
    ProposalProcedureNetworkIdMismatch(Vec<u8>),
    /// Tag 3: treasury withdrawal account addresses on wrong
    /// network — `NonEmptySet AccountAddress + Network`. Raw.
    TreasuryWithdrawalsNetworkIdMismatch(Vec<u8>),
    /// Tag 4: proposal deposit value mismatch — `Mismatch RelEQ
    /// Coin` via ToGroup flattened (R626 typed).
    ProposalDepositIncorrect(Mismatch<u64>),
    /// Tag 5: disallowed voters for specific actions —
    /// `NonEmpty (Voter, GovActionId)`. Raw.
    DisallowedVoters(Vec<u8>),
    /// Tag 6: cold-committee credentials both removed and added
    /// — `NonEmptySet (Credential ColdCommitteeRole)`. Raw.
    ConflictingCommitteeUpdate(Vec<u8>),
    /// Tag 7: committee expiration epoch too small —
    /// `NonEmptyMap (Credential ColdCommitteeRole) EpochNo`.
    /// Raw.
    ExpirationEpochTooSmall(Vec<u8>),
    /// Tag 8: invalid previous gov-action id —
    /// `ProposalProcedure era`. Raw.
    InvalidPrevGovActionId(Vec<u8>),
    /// Tag 9: voting on expired gov action —
    /// `NonEmpty (Voter, GovActionId)`. Raw.
    VotingOnExpiredGovAction(Vec<u8>),
    /// Tag 10: hard-fork proposal protocol-version sequence —
    /// `StrictMaybe (GovPurposeId 'HardForkPurpose) + Mismatch
    /// RelGT ProtVer` via ToGroup. Raw.
    ProposalCantFollow(Vec<u8>),
    /// Tag 11: guardrails-script-hash mismatch —
    /// `StrictMaybe ScriptHash + StrictMaybe ScriptHash`. Raw.
    InvalidGuardrailsScriptHash(Vec<u8>),
    /// Tag 12: proposal not allowed during bootstrap —
    /// `ProposalProcedure era`. Raw.
    DisallowedProposalDuringBootstrap(Vec<u8>),
    /// Tag 13: votes not allowed during bootstrap —
    /// `NonEmpty (Voter, GovActionId)`. Raw.
    DisallowedVotesDuringBootstrap(Vec<u8>),
    /// Tag 14: voters do not exist in ledger state —
    /// `NonEmpty Voter`. Raw.
    VotersDoNotExist(Vec<u8>),
    /// Tag 15: treasury withdrawals sum to zero —
    /// `GovAction era`. Raw.
    ZeroTreasuryWithdrawals(Vec<u8>),
    /// Tag 16: proposal return-account address does not exist —
    /// `AccountAddress`. Raw.
    ProposalReturnAccountDoesNotExist(Vec<u8>),
    /// Tag 17: treasury withdrawal return-accounts do not exist
    /// — `NonEmpty AccountAddress`. Raw.
    TreasuryWithdrawalReturnAccountsDoNotExist(Vec<u8>),
    /// Tag 18: votes by unelected committee members —
    /// `NonEmpty (Credential HotCommitteeRole)`. Raw.
    UnelectedCommitteeVoters(Vec<u8>),
}

impl ConwayGovPredFailure {
    /// Upstream CBOR tag for this variant.
    pub fn tag(&self) -> u8 {
        match self {
            Self::GovActionsDoNotExist(_) => 0,
            Self::MalformedProposal(_) => 1,
            Self::ProposalProcedureNetworkIdMismatch(_) => 2,
            Self::TreasuryWithdrawalsNetworkIdMismatch(_) => 3,
            Self::ProposalDepositIncorrect(_) => 4,
            Self::DisallowedVoters(_) => 5,
            Self::ConflictingCommitteeUpdate(_) => 6,
            Self::ExpirationEpochTooSmall(_) => 7,
            Self::InvalidPrevGovActionId(_) => 8,
            Self::VotingOnExpiredGovAction(_) => 9,
            Self::ProposalCantFollow(_) => 10,
            Self::InvalidGuardrailsScriptHash(_) => 11,
            Self::DisallowedProposalDuringBootstrap(_) => 12,
            Self::DisallowedVotesDuringBootstrap(_) => 13,
            Self::VotersDoNotExist(_) => 14,
            Self::ZeroTreasuryWithdrawals(_) => 15,
            Self::ProposalReturnAccountDoesNotExist(_) => 16,
            Self::TreasuryWithdrawalReturnAccountsDoNotExist(_) => 17,
            Self::UnelectedCommitteeVoters(_) => 18,
        }
    }

    /// Upstream stock-derived `Show` constructor name.
    pub fn constructor(&self) -> &'static str {
        match self {
            Self::GovActionsDoNotExist(_) => "GovActionsDoNotExist",
            Self::MalformedProposal(_) => "MalformedProposal",
            Self::ProposalProcedureNetworkIdMismatch(_) => "ProposalProcedureNetworkIdMismatch",
            Self::TreasuryWithdrawalsNetworkIdMismatch(_) => "TreasuryWithdrawalsNetworkIdMismatch",
            Self::ProposalDepositIncorrect(_) => "ProposalDepositIncorrect",
            Self::DisallowedVoters(_) => "DisallowedVoters",
            Self::ConflictingCommitteeUpdate(_) => "ConflictingCommitteeUpdate",
            Self::ExpirationEpochTooSmall(_) => "ExpirationEpochTooSmall",
            Self::InvalidPrevGovActionId(_) => "InvalidPrevGovActionId",
            Self::VotingOnExpiredGovAction(_) => "VotingOnExpiredGovAction",
            Self::ProposalCantFollow(_) => "ProposalCantFollow",
            Self::InvalidGuardrailsScriptHash(_) => "InvalidGuardrailsScriptHash",
            Self::DisallowedProposalDuringBootstrap(_) => "DisallowedProposalDuringBootstrap",
            Self::DisallowedVotesDuringBootstrap(_) => "DisallowedVotesDuringBootstrap",
            Self::VotersDoNotExist(_) => "VotersDoNotExist",
            Self::ZeroTreasuryWithdrawals(_) => "ZeroTreasuryWithdrawals",
            Self::ProposalReturnAccountDoesNotExist(_) => "ProposalReturnAccountDoesNotExist",
            Self::TreasuryWithdrawalReturnAccountsDoNotExist(_) => {
                "TreasuryWithdrawalReturnAccountsDoNotExist"
            }
            Self::UnelectedCommitteeVoters(_) => "UnelectedCommitteeVoters",
        }
    }

    /// Decode the full `ConwayGovPredFailure` outer envelope. Most
    /// variants use a 2-element envelope; tags 2/3/11 use 3-element
    /// (two args); tag 4 uses 3-element ToGroup-flattened (Mismatch
    /// `supplied, expected`); tag 10 uses 4-element (StrictMaybe +
    /// ToGroup Mismatch).
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, DecoderError> {
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(bytes);
        let len = dec.array().map_err(|err| {
            DecoderError(format!(
                "ConwayGovPredFailure: expected outer CBOR array: {err:?}"
            ))
        })?;
        if !(2..=4).contains(&len) {
            return Err(DecoderError(format!(
                "ConwayGovPredFailure: expected 2- to 4-element array, got len {len}"
            )));
        }
        let tag = dec.unsigned().map_err(|err| {
            DecoderError(format!("ConwayGovPredFailure: expected Word8 tag: {err:?}"))
        })?;
        let payload_offset = dec.position();
        let capture_raw = |label: &str, expected_len: u64| -> Result<Vec<u8>, DecoderError> {
            if len != expected_len {
                return Err(DecoderError(format!(
                    "{label}: expected {expected_len}-element envelope, got len {len}"
                )));
            }
            bytes
                .get(payload_offset..)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| {
                    DecoderError("ConwayGovPredFailure: payload offset out of bounds".to_string())
                })
        };
        match tag {
            // Tag 4: typed ProposalDepositIncorrect — Mismatch
            // RelEQ Coin via ToGroup flattened.
            4 => {
                if len != 3 {
                    return Err(DecoderError(format!(
                        "ProposalDepositIncorrect: expected 3-element envelope, got len {len}"
                    )));
                }
                let supplied = dec.unsigned().map_err(|err| {
                    DecoderError(format!("ProposalDepositIncorrect: supplied: {err:?}"))
                })?;
                let expected = dec.unsigned().map_err(|err| {
                    DecoderError(format!("ProposalDepositIncorrect: expected: {err:?}"))
                })?;
                Ok(Self::ProposalDepositIncorrect(Mismatch {
                    relation: MismatchRelation::RelEQ,
                    supplied,
                    expected,
                }))
            }
            // Raw variants with various envelope lengths.
            0 => Ok(Self::GovActionsDoNotExist(capture_raw(
                "GovActionsDoNotExist",
                2,
            )?)),
            1 => Ok(Self::MalformedProposal(capture_raw(
                "MalformedProposal",
                2,
            )?)),
            2 => Ok(Self::ProposalProcedureNetworkIdMismatch(capture_raw(
                "ProposalProcedureNetworkIdMismatch",
                3,
            )?)),
            3 => Ok(Self::TreasuryWithdrawalsNetworkIdMismatch(capture_raw(
                "TreasuryWithdrawalsNetworkIdMismatch",
                3,
            )?)),
            5 => Ok(Self::DisallowedVoters(capture_raw("DisallowedVoters", 2)?)),
            6 => Ok(Self::ConflictingCommitteeUpdate(capture_raw(
                "ConflictingCommitteeUpdate",
                2,
            )?)),
            7 => Ok(Self::ExpirationEpochTooSmall(capture_raw(
                "ExpirationEpochTooSmall",
                2,
            )?)),
            8 => Ok(Self::InvalidPrevGovActionId(capture_raw(
                "InvalidPrevGovActionId",
                2,
            )?)),
            9 => Ok(Self::VotingOnExpiredGovAction(capture_raw(
                "VotingOnExpiredGovAction",
                2,
            )?)),
            10 => Ok(Self::ProposalCantFollow(capture_raw(
                "ProposalCantFollow",
                4,
            )?)),
            11 => Ok(Self::InvalidGuardrailsScriptHash(capture_raw(
                "InvalidGuardrailsScriptHash",
                3,
            )?)),
            12 => Ok(Self::DisallowedProposalDuringBootstrap(capture_raw(
                "DisallowedProposalDuringBootstrap",
                2,
            )?)),
            13 => Ok(Self::DisallowedVotesDuringBootstrap(capture_raw(
                "DisallowedVotesDuringBootstrap",
                2,
            )?)),
            14 => Ok(Self::VotersDoNotExist(capture_raw("VotersDoNotExist", 2)?)),
            15 => Ok(Self::ZeroTreasuryWithdrawals(capture_raw(
                "ZeroTreasuryWithdrawals",
                2,
            )?)),
            16 => Ok(Self::ProposalReturnAccountDoesNotExist(capture_raw(
                "ProposalReturnAccountDoesNotExist",
                2,
            )?)),
            17 => Ok(Self::TreasuryWithdrawalReturnAccountsDoNotExist(
                capture_raw("TreasuryWithdrawalReturnAccountsDoNotExist", 2)?,
            )),
            18 => Ok(Self::UnelectedCommitteeVoters(capture_raw(
                "UnelectedCommitteeVoters",
                2,
            )?)),
            other => Err(DecoderError(format!(
                "ConwayGovPredFailure: unknown variant tag {other}"
            ))),
        }
    }
}

impl fmt::Display for ConwayGovPredFailure {
    /// Render upstream stock-derived `Show
    /// (ConwayGovPredFailure era)`. Tag 4 routes through typed
    /// `Mismatch<CoinShow>`; all other variants emit `<raw-cbor
    /// N bytes>` until their typed governance-specific decoders
    /// land.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProposalDepositIncorrect(mm) => {
                let typed = Mismatch {
                    relation: mm.relation,
                    supplied: CoinShow(mm.supplied),
                    expected: CoinShow(mm.expected),
                };
                write!(f, "ProposalDepositIncorrect ({typed})")
            }
            Self::GovActionsDoNotExist(b)
            | Self::MalformedProposal(b)
            | Self::ProposalProcedureNetworkIdMismatch(b)
            | Self::TreasuryWithdrawalsNetworkIdMismatch(b)
            | Self::DisallowedVoters(b)
            | Self::ConflictingCommitteeUpdate(b)
            | Self::ExpirationEpochTooSmall(b)
            | Self::InvalidPrevGovActionId(b)
            | Self::VotingOnExpiredGovAction(b)
            | Self::ProposalCantFollow(b)
            | Self::InvalidGuardrailsScriptHash(b)
            | Self::DisallowedProposalDuringBootstrap(b)
            | Self::DisallowedVotesDuringBootstrap(b)
            | Self::VotersDoNotExist(b)
            | Self::ZeroTreasuryWithdrawals(b)
            | Self::ProposalReturnAccountDoesNotExist(b)
            | Self::TreasuryWithdrawalReturnAccountsDoNotExist(b)
            | Self::UnelectedCommitteeVoters(b) => {
                write!(f, "{} <raw-cbor {} bytes>", self.constructor(), b.len())
            }
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
        assert_eq!(
            ShelleyLedgerPredFailure::DelegsFailure(ShelleyDelegsPredFailure::DelplFailure(
                ShelleyDelplPredFailure::PoolFailure(
                    ShelleyPoolPredFailure::StakePoolNotRegisteredOnKeyPOOL(KeyHash([0_u8; 28]))
                )
            ))
            .tag(),
            1
        );
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
            ShelleyLedgerPredFailure::DelegsFailure(ShelleyDelegsPredFailure::DelplFailure(
                ShelleyDelplPredFailure::PoolFailure(
                    ShelleyPoolPredFailure::StakePoolNotRegisteredOnKeyPOOL(KeyHash([0_u8; 28]))
                )
            ))
            .constructor(),
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
    fn shelley_ledger_pred_failure_display_routes_typed_delegs() {
        // R612 wired DelegsFailure to typed ShelleyDelegsPredFailure;
        // R613 wired the inner DELPL payload; R614 wired the deeper
        // POOL sub-rule. The LEDGER → DELEGS → DELPL → POOL Display
        // chain renders typed end-to-end now.
        let f = ShelleyLedgerPredFailure::DelegsFailure(ShelleyDelegsPredFailure::DelplFailure(
            ShelleyDelplPredFailure::PoolFailure(
                ShelleyPoolPredFailure::StakePoolNotRegisteredOnKeyPOOL(KeyHash([0x77_u8; 28])),
            ),
        ));
        let s = f.to_string();
        assert!(
            s.starts_with(
                "DelegsFailure (DelplFailure (PoolFailure (StakePoolNotRegisteredOnKeyPOOL (KeyHash {unKeyHash = \"7777"
            ),
            "got: {s}"
        );
    }

    #[test]
    fn shelley_delegs_pred_failure_from_cbor_decodes_tag1() {
        // outer DELEGS [0x82, 0x01, inner-DELPL]; inner-DELPL
        // [0x82, 0x00, inner-POOL]; inner-POOL
        // [0x82, 0x00, bytes(28)] for tag-0 StakePoolNotRegisteredOnKeyPOOL.
        let mut cbor = vec![0x82_u8, 0x01, 0x82, 0x00, 0x82, 0x00];
        cbor.push(0x58); // bytes header
        cbor.push(28);
        cbor.extend_from_slice(&[0x42_u8; 28]);
        let f = ShelleyDelegsPredFailure::from_cbor(&cbor).expect("DelplFailure");
        let ShelleyDelegsPredFailure::DelplFailure(delpl) = &f;
        assert_eq!(delpl.tag(), 0);
        assert_eq!(delpl.constructor(), "PoolFailure");
        if let ShelleyDelplPredFailure::PoolFailure(pool) = delpl {
            assert_eq!(pool.tag(), 0);
            assert!(matches!(
                pool,
                ShelleyPoolPredFailure::StakePoolNotRegisteredOnKeyPOOL(_)
            ));
        } else {
            panic!("expected typed PoolFailure inside DELPL");
        }
        assert_eq!(f.tag(), 1);
        let s = f.to_string();
        assert!(
            s.contains(
                "DelplFailure (PoolFailure (StakePoolNotRegisteredOnKeyPOOL (KeyHash {unKeyHash = \"4242"
            ),
            "got: {s}"
        );
    }

    #[test]
    fn shelley_delpl_pred_failure_pool_failure_decodes_tag0() {
        // Inner POOL envelope [0x82, 0x00, bytes(28)]
        let mut cbor = vec![0x82_u8, 0x00, 0x82, 0x00];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x33_u8; 28]);
        let f = ShelleyDelplPredFailure::from_cbor(&cbor).expect("PoolFailure");
        if let ShelleyDelplPredFailure::PoolFailure(pool) = &f {
            assert_eq!(pool.tag(), 0);
        } else {
            panic!("expected typed PoolFailure, got {f:?}");
        }
        assert_eq!(f.tag(), 0);
        assert_eq!(f.constructor(), "PoolFailure");
    }

    #[test]
    fn shelley_pool_pred_failure_stake_pool_not_registered_decodes_tag0() {
        // outer [0x82, 0x00, bytes(28)]
        let mut cbor = vec![0x82_u8, 0x00];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0xAB_u8; 28]);
        let f = ShelleyPoolPredFailure::from_cbor(&cbor).expect("StakePoolNotRegisteredOnKeyPOOL");
        if let ShelleyPoolPredFailure::StakePoolNotRegisteredOnKeyPOOL(kh) = &f {
            assert_eq!(kh.0, [0xAB_u8; 28]);
        } else {
            panic!("expected StakePoolNotRegisteredOnKeyPOOL, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("StakePoolNotRegisteredOnKeyPOOL (KeyHash {unKeyHash = \"abab"),
            "got: {f}"
        );
    }

    #[test]
    fn shelley_pool_pred_failure_cost_too_low_decodes_tag3() {
        // tag 3 (StakePoolCostTooLowPOOL): Mismatch RelGTEQ Coin
        // — outer [0x83, 0x03, supplied=100, expected=200]
        let cbor = [0x83_u8, 0x03, 0x18, 0x64, 0x18, 0xC8];
        let f = ShelleyPoolPredFailure::from_cbor(&cbor).expect("StakePoolCostTooLowPOOL");
        if let ShelleyPoolPredFailure::StakePoolCostTooLowPOOL(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelGTEQ);
            assert_eq!(mm.supplied, 100);
            assert_eq!(mm.expected, 200);
        } else {
            panic!("expected typed StakePoolCostTooLowPOOL, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "StakePoolCostTooLowPOOL (Mismatch (RelGTEQ) {supplied: Coin 100, expected: Coin 200})"
        );
    }

    #[test]
    fn shelley_pool_pred_failure_wrong_network_decodes_tag4() {
        // outer [0x84, 0x04, expected=1, supplied=0, bytes(28)]
        let mut cbor = vec![0x84_u8, 0x04, 0x01, 0x00];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x33_u8; 28]);
        let f = ShelleyPoolPredFailure::from_cbor(&cbor).expect("WrongNetworkPOOL");
        if let ShelleyPoolPredFailure::WrongNetworkPOOL {
            expected,
            supplied,
            pool_id,
        } = &f
        {
            assert_eq!(*expected, Network::Mainnet);
            assert_eq!(*supplied, Network::Testnet);
            assert_eq!(pool_id.0, [0x33_u8; 28]);
        } else {
            panic!("expected typed WrongNetworkPOOL, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with("WrongNetworkPOOL (Mismatch (RelEQ) {supplied: Testnet, expected: Mainnet}) (KeyHash {unKeyHash = \"3333"),
            "got: {s}"
        );
    }

    #[test]
    fn shelley_pool_pred_failure_metadata_hash_too_big_decodes_tag5() {
        // outer [0x83, 0x05, bytes(28), size=4096]
        let mut cbor = vec![0x83_u8, 0x05];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x44_u8; 28]);
        cbor.extend_from_slice(&[0x19, 0x10, 0x00]); // 4096
        let f = ShelleyPoolPredFailure::from_cbor(&cbor).expect("PoolMedataHashTooBig");
        if let ShelleyPoolPredFailure::PoolMedataHashTooBig { pool_id, size } = &f {
            assert_eq!(pool_id.0, [0x44_u8; 28]);
            assert_eq!(*size, 4096);
        } else {
            panic!("expected typed PoolMedataHashTooBig, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with("PoolMedataHashTooBig (KeyHash {unKeyHash = \"4444"),
            "got: {s}"
        );
        assert!(s.ends_with(") 4096"), "got: {s}");
    }

    #[test]
    fn shelley_pool_pred_failure_vrf_already_registered_decodes_tag6() {
        // outer [0x83, 0x06, bytes(28), bytes(32)]
        let mut cbor = vec![0x83_u8, 0x06];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x55_u8; 28]);
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0x66_u8; 32]);
        let f = ShelleyPoolPredFailure::from_cbor(&cbor).expect("VRFKeyHashAlreadyRegistered");
        if let ShelleyPoolPredFailure::VRFKeyHashAlreadyRegistered {
            pool_id,
            vrf_key_hash,
        } = &f
        {
            assert_eq!(pool_id.0, [0x55_u8; 28]);
            assert_eq!(vrf_key_hash.0, [0x66_u8; 32]);
        } else {
            panic!("expected typed VRFKeyHashAlreadyRegistered, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with("VRFKeyHashAlreadyRegistered (KeyHash {unKeyHash = \"5555"),
            "got: {s}"
        );
        assert!(
            s.contains("VRFVerKeyHash {unVRFVerKeyHash = \"6666"),
            "got: {s}"
        );
    }

    #[test]
    fn shelley_pool_pred_failure_retirement_wrong_epoch_decodes_tag1() {
        // outer [0x84, 0x01, gt_expected=5, supplied=3, lt_expected=6]
        // — flattened pair of Mismatches sharing the `supplied`
        // field.
        let cbor = [0x84_u8, 0x01, 0x18, 0x05, 0x18, 0x03, 0x18, 0x06];
        let f =
            ShelleyPoolPredFailure::from_cbor(&cbor).expect("StakePoolRetirementWrongEpochPOOL");
        if let ShelleyPoolPredFailure::StakePoolRetirementWrongEpochPOOL {
            supplied,
            gt_expected,
            lt_expected,
        } = &f
        {
            assert_eq!(*supplied, 3);
            assert_eq!(*gt_expected, 5);
            assert_eq!(*lt_expected, 6);
        } else {
            panic!("expected typed tag-1, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "StakePoolRetirementWrongEpochPOOL (Mismatch (RelGT) {supplied: 3, expected: 5}) (Mismatch (RelLTEQ) {supplied: 3, expected: 6})"
        );
    }

    #[test]
    fn shelley_pool_pred_failure_unknown_tag_rejects() {
        let cbor = vec![0x82_u8, 0x18, 77, 0x40];
        let err = ShelleyPoolPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 77"),
            "got: {err}"
        );
    }

    #[test]
    fn shelley_delpl_pred_failure_deleg_failure_decodes_tag1() {
        // DELPL tag 1 with inner DELEG tag 4 (WrongCertificateTypeDELEG,
        // no payload). Outer [0x82, 0x01, [0x81, 0x04]].
        let cbor = [0x82_u8, 0x01, 0x81, 0x04];
        let f = ShelleyDelplPredFailure::from_cbor(&cbor).expect("DelegFailure");
        if let ShelleyDelplPredFailure::DelegFailure(deleg) = &f {
            assert_eq!(deleg.tag(), 4);
            assert!(matches!(
                deleg,
                ShelleyDelegPredFailure::WrongCertificateTypeDELEG
            ));
        } else {
            panic!("expected typed DelegFailure, got {f:?}");
        }
        assert_eq!(f.tag(), 1);
        assert_eq!(f.constructor(), "DelegFailure");
        assert_eq!(f.to_string(), "DelegFailure (WrongCertificateTypeDELEG)");
    }

    #[test]
    fn shelley_deleg_pred_failure_no_payload_variants() {
        for (cbor_tag, expected_name) in [
            (4_u8, "WrongCertificateTypeDELEG"),
            (11_u8, "MIRTransferNotCurrentlyAllowed"),
            (12_u8, "MIRNegativesNotCurrentlyAllowed"),
            (14_u8, "MIRProducesNegativeUpdate"),
        ] {
            let cbor = [0x81_u8, cbor_tag];
            let f = ShelleyDelegPredFailure::from_cbor(&cbor).expect("DELEG no-payload");
            assert_eq!(f.tag(), cbor_tag);
            assert_eq!(f.constructor(), expected_name);
            assert_eq!(f.to_string(), expected_name);
        }
    }

    #[test]
    fn shelley_deleg_pred_failure_coin_decodes_tag2() {
        // outer [0x82, 0x02, coin=12345]
        let cbor = [0x82_u8, 0x02, 0x19, 0x30, 0x39];
        let f =
            ShelleyDelegPredFailure::from_cbor(&cbor).expect("StakeKeyNonZeroAccountBalanceDELEG");
        if let ShelleyDelegPredFailure::StakeKeyNonZeroAccountBalanceDELEG(coin) = &f {
            assert_eq!(*coin, 12345);
        } else {
            panic!("expected StakeKeyNonZeroAccountBalanceDELEG, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "StakeKeyNonZeroAccountBalanceDELEG (Coin 12345)"
        );
    }

    #[test]
    fn shelley_deleg_pred_failure_keyhash_decodes_tag5() {
        // outer [0x82, 0x05, bytes(28)] for GenesisKeyNotInMappingDELEG
        let mut cbor = vec![0x82_u8, 0x05];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x99_u8; 28]);
        let f = ShelleyDelegPredFailure::from_cbor(&cbor).expect("GenesisKeyNotInMappingDELEG");
        if let ShelleyDelegPredFailure::GenesisKeyNotInMappingDELEG(kh) = &f {
            assert_eq!(kh.0, [0x99_u8; 28]);
        } else {
            panic!("expected GenesisKeyNotInMappingDELEG, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("GenesisKeyNotInMappingDELEG (KeyHash {unKeyHash = \"9999"),
            "got: {f}"
        );
    }

    #[test]
    fn shelley_deleg_pred_failure_vrf_decodes_tag9() {
        // outer [0x82, 0x09, bytes(32)] for DuplicateGenesisVRFDELEG
        let mut cbor = vec![0x82_u8, 0x09];
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0x55_u8; 32]);
        let f = ShelleyDelegPredFailure::from_cbor(&cbor).expect("DuplicateGenesisVRFDELEG");
        if let ShelleyDelegPredFailure::DuplicateGenesisVRFDELEG(vrf) = &f {
            assert_eq!(vrf.0, [0x55_u8; 32]);
        } else {
            panic!("expected DuplicateGenesisVRFDELEG, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("DuplicateGenesisVRFDELEG (VRFVerKeyHash {unVRFVerKeyHash = \"5555"),
            "got: {f}"
        );
    }

    #[test]
    fn shelley_deleg_pred_failure_stake_key_already_registered_decodes_tag0() {
        // outer [0x82, 0x00, credential[2-array: 0, bytes(28)]] for KeyHashObj
        let mut cbor = vec![0x82_u8, 0x00, 0x82, 0x00];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x88_u8; 28]);
        let f = ShelleyDelegPredFailure::from_cbor(&cbor).expect("StakeKeyAlreadyRegisteredDELEG");
        if let ShelleyDelegPredFailure::StakeKeyAlreadyRegisteredDELEG(cred) = &f {
            assert!(matches!(cred, Credential::KeyHashObj(kh) if kh.0 == [0x88_u8; 28]));
        } else {
            panic!("expected typed tag-0, got {f:?}");
        }
        assert!(
            f.to_string().starts_with(
                "StakeKeyAlreadyRegisteredDELEG (KeyHashObj (KeyHash {unKeyHash = \"8888"
            ),
            "got: {f}"
        );
    }

    #[test]
    fn shelley_deleg_pred_failure_stake_key_not_registered_decodes_tag1_scripthash() {
        // outer [0x82, 0x01, credential[2-array: 1, bytes(28)]] for ScriptHashObj
        let mut cbor = vec![0x82_u8, 0x01, 0x82, 0x01];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0xAA_u8; 28]);
        let f = ShelleyDelegPredFailure::from_cbor(&cbor).expect("StakeKeyNotRegisteredDELEG");
        if let ShelleyDelegPredFailure::StakeKeyNotRegisteredDELEG(cred) = &f {
            assert!(matches!(cred, Credential::ScriptHashObj(sh) if sh.0 == [0xAA_u8; 28]));
        } else {
            panic!("expected typed tag-1, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("StakeKeyNotRegisteredDELEG (ScriptHashObj (ScriptHash \"aaaa"),
            "got: {f}"
        );
    }

    #[test]
    fn shelley_deleg_pred_failure_stake_delegation_impossible_decodes_tag3() {
        // outer [0x82, 0x03, credential[2-array: 0, bytes(28)]]
        let mut cbor = vec![0x82_u8, 0x03, 0x82, 0x00];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x55_u8; 28]);
        let f = ShelleyDelegPredFailure::from_cbor(&cbor).expect("StakeDelegationImpossibleDELEG");
        if let ShelleyDelegPredFailure::StakeDelegationImpossibleDELEG(cred) = &f {
            assert!(matches!(cred, Credential::KeyHashObj(_)));
        } else {
            panic!("expected typed tag-3, got {f:?}");
        }
    }

    #[test]
    fn credential_from_decoder_rejects_unknown_tag() {
        use yggdrasil_ledger::Decoder;
        let mut cbor = vec![0x82_u8, 0x05];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x00_u8; 28]);
        let mut dec = Decoder::new(&cbor);
        let err = Credential::from_decoder(&mut dec).expect_err("unknown tag must reject");
        assert!(err.to_string().contains("unknown tag 5"), "got: {err}");
    }

    #[test]
    fn shelley_deleg_pred_failure_unknown_tag_rejects() {
        // Tag 10 was deliberately skipped by upstream, so it must
        // be rejected.
        let cbor = vec![0x82_u8, 0x0A, 0x40];
        let err = ShelleyDelegPredFailure::from_cbor(&cbor).expect_err("tag 10 must reject");
        assert!(
            err.to_string().contains("unknown variant tag 10"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_ledger_pred_failure_utxow_typed_routing_tag1() {
        // R624 wires LEDGER tag 1 to typed UTXOW. Inner payload
        // here is UTXOW tag 8 (InvalidMetadata — no payload).
        // Outer LEDGER [0x82, 0x01, [0x81, 0x08]].
        let cbor = [0x82_u8, 0x01, 0x81, 0x08];
        let f = ConwayLedgerPredFailure::from_cbor(&cbor).expect("ConwayUtxowFailure");
        if let ConwayLedgerPredFailure::ConwayUtxowFailure(utxow) = &f {
            assert_eq!(utxow.tag(), 8);
            assert!(matches!(utxow, ConwayUtxowPredFailure::InvalidMetadata));
        } else {
            panic!("expected ConwayUtxowFailure(_), got {f:?}");
        }
        assert_eq!(f.tag(), 1);
        assert_eq!(f.constructor(), "ConwayUtxowFailure");
        assert_eq!(f.to_string(), "ConwayUtxowFailure (InvalidMetadata)");
    }

    #[test]
    fn conway_utxow_pred_failure_invalid_metadata_decodes_tag8() {
        let cbor = [0x81_u8, 0x08];
        let f = ConwayUtxowPredFailure::from_cbor(&cbor).expect("InvalidMetadata");
        assert!(matches!(f, ConwayUtxowPredFailure::InvalidMetadata));
        assert_eq!(f.tag(), 8);
        assert_eq!(f.constructor(), "InvalidMetadata");
        assert_eq!(f.to_string(), "InvalidMetadata");
    }

    #[test]
    fn conway_utxow_pred_failure_missing_tx_body_metadata_hash_decodes_tag5() {
        // outer [0x82, 0x05, bytes(32)] (TxAuxDataHash)
        let mut cbor = vec![0x82_u8, 0x05];
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0xCC_u8; 32]);
        let f = ConwayUtxowPredFailure::from_cbor(&cbor).expect("MissingTxBodyMetadataHash");
        if let ConwayUtxowPredFailure::MissingTxBodyMetadataHash(hash) = &f {
            assert_eq!(hash.0, [0xCC_u8; 32]);
        } else {
            panic!("expected MissingTxBodyMetadataHash, got {f:?}");
        }
        assert!(
            f.to_string().starts_with(
                "MissingTxBodyMetadataHash (TxAuxDataHash {unTxAuxDataHash = SafeHash"
            ),
            "got: {f}"
        );
    }

    #[test]
    fn conway_utxow_pred_failure_conflicting_metadata_hash_decodes_tag7() {
        // Tag 7 ToGroup-flattened: [0x83, 0x07, supplied_hash,
        // expected_hash]
        let mut cbor = vec![0x83_u8, 0x07];
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0xAA_u8; 32]);
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0xBB_u8; 32]);
        let f = ConwayUtxowPredFailure::from_cbor(&cbor).expect("ConflictingMetadataHash");
        if let ConwayUtxowPredFailure::ConflictingMetadataHash(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelEQ);
            assert_eq!(mm.supplied.0, [0xAA_u8; 32]);
            assert_eq!(mm.expected.0, [0xBB_u8; 32]);
        } else {
            panic!("expected ConflictingMetadataHash, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("ConflictingMetadataHash (Mismatch (RelEQ)"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_utxow_pred_failure_missing_script_witnesses_decodes_tag3() {
        // outer [0x82, 0x03, tag-258 array(1) bytes(28)]
        let mut cbor = vec![0x82_u8, 0x03, 0xD9, 0x01, 0x02, 0x81];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0xEE_u8; 28]);
        let f = ConwayUtxowPredFailure::from_cbor(&cbor).expect("MissingScriptWitnessesUTXOW");
        if let ConwayUtxowPredFailure::MissingScriptWitnessesUTXOW(set) = &f {
            assert_eq!(set.entries.len(), 1);
        } else {
            panic!("expected MissingScriptWitnessesUTXOW, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("MissingScriptWitnessesUTXOW (NonEmptySet (fromList [ScriptHash"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_utxow_pred_failure_routes_pending_to_raw_tag10() {
        // tag 10 (MissingRedeemers) — payload pending PlutusPurpose
        let cbor = [0x82_u8, 0x0A, 0x80];
        let f = ConwayUtxowPredFailure::from_cbor(&cbor).expect("MissingRedeemers");
        assert_eq!(f.tag(), 10);
        assert_eq!(f.constructor(), "MissingRedeemers");
        assert!(
            f.to_string().starts_with("MissingRedeemers <raw-cbor"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_utxow_pred_failure_missing_required_datums_tag11() {
        // outer [0x83, 0x0B, NonEmptySet [1 hash], Set [1 hash]]
        let mut cbor = vec![0x83_u8, 0x0B];
        // NonEmptySet: tag-258 array(1) of bytes(32)
        cbor.extend_from_slice(&[0xD9, 0x01, 0x02, 0x81]);
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0x11_u8; 32]);
        // Set: bare array(1) of bytes(32)
        cbor.push(0x81);
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0x22_u8; 32]);
        let f = ConwayUtxowPredFailure::from_cbor(&cbor).expect("MissingRequiredDatums");
        if let ConwayUtxowPredFailure::MissingRequiredDatums { missing, received } = &f {
            assert_eq!(missing.entries.len(), 1);
            assert_eq!(received.entries.len(), 1);
        } else {
            panic!("expected MissingRequiredDatums, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with("MissingRequiredDatums (NonEmptySet (fromList [SafeHash \"1111"),
            "got: {s}"
        );
        assert!(s.contains(") (fromList [SafeHash \"2222"), "got: {s}");
    }

    #[test]
    fn conway_utxow_pred_failure_not_allowed_supplemental_datums_tag12_empty_set() {
        // outer [0x83, 0x0C, NonEmptySet [1 hash], Set []]
        let mut cbor = vec![0x83_u8, 0x0C];
        cbor.extend_from_slice(&[0xD9, 0x01, 0x02, 0x81]);
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0x33_u8; 32]);
        cbor.push(0x80); // empty Set
        let f = ConwayUtxowPredFailure::from_cbor(&cbor).expect("NotAllowedSupplementalDatums");
        if let ConwayUtxowPredFailure::NotAllowedSupplementalDatums {
            unallowed,
            acceptable,
        } = &f
        {
            assert_eq!(unallowed.entries.len(), 1);
            assert!(acceptable.entries.is_empty());
        } else {
            panic!("expected NotAllowedSupplementalDatums, got {f:?}");
        }
        assert!(f.to_string().ends_with(") (fromList [])"), "got: {f}");
    }

    #[test]
    fn conway_utxow_pred_failure_missing_required_datums_rejects_empty_nonempty_set() {
        // NonEmptySet must reject an empty array.
        let cbor = [0x83_u8, 0x0B, 0x80, 0x80];
        let err =
            ConwayUtxowPredFailure::from_cbor(&cbor).expect_err("empty NonEmptySet must reject");
        assert!(
            err.to_string()
                .contains("NonEmptySet requires at least one entry"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_certs_pred_failure_withdrawals_not_in_rewards_tag0() {
        // outer [0x82, 0x00, empty-map 0xa0]
        let cbor = [0x82_u8, 0x00, 0xa0];
        let f = ConwayCertsPredFailure::from_cbor(&cbor).expect("WithdrawalsNotInRewardsCERTS");
        if let ConwayCertsPredFailure::WithdrawalsNotInRewardsCERTS(w) = &f {
            assert!(w.entries.is_empty());
        } else {
            panic!("expected WithdrawalsNotInRewardsCERTS, got {f:?}");
        }
        assert_eq!(f.tag(), 0);
        assert_eq!(f.constructor(), "WithdrawalsNotInRewardsCERTS");
        assert_eq!(
            f.to_string(),
            "WithdrawalsNotInRewardsCERTS (Withdrawals {unWithdrawals = fromList []})"
        );
    }

    #[test]
    fn conway_certs_pred_failure_cert_failure_tag1() {
        // CERTS → CERT → DELEG chain. Outer CERTS [0x82, 0x01,
        // inner-CERT]; inner-CERT [0x82, 0x01, inner-DELEG];
        // inner-DELEG [0x82, 0x01, coin=100] for
        // IncorrectDepositDELEG.
        let cbor = [0x82_u8, 0x01, 0x82, 0x01, 0x82, 0x01, 0x18, 100];
        let f = ConwayCertsPredFailure::from_cbor(&cbor).expect("CertFailure");
        if let ConwayCertsPredFailure::CertFailure(cert) = &f {
            assert_eq!(cert.tag(), 1);
            if let ConwayCertPredFailure::DelegFailure(deleg) = cert {
                assert_eq!(deleg.tag(), 1);
                assert!(matches!(
                    deleg,
                    ConwayDelegPredFailure::IncorrectDepositDELEG(100)
                ));
            } else {
                panic!("expected DelegFailure inside CERT, got {cert:?}");
            }
        } else {
            panic!("expected typed CertFailure, got {f:?}");
        }
        assert_eq!(f.tag(), 1);
        assert_eq!(f.constructor(), "CertFailure");
        assert_eq!(
            f.to_string(),
            "CertFailure (DelegFailure (IncorrectDepositDELEG (Coin 100)))"
        );
    }

    #[test]
    fn conway_ledger_pred_failure_certs_typed_routing_tag2() {
        // Outer LEDGER [0x82, 0x02, inner-CERTS]; inner-CERTS
        // [0x82, 0x00, empty-map 0xa0] = WithdrawalsNotInRewardsCERTS
        let cbor = [0x82_u8, 0x02, 0x82, 0x00, 0xa0];
        let f = ConwayLedgerPredFailure::from_cbor(&cbor).expect("ConwayCertsFailure");
        if let ConwayLedgerPredFailure::ConwayCertsFailure(certs) = &f {
            assert_eq!(certs.tag(), 0);
        } else {
            panic!("expected ConwayCertsFailure(_), got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "ConwayCertsFailure (WithdrawalsNotInRewardsCERTS (Withdrawals {unWithdrawals = fromList []}))"
        );
    }

    #[test]
    fn conway_gov_pred_failure_proposal_deposit_incorrect_tag4() {
        // outer [0x83, 0x04, supplied=500, expected=1000] (ToGroup
        // flattened Mismatch RelEQ Coin)
        let cbor = [0x83_u8, 0x04, 0x19, 0x01, 0xF4, 0x19, 0x03, 0xE8];
        let f = ConwayGovPredFailure::from_cbor(&cbor).expect("ProposalDepositIncorrect");
        if let ConwayGovPredFailure::ProposalDepositIncorrect(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelEQ);
            assert_eq!(mm.supplied, 500);
            assert_eq!(mm.expected, 1000);
        } else {
            panic!("expected ProposalDepositIncorrect, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "ProposalDepositIncorrect (Mismatch (RelEQ) {supplied: Coin 500, expected: Coin 1000})"
        );
    }

    #[test]
    fn conway_gov_pred_failure_routes_pending_to_raw_tag0() {
        // tag 0 (GovActionsDoNotExist) — pending GovActionId
        let cbor = [0x82_u8, 0x00, 0x80];
        let f = ConwayGovPredFailure::from_cbor(&cbor).expect("GovActionsDoNotExist");
        assert_eq!(f.tag(), 0);
        assert_eq!(f.constructor(), "GovActionsDoNotExist");
        assert!(
            f.to_string().starts_with("GovActionsDoNotExist <raw-cbor"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_ledger_pred_failure_gov_typed_routing_tag3() {
        // Outer LEDGER [0x82, 0x03, inner-GOV]; inner-GOV
        // [0x83, 0x04, supplied=100, expected=200] (Mismatch)
        let cbor = [0x82_u8, 0x03, 0x83, 0x04, 0x18, 100, 0x18, 200];
        let f = ConwayLedgerPredFailure::from_cbor(&cbor).expect("ConwayGovFailure");
        if let ConwayLedgerPredFailure::ConwayGovFailure(gov) = &f {
            assert_eq!(gov.tag(), 4);
        } else {
            panic!("expected ConwayGovFailure(_), got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "ConwayGovFailure (ProposalDepositIncorrect (Mismatch (RelEQ) {supplied: Coin 100, expected: Coin 200}))"
        );
    }

    #[test]
    fn conway_gov_pred_failure_unknown_tag_rejects() {
        let cbor = vec![0x82_u8, 0x18, 99, 0x40];
        let err = ConwayGovPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 99"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_cert_pred_failure_pool_failure_decodes_tag2() {
        // outer [0x82, 0x02, inner-POOL]; inner-POOL = [0x82,
        // 0x00, bytes(28)] for tag-0 StakePoolNotRegisteredOnKey
        let mut cbor = vec![0x82_u8, 0x02, 0x82, 0x00];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x12_u8; 28]);
        let f = ConwayCertPredFailure::from_cbor(&cbor).expect("PoolFailure");
        if let ConwayCertPredFailure::PoolFailure(pool) = &f {
            assert_eq!(pool.tag(), 0);
        } else {
            panic!("expected typed PoolFailure, got {f:?}");
        }
        assert_eq!(f.tag(), 2);
        assert_eq!(f.constructor(), "PoolFailure");
        assert!(
            f.to_string()
                .starts_with("PoolFailure (StakePoolNotRegisteredOnKeyPOOL (KeyHash"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_cert_pred_failure_deleg_failure_typed_routing_tag1() {
        // CERT tag 1 with inner DELEG tag 1 (IncorrectDepositDELEG
        // coin=50). Outer [0x82, 0x01, [0x82, 0x01, 50]].
        let cbor = [0x82_u8, 0x01, 0x82, 0x01, 0x18, 50];
        let f = ConwayCertPredFailure::from_cbor(&cbor).expect("DelegFailure");
        if let ConwayCertPredFailure::DelegFailure(deleg) = &f {
            assert_eq!(deleg.tag(), 1);
            assert!(matches!(
                deleg,
                ConwayDelegPredFailure::IncorrectDepositDELEG(50)
            ));
        } else {
            panic!("expected typed DelegFailure, got {f:?}");
        }
        assert_eq!(f.tag(), 1);
        assert_eq!(f.constructor(), "DelegFailure");
        assert_eq!(
            f.to_string(),
            "DelegFailure (IncorrectDepositDELEG (Coin 50))"
        );
    }

    #[test]
    fn conway_deleg_pred_failure_incorrect_deposit_tag1() {
        let cbor = [0x82_u8, 0x01, 0x18, 200];
        let f = ConwayDelegPredFailure::from_cbor(&cbor).expect("IncorrectDepositDELEG");
        assert!(matches!(
            f,
            ConwayDelegPredFailure::IncorrectDepositDELEG(200)
        ));
        assert_eq!(f.tag(), 1);
        assert_eq!(f.to_string(), "IncorrectDepositDELEG (Coin 200)");
    }

    #[test]
    fn conway_deleg_pred_failure_stake_key_registered_tag2() {
        // outer [0x82, 0x02, credential[2-array: 0, bytes(28)]]
        let mut cbor = vec![0x82_u8, 0x02, 0x82, 0x00];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0xAB_u8; 28]);
        let f = ConwayDelegPredFailure::from_cbor(&cbor).expect("StakeKeyRegisteredDELEG");
        if let ConwayDelegPredFailure::StakeKeyRegisteredDELEG(cred) = &f {
            assert!(matches!(cred, Credential::KeyHashObj(_)));
        } else {
            panic!("expected StakeKeyRegisteredDELEG, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("StakeKeyRegisteredDELEG (KeyHashObj (KeyHash"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_deleg_pred_failure_delegatee_stake_pool_not_registered_tag6() {
        let mut cbor = vec![0x82_u8, 0x06];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x77_u8; 28]);
        let f =
            ConwayDelegPredFailure::from_cbor(&cbor).expect("DelegateeStakePoolNotRegisteredDELEG");
        if let ConwayDelegPredFailure::DelegateeStakePoolNotRegisteredDELEG(kh) = &f {
            assert_eq!(kh.0, [0x77_u8; 28]);
        } else {
            panic!("expected DelegateeStakePoolNotRegisteredDELEG, got {f:?}");
        }
    }

    #[test]
    fn conway_deleg_pred_failure_deposit_incorrect_tag7() {
        // outer [0x82, 0x07, mismatch [supplied=10, expected=20]]
        let cbor = [0x82_u8, 0x07, 0x82, 0x0a, 0x14];
        let f = ConwayDelegPredFailure::from_cbor(&cbor).expect("DepositIncorrectDELEG");
        if let ConwayDelegPredFailure::DepositIncorrectDELEG(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelEQ);
            assert_eq!(mm.supplied, 10);
            assert_eq!(mm.expected, 20);
        } else {
            panic!("expected DepositIncorrectDELEG, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "DepositIncorrectDELEG (Mismatch (RelEQ) {supplied: Coin 10, expected: Coin 20})"
        );
    }

    #[test]
    fn conway_deleg_pred_failure_unknown_tag_rejects() {
        // Tag 0 not used by upstream (DELEG tags start at 1).
        let cbor = vec![0x82_u8, 0x00, 0x40];
        let err = ConwayDelegPredFailure::from_cbor(&cbor).expect_err("tag 0 must reject");
        assert!(
            err.to_string().contains("unknown variant tag 0"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_gov_cert_pred_failure_drep_already_registered_tag0() {
        // outer [0x82, 0x00, credential[2-array: 0, bytes(28)]]
        let mut cbor = vec![0x82_u8, 0x00, 0x82, 0x00];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x33_u8; 28]);
        let f = ConwayGovCertPredFailure::from_cbor(&cbor).expect("ConwayDRepAlreadyRegistered");
        if let ConwayGovCertPredFailure::ConwayDRepAlreadyRegistered(cred) = &f {
            assert!(matches!(cred, Credential::KeyHashObj(_)));
        } else {
            panic!("expected ConwayDRepAlreadyRegistered, got {f:?}");
        }
        assert_eq!(f.tag(), 0);
        assert!(
            f.to_string()
                .starts_with("ConwayDRepAlreadyRegistered (KeyHashObj (KeyHash"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_gov_cert_pred_failure_drep_incorrect_deposit_tag2() {
        // outer [0x83, 0x02, supplied=10, expected=20] (ToGroup-flattened)
        let cbor = [0x83_u8, 0x02, 0x0a, 0x14];
        let f = ConwayGovCertPredFailure::from_cbor(&cbor).expect("ConwayDRepIncorrectDeposit");
        if let ConwayGovCertPredFailure::ConwayDRepIncorrectDeposit(mm) = &f {
            assert_eq!(mm.supplied, 10);
            assert_eq!(mm.expected, 20);
        } else {
            panic!("expected ConwayDRepIncorrectDeposit, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "ConwayDRepIncorrectDeposit (Mismatch (RelEQ) {supplied: Coin 10, expected: Coin 20})"
        );
    }

    #[test]
    fn conway_gov_cert_pred_failure_committee_resigned_tag3() {
        let mut cbor = vec![0x82_u8, 0x03, 0x82, 0x01];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x44_u8; 28]);
        let f = ConwayGovCertPredFailure::from_cbor(&cbor)
            .expect("ConwayCommitteeHasPreviouslyResigned");
        if let ConwayGovCertPredFailure::ConwayCommitteeHasPreviouslyResigned(cred) = &f {
            assert!(matches!(cred, Credential::ScriptHashObj(_)));
        } else {
            panic!("expected ConwayCommitteeHasPreviouslyResigned, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("ConwayCommitteeHasPreviouslyResigned (ScriptHashObj (ScriptHash"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_cert_pred_failure_gov_cert_failure_typed_routing_tag3() {
        // CERT tag 3 with inner GOVCERT tag 0
        // (ConwayDRepAlreadyRegistered KeyHashObj). Outer:
        // [0x82, 0x03, [0x82, 0x00, [0x82, 0x00, bytes(28)]]]
        let mut cbor = vec![0x82_u8, 0x03, 0x82, 0x00, 0x82, 0x00];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x99_u8; 28]);
        let f = ConwayCertPredFailure::from_cbor(&cbor).expect("GovCertFailure");
        if let ConwayCertPredFailure::GovCertFailure(govcert) = &f {
            assert_eq!(govcert.tag(), 0);
        } else {
            panic!("expected typed GovCertFailure, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("GovCertFailure (ConwayDRepAlreadyRegistered (KeyHashObj"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_gov_cert_pred_failure_unknown_tag_rejects() {
        let cbor = vec![0x82_u8, 0x18, 99, 0x40];
        let err = ConwayGovCertPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 99"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_cert_pred_failure_unknown_tag_rejects() {
        // Tag 0 not used by upstream (CERT tags start at 1) — must
        // reject.
        let cbor = vec![0x82_u8, 0x00, 0x40];
        let err = ConwayCertPredFailure::from_cbor(&cbor).expect_err("tag 0 must reject");
        assert!(
            err.to_string().contains("unknown variant tag 0"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_certs_pred_failure_unknown_tag_rejects() {
        let cbor = vec![0x82_u8, 0x18, 99, 0x40];
        let err = ConwayCertsPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 99"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_utxow_pred_failure_unknown_tag_rejects() {
        let cbor = vec![0x82_u8, 0x18, 99, 0x40];
        let err = ConwayUtxowPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 99"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_utxow_pred_failure_script_integrity_hash_mismatch_tag18() {
        // outer [0x83, 0x12, Mismatch-2array, StrictMaybe-bytes].
        // Mismatch [supplied=SJust hash, expected=SNothing].
        // provided = SJust bytes(3).
        let mut cbor = vec![0x83_u8, 0x12, 0x82, 0x81]; // Mismatch 2-arr, supplied SJust
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0xCD_u8; 32]);
        cbor.push(0x80); // expected SNothing
        cbor.push(0x81); // provided SJust list(1)
        cbor.push(0x43); // bytes(3)
        cbor.extend_from_slice(b"sih");
        let f = ConwayUtxowPredFailure::from_cbor(&cbor).expect("ScriptIntegrityHashMismatch");
        if let ConwayUtxowPredFailure::ScriptIntegrityHashMismatch { mismatch, provided } = &f {
            assert_eq!(mismatch.supplied.0, Some([0xCD_u8; 32]));
            assert_eq!(mismatch.expected.0, None);
            assert_eq!(provided.0.as_deref(), Some(b"sih".as_slice()));
        } else {
            panic!("expected ScriptIntegrityHashMismatch, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with(
                "ScriptIntegrityHashMismatch (Mismatch (RelEQ) {supplied: SJust (SafeHash \"cdcd"
            ),
            "got: {s}"
        );
        assert!(
            s.ends_with("expected: SNothing}) (SJust <bytestring 3 bytes>)"),
            "got: {s}"
        );
    }

    #[test]
    fn conway_utxow_pred_failure_pp_view_hashes_dont_match_tag13() {
        // outer [0x83, 0x0D, supplied SMaybe, expected SMaybe]
        // (ToGroup flattened). supplied = SJust hash(32 of 0xAB),
        // expected = SNothing.
        let mut cbor = vec![0x83_u8, 0x0D, 0x81]; // SJust list(1)
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0xAB_u8; 32]);
        cbor.push(0x80); // SNothing list(0)
        let f = ConwayUtxowPredFailure::from_cbor(&cbor).expect("PPViewHashesDontMatch");
        if let ConwayUtxowPredFailure::PPViewHashesDontMatch(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelEQ);
            assert_eq!(mm.supplied.0, Some([0xAB_u8; 32]));
            assert_eq!(mm.expected.0, None);
        } else {
            panic!("expected PPViewHashesDontMatch, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with(
                "PPViewHashesDontMatch (Mismatch (RelEQ) {supplied: SJust (SafeHash \"abab"
            ),
            "got: {s}"
        );
        assert!(s.ends_with("expected: SNothing})"), "got: {s}");
    }

    #[test]
    fn conway_utxow_pred_failure_extra_redeemers_tag15() {
        // outer [0x82, 0x0F, NonEmpty [PlutusPurpose AsIx]].
        // NonEmpty array(2): [ConwaySpending(AsIx 3),
        // ConwayMinting(AsIx 7)]. Each purpose is a 2-element
        // group [tag, index].
        let cbor = [0x82_u8, 0x0F, 0x82, 0x82, 0x00, 0x03, 0x82, 0x01, 0x07];
        let f = ConwayUtxowPredFailure::from_cbor(&cbor).expect("ExtraRedeemers");
        if let ConwayUtxowPredFailure::ExtraRedeemers(purposes) = &f {
            assert_eq!(purposes.entries.len(), 2);
            assert_eq!(
                purposes.entries[0],
                ConwayPlutusPurposeIx::ConwaySpending(3)
            );
            assert_eq!(purposes.entries[1], ConwayPlutusPurposeIx::ConwayMinting(7));
        } else {
            panic!("expected ExtraRedeemers, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "ExtraRedeemers (ConwaySpending (AsIx {unAsIx = 3}) :| [ConwayMinting (AsIx {unAsIx = 7})])"
        );
    }

    #[test]
    fn conway_utxow_pred_failure_extra_redeemers_rejects_empty() {
        let cbor = [0x82_u8, 0x0F, 0x80];
        let err = ConwayUtxowPredFailure::from_cbor(&cbor).expect_err("empty NonEmpty must reject");
        assert!(
            err.to_string()
                .contains("NonEmpty requires at least one entry"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_plutus_purpose_ix_covers_all_six_purposes() {
        for (tag, expected) in [
            (0_u8, "ConwaySpending"),
            (1, "ConwayMinting"),
            (2, "ConwayCertifying"),
            (3, "ConwayRewarding"),
            (4, "ConwayVoting"),
            (5, "ConwayProposing"),
        ] {
            // Wrap a single purpose in a NonEmpty for decoding.
            let cbor = [0x82_u8, 0x0F, 0x81, 0x82, tag, 0x09];
            let f =
                ConwayUtxowPredFailure::from_cbor(&cbor).expect("ExtraRedeemers single purpose");
            let ConwayUtxowPredFailure::ExtraRedeemers(purposes) = &f else {
                panic!("expected ExtraRedeemers, got {f:?}");
            };
            assert_eq!(purposes.entries[0].constructor(), expected);
        }
    }

    #[test]
    fn conway_utxo_pred_failure_input_set_empty_tag4() {
        let cbor = [0x81_u8, 0x04];
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("InputSetEmptyUTxO");
        assert!(matches!(f, ConwayUtxoPredFailure::InputSetEmptyUTxO));
        assert_eq!(f.tag(), 4);
        assert_eq!(f.to_string(), "InputSetEmptyUTxO");
    }

    #[test]
    fn conway_utxo_pred_failure_no_collateral_inputs_tag19() {
        let cbor = [0x81_u8, 0x13];
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("NoCollateralInputs");
        assert!(matches!(f, ConwayUtxoPredFailure::NoCollateralInputs));
        assert_eq!(f.tag(), 19);
        assert_eq!(f.to_string(), "NoCollateralInputs");
    }

    #[test]
    fn conway_utxo_pred_failure_max_tx_size_tag3() {
        // outer [0x83, 0x03, supplied=20000, expected=16384]
        let cbor = [0x83_u8, 0x03, 0x19, 0x4E, 0x20, 0x19, 0x40, 0x00];
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("MaxTxSizeUTxO");
        if let ConwayUtxoPredFailure::MaxTxSizeUTxO(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelLTEQ);
            assert_eq!(mm.supplied, 20000);
            assert_eq!(mm.expected, 16384);
        } else {
            panic!("expected MaxTxSizeUTxO, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "MaxTxSizeUTxO (Mismatch (RelLTEQ) {supplied: 20000, expected: 16384})"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_fee_too_small_tag5() {
        // Tag 5 uses swapMismatch: wire order is
        // expected-then-supplied. outer [0x83, 0x05, expected=170000,
        // supplied=150000]. 170000 = 0x00029810, 150000 = 0x000249F0.
        let cbor = [
            0x83_u8, 0x05, 0x1A, 0x00, 0x02, 0x98, 0x10, 0x1A, 0x00, 0x02, 0x49, 0xF0,
        ];
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("FeeTooSmallUTxO");
        if let ConwayUtxoPredFailure::FeeTooSmallUTxO(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelGTEQ);
            assert_eq!(mm.supplied, 150000);
            assert_eq!(mm.expected, 170000);
        } else {
            panic!("expected FeeTooSmallUTxO, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "FeeTooSmallUTxO (Mismatch (RelGTEQ) {supplied: Coin 150000, expected: Coin 170000})"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_bad_inputs_tag1() {
        // outer [0x82, 0x01, tag-258 array(1) of TxIn [txid32, ix]]
        let mut cbor = vec![0x82_u8, 0x01, 0xD9, 0x01, 0x02, 0x81, 0x82];
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0x11_u8; 32]);
        cbor.push(0x00); // ix = 0
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("BadInputsUTxO");
        if let ConwayUtxoPredFailure::BadInputsUTxO(set) = &f {
            assert_eq!(set.entries.len(), 1);
        } else {
            panic!("expected BadInputsUTxO, got {f:?}");
        }
        assert!(
            f.to_string().starts_with("BadInputsUTxO (NonEmptySet"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_outside_forecast_tag17() {
        let cbor = [0x82_u8, 0x11, 0x1A, 0x00, 0x0F, 0x42, 0x40];
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("OutsideForecast");
        assert!(matches!(
            f,
            ConwayUtxoPredFailure::OutsideForecast(1_000_000)
        ));
        assert_eq!(f.to_string(), "OutsideForecast (SlotNo 1000000)");
    }

    #[test]
    fn conway_utxo_pred_failure_insufficient_collateral_tag12() {
        // outer [0x83, 0x0C, balance=-500 (negative), required=1000]
        // -500 CBOR negative: 0x39 0x01 0xF3 (negative, -(0x01F3+1)
        // = -500).
        let cbor = [0x83_u8, 0x0C, 0x39, 0x01, 0xF3, 0x19, 0x03, 0xE8];
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("InsufficientCollateral");
        if let ConwayUtxoPredFailure::InsufficientCollateral { balance, required } = &f {
            assert_eq!(*balance, -500);
            assert_eq!(*required, 1000);
        } else {
            panic!("expected InsufficientCollateral, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "InsufficientCollateral (DeltaCoin (-500)) (Coin 1000)"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_incorrect_total_collateral_tag20() {
        // outer [0x83, 0x14, provided=750 (positive), declared=800]
        let cbor = [0x83_u8, 0x14, 0x19, 0x02, 0xEE, 0x19, 0x03, 0x20];
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("IncorrectTotalCollateralField");
        if let ConwayUtxoPredFailure::IncorrectTotalCollateralField { provided, declared } = &f {
            assert_eq!(*provided, 750);
            assert_eq!(*declared, 800);
        } else {
            panic!("expected IncorrectTotalCollateralField, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "IncorrectTotalCollateralField (DeltaCoin 750) (Coin 800)"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_outside_validity_interval_tag2() {
        // outer [0x83, 0x02, ValidityInterval, current_slot=5000].
        // ValidityInterval [invalidBefore=SJust 100,
        // invalidHereafter=SJust 200]: [0x82, [0x81, 100],
        // [0x81, 200]].
        let cbor = [
            0x83_u8, 0x02, 0x82, 0x81, 0x18, 100, 0x81, 0x18, 200, 0x19, 0x13, 0x88,
        ];
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("OutsideValidityIntervalUTxO");
        if let ConwayUtxoPredFailure::OutsideValidityIntervalUTxO {
            interval,
            current_slot,
        } = &f
        {
            assert_eq!(interval.invalid_before.0, Some(100));
            assert_eq!(interval.invalid_hereafter.0, Some(200));
            assert_eq!(*current_slot, 5000);
        } else {
            panic!("expected OutsideValidityIntervalUTxO, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "OutsideValidityIntervalUTxO (ValidityInterval {invalidBefore = SJust (SlotNo {unSlotNo = 100}), invalidHereafter = SJust (SlotNo {unSlotNo = 200})}) (SlotNo {unSlotNo = 5000})"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_outside_validity_interval_open_bounds_tag2() {
        // ValidityInterval with SNothing on both bounds:
        // [0x82, [], []].
        let cbor = [0x83_u8, 0x02, 0x82, 0x80, 0x80, 0x00];
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("OutsideValidityIntervalUTxO");
        if let ConwayUtxoPredFailure::OutsideValidityIntervalUTxO { interval, .. } = &f {
            assert_eq!(interval.invalid_before.0, None);
            assert_eq!(interval.invalid_hereafter.0, None);
        } else {
            panic!("expected OutsideValidityIntervalUTxO, got {f:?}");
        }
        assert!(
            f.to_string().contains(
                "ValidityInterval {invalidBefore = SNothing, invalidHereafter = SNothing}"
            ),
            "got: {f}"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_babbage_output_too_small_tag21() {
        // outer [0x82, 0x15, NonEmpty [(TxOut, Coin)]]. NonEmpty
        // array(1) of pair [TxOut, Coin]. TxOut = [bytes(29) addr,
        // coin=500]; pair coin (min value) = 1000000.
        let mut cbor = vec![0x82_u8, 0x15, 0x81, 0x82, 0x82];
        cbor.push(0x58); // addr bytes header
        cbor.push(29);
        cbor.push(0x61); // enterprise/key/Mainnet header
        cbor.extend_from_slice(&[0xCC_u8; 28]);
        cbor.extend_from_slice(&[0x19, 0x01, 0xF4]); // TxOut coin = 500
        cbor.extend_from_slice(&[0x1A, 0x00, 0x0F, 0x42, 0x40]); // min value = 1_000_000
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("BabbageOutputTooSmallUTxO");
        if let ConwayUtxoPredFailure::BabbageOutputTooSmallUTxO(pairs) = &f {
            assert_eq!(pairs.entries.len(), 1);
            assert_eq!(pairs.entries[0].0.coin, 500);
            assert_eq!(pairs.entries[0].1, 1_000_000);
        } else {
            panic!("expected BabbageOutputTooSmallUTxO, got {f:?}");
        }
        let s = f.to_string();
        // `BabbageOutputTooSmallUTxO (` + pair tuple `(` + TxOut
        // tuple `(` = three opening parens.
        assert!(
            s.starts_with("BabbageOutputTooSmallUTxO (((Addr Mainnet"),
            "got: {s}"
        );
        assert!(s.contains(", Coin 500), Coin 1000000)"), "got: {s}");
        assert!(s.ends_with(":| [])"), "got: {s}");
    }

    #[test]
    fn conway_utxo_pred_failure_babbage_output_too_small_rejects_empty() {
        let cbor = [0x82_u8, 0x15, 0x80];
        let err = ConwayUtxoPredFailure::from_cbor(&cbor).expect_err("empty NonEmpty must reject");
        assert!(
            err.to_string()
                .contains("NonEmpty requires at least one entry"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_scripts_not_paid_tag13() {
        // outer [0x82, 0x0D, map(1){TxIn: TxOut}]. TxIn =
        // [txid32, ix]; TxOut = [bytes(29) addr, coin].
        let mut cbor = vec![0x82_u8, 0x0D, 0xA1]; // map(1)
        // key: TxIn [txid32, ix=2]
        cbor.push(0x82);
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0x55_u8; 32]);
        cbor.push(0x02);
        // value: TxOut [bytes(29), coin=900]
        cbor.push(0x82);
        cbor.push(0x58);
        cbor.push(29);
        cbor.push(0x61);
        cbor.extend_from_slice(&[0xEE_u8; 28]);
        cbor.extend_from_slice(&[0x19, 0x03, 0x84]); // coin 900
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("ScriptsNotPaidUTxO");
        if let ConwayUtxoPredFailure::ScriptsNotPaidUTxO(map) = &f {
            assert_eq!(map.entries.len(), 1);
            assert_eq!(map.entries[0].0.tx_ix.0, 2);
            assert_eq!(map.entries[0].1.coin, 900);
        } else {
            panic!("expected ScriptsNotPaidUTxO, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with("ScriptsNotPaidUTxO (NonEmptyMap (fromList [(TxIn (TxId"),
            "got: {s}"
        );
        assert!(s.contains(", Coin 900))])"), "got: {s}");
    }

    #[test]
    fn conway_utxo_pred_failure_scripts_not_paid_rejects_empty() {
        let cbor = [0x82_u8, 0x0D, 0xA0];
        let err =
            ConwayUtxoPredFailure::from_cbor(&cbor).expect_err("empty NonEmptyMap must reject");
        assert!(
            err.to_string()
                .contains("NonEmptyMap requires at least one entry"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_value_not_conserved_tag6() {
        // outer [0x83, 0x06, consumed MaryValue, produced
        // MaryValue]. consumed = bare coin 1000; produced =
        // [coin 900, multiasset {policy: {asset: 5}}].
        let mut cbor = vec![0x83_u8, 0x06];
        cbor.extend_from_slice(&[0x19, 0x03, 0xE8]); // consumed: bare coin 1000
        // produced: 2-array [coin 900, multiasset]
        cbor.push(0x82);
        cbor.extend_from_slice(&[0x19, 0x03, 0x84]); // coin 900
        cbor.push(0xA1); // multiasset map(1)
        cbor.push(0x58); // PolicyID bytes(28)
        cbor.push(28);
        cbor.extend_from_slice(&[0x9A_u8; 28]);
        cbor.push(0xA1); // asset map(1)
        cbor.push(0x44); // AssetName bytes(4)
        cbor.extend_from_slice(b"gold");
        cbor.push(0x05); // amount 5
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("ValueNotConservedUTxO");
        if let ConwayUtxoPredFailure::ValueNotConservedUTxO(mm) = &f {
            assert_eq!(mm.supplied.coin, 1000);
            assert!(mm.supplied.assets.policies.is_empty());
            assert_eq!(mm.expected.coin, 900);
            assert_eq!(mm.expected.assets.policies.len(), 1);
            assert_eq!(mm.expected.assets.policies[0].1[0].1, 5);
        } else {
            panic!("expected ValueNotConservedUTxO, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.starts_with(
                "ValueNotConservedUTxO (Mismatch (RelEQ) {supplied: MaryValue (Coin 1000) (MultiAsset (fromList []))"
            ),
            "got: {s}"
        );
        assert!(s.contains("fromList [(\"676f6c64\",5)]"), "got: {s}");
    }

    #[test]
    fn conway_utxo_pred_failure_collateral_contains_non_ada_tag15() {
        // outer [0x82, 0x0F, MaryValue]. MaryValue = [coin 200,
        // multiasset {policy: {asset: 3}}].
        let mut cbor = vec![0x82_u8, 0x0F, 0x82];
        cbor.extend_from_slice(&[0x18, 0xC8]); // coin 200
        cbor.push(0xA1);
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0x77_u8; 28]);
        cbor.push(0xA1);
        cbor.push(0x40); // AssetName bytes(0) — the empty ADA-name slot
        cbor.push(0x03); // amount 3
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("CollateralContainsNonADA");
        if let ConwayUtxoPredFailure::CollateralContainsNonADA(value) = &f {
            assert_eq!(value.coin, 200);
            assert_eq!(value.assets.policies.len(), 1);
        } else {
            panic!("expected CollateralContainsNonADA, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("CollateralContainsNonADA (MaryValue (Coin 200)"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_babbage_non_disjoint_ref_inputs_tag22() {
        // outer [0x82, 0x16, NonEmpty [TxIn]]. NonEmpty is a bare
        // array(1) of TxIn [txid32, ix].
        let mut cbor = vec![0x82_u8, 0x16, 0x81, 0x82];
        cbor.push(0x58);
        cbor.push(32);
        cbor.extend_from_slice(&[0x44_u8; 32]);
        cbor.push(0x07); // ix = 7
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("BabbageNonDisjointRefInputs");
        if let ConwayUtxoPredFailure::BabbageNonDisjointRefInputs(ins) = &f {
            assert_eq!(ins.entries.len(), 1);
            assert_eq!(ins.entries[0].tx_ix.0, 7);
        } else {
            panic!("expected BabbageNonDisjointRefInputs, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("BabbageNonDisjointRefInputs (TxIn (TxId"),
            "got: {f}"
        );
        assert!(f.to_string().ends_with(":| [])"), "got: {f}");
    }

    #[test]
    fn conway_utxo_pred_failure_babbage_non_disjoint_ref_inputs_rejects_empty() {
        let cbor = [0x82_u8, 0x16, 0x80];
        let err = ConwayUtxoPredFailure::from_cbor(&cbor).expect_err("empty NonEmpty must reject");
        assert!(
            err.to_string()
                .contains("NonEmpty requires at least one entry"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_value_not_conserved_ada_only_tag6() {
        // tag 6 with both MaryValues bare-coin (ADA-only):
        // [0x83, 0x06, 0, 0].
        let cbor = [0x83_u8, 0x06, 0x00, 0x00];
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("ValueNotConservedUTxO");
        assert_eq!(f.tag(), 6);
        if let ConwayUtxoPredFailure::ValueNotConservedUTxO(mm) = &f {
            assert_eq!(mm.supplied.coin, 0);
            assert!(mm.supplied.assets.policies.is_empty());
            assert_eq!(mm.expected.coin, 0);
        } else {
            panic!("expected ValueNotConservedUTxO, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("ValueNotConservedUTxO (Mismatch (RelEQ)"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_ex_units_too_big_tag14() {
        // outer [0x83, 0x0E, expected ExUnits, supplied ExUnits]
        // (swapMismatch expected-first). Each ExUnits is [mem,
        // steps]. expected=[10000, 5000000], supplied=[12000,
        // 6000000].
        let cbor = [
            0x83_u8, 0x0E, // tag 14
            0x82, 0x19, 0x27, 0x10, 0x1A, 0x00, 0x4C, 0x4B, 0x40, // expected
            0x82, 0x19, 0x2E, 0xE0, 0x1A, 0x00, 0x5B, 0x8D, 0x80, // supplied
        ];
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("ExUnitsTooBigUTxO");
        if let ConwayUtxoPredFailure::ExUnitsTooBigUTxO(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelLTEQ);
            assert_eq!(mm.supplied.mem, 12000);
            assert_eq!(mm.supplied.steps, 6_000_000);
            assert_eq!(mm.expected.mem, 10000);
            assert_eq!(mm.expected.steps, 5_000_000);
        } else {
            panic!("expected ExUnitsTooBigUTxO, got {f:?}");
        }
        let s = f.to_string();
        assert!(
            s.contains(
                "supplied: WrapExUnits {unWrapExUnits = ExUnits' {exUnitsMem' = 12000, exUnitsSteps' = 6000000}}"
            ),
            "got: {s}"
        );
        assert!(
            s.starts_with("ExUnitsTooBigUTxO (Mismatch (RelLTEQ)"),
            "got: {s}"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_unknown_tag_rejects() {
        let cbor = vec![0x82_u8, 0x18, 99, 0x40];
        let err = ConwayUtxoPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 99"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_utxos_pred_failure_validation_tag_mismatch_passed_tag0() {
        // outer [0x83, 0x00, isValid=true, [0x81, 0x00]]
        // (TagMismatchDescription PassedUnexpectedly)
        let cbor = [0x83_u8, 0x00, 0xf5, 0x81, 0x00];
        let f = ConwayUtxosPredFailure::from_cbor(&cbor).expect("ValidationTagMismatch");
        if let ConwayUtxosPredFailure::ValidationTagMismatch {
            is_valid,
            description,
        } = &f
        {
            assert!(*is_valid);
            assert!(matches!(
                description,
                TagMismatchDescription::PassedUnexpectedly
            ));
        } else {
            panic!("expected ValidationTagMismatch, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "ValidationTagMismatch (IsValid True) (PassedUnexpectedly)"
        );
    }

    #[test]
    fn conway_utxos_pred_failure_validation_tag_mismatch_failed_tag0() {
        // outer [0x83, 0x00, isValid=false, TagMismatchDescription
        // FailedUnexpectedly [FailureDescription PlutusFailure]].
        // TagMismatchDescription [0x82, 0x01, [array(1) of
        // FailureDescription]]; FailureDescription [0x83, 0x01,
        // text "boom", bytes "ctx"].
        let mut cbor = vec![0x83_u8, 0x00, 0xf4, 0x82, 0x01, 0x81, 0x83, 0x01];
        cbor.push(0x64); // text(4)
        cbor.extend_from_slice(b"boom");
        cbor.push(0x43); // bytes(3)
        cbor.extend_from_slice(b"ctx");
        let f = ConwayUtxosPredFailure::from_cbor(&cbor).expect("ValidationTagMismatch");
        if let ConwayUtxosPredFailure::ValidationTagMismatch {
            is_valid,
            description,
        } = &f
        {
            assert!(!*is_valid);
            if let TagMismatchDescription::FailedUnexpectedly(entries) = description {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].message, "boom");
                assert_eq!(entries[0].context, b"ctx");
            } else {
                panic!("expected FailedUnexpectedly, got {description:?}");
            }
        } else {
            panic!("expected ValidationTagMismatch, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "ValidationTagMismatch (IsValid False) (FailedUnexpectedly (PlutusFailure \"boom\" <bytestring 3 bytes> :| []))"
        );
    }

    #[test]
    fn conway_utxos_pred_failure_collect_errors_raw_tag1() {
        let cbor = [0x82_u8, 0x01, 0x80];
        let f = ConwayUtxosPredFailure::from_cbor(&cbor).expect("CollectErrors");
        assert!(matches!(f, ConwayUtxosPredFailure::CollectErrors(_)));
        assert_eq!(f.tag(), 1);
        assert!(
            f.to_string().starts_with("CollectErrors <raw-cbor"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_utxos_pred_failure_unknown_tag_rejects() {
        let cbor = vec![0x82_u8, 0x18, 99, 0x40];
        let err = ConwayUtxosPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 99"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_utxo_pred_failure_utxos_typed_routing_tag0() {
        // UTXO tag 0 → UTXOS tag 0 ValidationTagMismatch
        // (PassedUnexpectedly). Outer [0x82, 0x00, [0x83, 0x00,
        // true, [0x81, 0x00]]].
        let cbor = [0x82_u8, 0x00, 0x83, 0x00, 0xf5, 0x81, 0x00];
        let f = ConwayUtxoPredFailure::from_cbor(&cbor).expect("UtxosFailure");
        if let ConwayUtxoPredFailure::UtxosFailure(utxos) = &f {
            assert_eq!(utxos.tag(), 0);
        } else {
            panic!("expected typed UtxosFailure, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "UtxosFailure (ValidationTagMismatch (IsValid True) (PassedUnexpectedly))"
        );
    }

    #[test]
    fn conway_utxow_pred_failure_utxo_typed_routing_tag0() {
        // UTXOW tag 0 → UTXO tag 4 (InputSetEmptyUTxO). Outer
        // [0x82, 0x00, [0x81, 0x04]].
        let cbor = [0x82_u8, 0x00, 0x81, 0x04];
        let f = ConwayUtxowPredFailure::from_cbor(&cbor).expect("UtxoFailure");
        if let ConwayUtxowPredFailure::UtxoFailure(utxo) = &f {
            assert_eq!(utxo.tag(), 4);
            assert!(matches!(utxo, ConwayUtxoPredFailure::InputSetEmptyUTxO));
        } else {
            panic!("expected typed UtxoFailure, got {f:?}");
        }
        assert_eq!(f.to_string(), "UtxoFailure (InputSetEmptyUTxO)");
    }

    #[test]
    fn conway_ledger_pred_failure_wdrl_not_delegated_tag4() {
        let mut cbor = vec![0x82_u8, 0x04, 0x81];
        cbor.push(0x58);
        cbor.push(28);
        cbor.extend_from_slice(&[0xAB_u8; 28]);
        let f = ConwayLedgerPredFailure::from_cbor(&cbor).expect("ConwayWdrlNotDelegatedToDRep");
        if let ConwayLedgerPredFailure::ConwayWdrlNotDelegatedToDRep(keys) = &f {
            assert_eq!(keys.entries.len(), 1);
            assert_eq!(keys.entries[0].0, [0xAB_u8; 28]);
        } else {
            panic!("expected ConwayWdrlNotDelegatedToDRep, got {f:?}");
        }
        assert!(
            f.to_string()
                .starts_with("ConwayWdrlNotDelegatedToDRep (KeyHash {unKeyHash = \"abab"),
            "got: {f}"
        );
    }

    #[test]
    fn conway_ledger_pred_failure_treasury_mismatch_tag5() {
        let cbor = [0x83_u8, 0x05, 0x18, 200, 0x18, 100];
        let f = ConwayLedgerPredFailure::from_cbor(&cbor).expect("ConwayTreasuryValueMismatch");
        if let ConwayLedgerPredFailure::ConwayTreasuryValueMismatch(mm) = &f {
            assert_eq!(mm.supplied, 100);
            assert_eq!(mm.expected, 200);
        } else {
            panic!("expected ConwayTreasuryValueMismatch, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "ConwayTreasuryValueMismatch (Mismatch (RelEQ) {supplied: Coin 100, expected: Coin 200})"
        );
    }

    #[test]
    fn conway_ledger_pred_failure_ref_scripts_too_big_tag6() {
        let cbor = [0x83_u8, 0x06, 0x19, 0x02, 0x58, 0x19, 0x01, 0xF4];
        let f = ConwayLedgerPredFailure::from_cbor(&cbor).expect("ConwayTxRefScriptsSizeTooBig");
        if let ConwayLedgerPredFailure::ConwayTxRefScriptsSizeTooBig(mm) = &f {
            assert_eq!(mm.supplied, 600);
            assert_eq!(mm.expected, 500);
        } else {
            panic!("expected ConwayTxRefScriptsSizeTooBig, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "ConwayTxRefScriptsSizeTooBig (Mismatch (RelLTEQ) {supplied: 600, expected: 500})"
        );
    }

    #[test]
    fn conway_ledger_pred_failure_mempool_failure_tag7() {
        let mut cbor = vec![0x82_u8, 0x07];
        cbor.push(0x66);
        cbor.extend_from_slice(b"denied");
        let f = ConwayLedgerPredFailure::from_cbor(&cbor).expect("ConwayMempoolFailure");
        if let ConwayLedgerPredFailure::ConwayMempoolFailure(s) = &f {
            assert_eq!(s, "denied");
        } else {
            panic!("expected ConwayMempoolFailure, got {f:?}");
        }
        assert_eq!(f.to_string(), "ConwayMempoolFailure \"denied\"");
    }

    #[test]
    fn conway_ledger_pred_failure_withdrawals_missing_accounts_tag8() {
        let cbor = [0x82_u8, 0x08, 0xa0];
        let f =
            ConwayLedgerPredFailure::from_cbor(&cbor).expect("ConwayWithdrawalsMissingAccounts");
        if let ConwayLedgerPredFailure::ConwayWithdrawalsMissingAccounts(w) = &f {
            assert!(w.entries.is_empty());
        } else {
            panic!("expected ConwayWithdrawalsMissingAccounts, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "ConwayWithdrawalsMissingAccounts (Withdrawals {unWithdrawals = fromList []})"
        );
    }

    #[test]
    fn conway_ledger_pred_failure_unknown_tag_rejects() {
        let cbor = vec![0x82_u8, 0x18, 99, 0x40];
        let err = ConwayLedgerPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 99"),
            "got: {err}"
        );
    }

    #[test]
    fn conway_ledger_pred_failure_tag0_rejects() {
        // Tag 0 is deliberately not used by upstream (Conway tags
        // start at 1).
        let cbor = vec![0x82_u8, 0x00, 0x40];
        let err = ConwayLedgerPredFailure::from_cbor(&cbor).expect_err("tag 0 must reject");
        assert!(
            err.to_string().contains("unknown variant tag 0"),
            "got: {err}"
        );
    }

    #[test]
    fn shelley_deleg_pred_failure_insufficient_instantaneous_rewards_decodes_tag7() {
        // outer [0x83, 0x07, pot=0 (Reserves), mismatch [supplied=100, expected=200]]
        let cbor = [0x83_u8, 0x07, 0x00, 0x82, 0x18, 100, 0x18, 200];
        let f = ShelleyDelegPredFailure::from_cbor(&cbor)
            .expect("InsufficientForInstantaneousRewardsDELEG");
        if let ShelleyDelegPredFailure::InsufficientForInstantaneousRewardsDELEG { pot, mismatch } =
            &f
        {
            assert_eq!(*pot, MirPot::ReservesMIR);
            assert_eq!(mismatch.relation, MismatchRelation::RelLTEQ);
            assert_eq!(mismatch.supplied, 100);
            assert_eq!(mismatch.expected, 200);
        } else {
            panic!("expected typed tag-7, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "InsufficientForInstantaneousRewardsDELEG ReservesMIR (Mismatch (RelLTEQ) {supplied: Coin 100, expected: Coin 200})"
        );
    }

    #[test]
    fn shelley_deleg_pred_failure_mir_too_late_decodes_tag8() {
        // outer [0x82, 0x08, mismatch [supplied=900, expected=1000]] RelLT
        let cbor = [0x82_u8, 0x08, 0x82, 0x19, 0x03, 0x84, 0x19, 0x03, 0xE8];
        let f =
            ShelleyDelegPredFailure::from_cbor(&cbor).expect("MIRCertificateTooLateinEpochDELEG");
        if let ShelleyDelegPredFailure::MIRCertificateTooLateinEpochDELEG(mm) = &f {
            assert_eq!(mm.relation, MismatchRelation::RelLT);
            assert_eq!(mm.supplied, 900);
            assert_eq!(mm.expected, 1000);
        } else {
            panic!("expected typed tag-8, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "MIRCertificateTooLateinEpochDELEG (Mismatch (RelLT) {supplied: 900, expected: 1000})"
        );
    }

    #[test]
    fn shelley_deleg_pred_failure_insufficient_for_transfer_decodes_tag13() {
        // outer [0x83, 0x0D, pot=1 (Treasury), mismatch [supplied=50, expected=100]]
        let cbor = [0x83_u8, 0x0D, 0x01, 0x82, 0x18, 50, 0x18, 100];
        let f = ShelleyDelegPredFailure::from_cbor(&cbor).expect("InsufficientForTransferDELEG");
        if let ShelleyDelegPredFailure::InsufficientForTransferDELEG { pot, mismatch } = &f {
            assert_eq!(*pot, MirPot::TreasuryMIR);
            assert_eq!(mismatch.supplied, 50);
            assert_eq!(mismatch.expected, 100);
        } else {
            panic!("expected typed tag-13, got {f:?}");
        }
        assert_eq!(
            f.to_string(),
            "InsufficientForTransferDELEG TreasuryMIR (Mismatch (RelLTEQ) {supplied: Coin 50, expected: Coin 100})"
        );
    }

    #[test]
    fn shelley_deleg_pred_failure_mir_negative_transfer_decodes_tag15() {
        // outer [0x83, 0x0F, pot=0, amount=1234]
        let cbor = [0x83_u8, 0x0F, 0x00, 0x19, 0x04, 0xD2];
        let f = ShelleyDelegPredFailure::from_cbor(&cbor).expect("MIRNegativeTransfer");
        if let ShelleyDelegPredFailure::MIRNegativeTransfer { pot, amount } = &f {
            assert_eq!(*pot, MirPot::ReservesMIR);
            assert_eq!(*amount, 1234);
        } else {
            panic!("expected typed tag-15, got {f:?}");
        }
        assert_eq!(f.to_string(), "MIRNegativeTransfer ReservesMIR (Coin 1234)");
    }

    #[test]
    fn mir_pot_from_decoder_round_trips() {
        use yggdrasil_ledger::Decoder;
        let mut dec0 = Decoder::new(&[0x00]);
        assert_eq!(
            MirPot::from_decoder(&mut dec0).expect("reserves"),
            MirPot::ReservesMIR
        );
        let mut dec1 = Decoder::new(&[0x01]);
        assert_eq!(
            MirPot::from_decoder(&mut dec1).expect("treasury"),
            MirPot::TreasuryMIR
        );
        let mut dec_bad = Decoder::new(&[0x02]);
        let err = MirPot::from_decoder(&mut dec_bad).expect_err("unknown rejects");
        assert!(err.to_string().contains("unknown pot 2"), "got: {err}");
    }

    #[test]
    fn shelley_delpl_pred_failure_unknown_tag_rejects() {
        let cbor = vec![0x82_u8, 0x18, 88, 0x40];
        let err = ShelleyDelplPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 88"),
            "got: {err}"
        );
    }

    #[test]
    fn shelley_delegs_pred_failure_unknown_tag_rejects() {
        let cbor = vec![0x82_u8, 0x18, 99, 0x40];
        let err = ShelleyDelegsPredFailure::from_cbor(&cbor).expect_err("unknown tag must reject");
        assert!(
            err.to_string().contains("unknown variant tag 99"),
            "got: {err}"
        );
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
        // Tag 6 with NonEmpty [TxOut(addr, coin=100)]; Shelley
        // TxOut is a 2-array [bytes(addr), coin].
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
            let entry = &outs.entries[0];
            assert_eq!(entry.addr.0.len(), 29);
            assert_eq!(entry.addr.0[0], 0x61);
            assert_eq!(entry.coin, 100);
        } else {
            panic!("expected OutputTooSmallUTxO typed, got {f:?}");
        }
        let s = f.to_string();
        // Header byte 0x61 = enterprise address (high nibble
        // 0x60), payment=KeyHash, network=Mainnet (low nibble
        // 0x01). Body = 28×0xAA. R621 renders the typed shape:
        // `OutputTooSmallUTxO ((Addr Mainnet (KeyHashObj (KeyHash
        // {unKeyHash = "aaaa..."})) (StakeRefNull), Coin 100) :|
        // [])`.
        assert!(
            s.starts_with(
                "OutputTooSmallUTxO ((Addr Mainnet (KeyHashObj (KeyHash {unKeyHash = \"aaaa"
            ),
            "got: {s}"
        );
        assert!(s.contains("StakeRefNull"), "got: {s}");
        assert!(s.contains(", Coin 100)"), "got: {s}");
        assert!(s.ends_with(":| [])"), "got: {s}");
    }

    #[test]
    fn addr_typed_display_covers_all_shelley_types() {
        // Base address — key/key, Mainnet, 28+28 body.
        let mut base = vec![0x01_u8]; // 0000_0001
        base.extend_from_slice(&[0x11_u8; 28]);
        base.extend_from_slice(&[0x22_u8; 28]);
        let s = Addr(base).to_string();
        assert!(
            s.starts_with("Addr Mainnet (KeyHashObj (KeyHash {unKeyHash = \"11"),
            "got: {s}"
        );
        assert!(
            s.contains("StakeRefBase (KeyHashObj (KeyHash {unKeyHash = \"22"),
            "got: {s}"
        );

        // Base address — script/script, Testnet (low nibble 0).
        let mut base_ss = vec![0x30_u8]; // 0011_0000
        base_ss.extend_from_slice(&[0x33_u8; 28]);
        base_ss.extend_from_slice(&[0x44_u8; 28]);
        let s = Addr(base_ss).to_string();
        assert!(
            s.starts_with("Addr Testnet (ScriptHashObj (ScriptHash \"33"),
            "got: {s}"
        );
        assert!(
            s.contains("StakeRefBase (ScriptHashObj (ScriptHash \"44"),
            "got: {s}"
        );

        // Enterprise address — script-payment, no stake.
        let mut ent = vec![0x71_u8]; // 0111_0001
        ent.extend_from_slice(&[0x55_u8; 28]);
        let s = Addr(ent).to_string();
        assert!(
            s.starts_with("Addr Mainnet (ScriptHashObj (ScriptHash \"55"),
            "got: {s}"
        );
        assert!(s.ends_with("StakeRefNull)"), "got: {s}");

        // Pointer address — key-payment + 3 VLQ-encoded Ptr
        // fields (slot=5, tx_ix=3, cert_ix=7). Each fits in a
        // single byte with the continuation bit clear.
        let mut ptr = vec![0x40_u8]; // 0100_0000
        ptr.extend_from_slice(&[0x66_u8; 28]);
        ptr.extend_from_slice(&[0x05, 0x03, 0x07]);
        let s = Addr(ptr).to_string();
        assert!(
            s.starts_with("Addr Testnet (KeyHashObj (KeyHash {unKeyHash = \"66"),
            "got: {s}"
        );
        assert!(
            s.contains(
                "StakeRefPtr (Ptr (SlotNo32 5) (TxIx {unTxIx = 3}) (CertIx {unCertIx = 7}))"
            ),
            "got: {s}"
        );

        // Pointer address — multi-byte VLQ for slot (300 →
        // 0x82 0x2C: 0x82 = 0x80 | 0x02, then 0x2C = 44; combined
        // (2 << 7) | 44 = 300).
        let mut ptr_multi = vec![0x40_u8];
        ptr_multi.extend_from_slice(&[0x77_u8; 28]);
        ptr_multi.extend_from_slice(&[0x82, 0x2C, 0x01, 0x02]);
        let s = Addr(ptr_multi).to_string();
        assert!(
            s.contains(
                "StakeRefPtr (Ptr (SlotNo32 300) (TxIx {unTxIx = 1}) (CertIx {unCertIx = 2}))"
            ),
            "got: {s}"
        );

        // Pointer address — truncated tail (missing cert_ix)
        // routes to the malformed marker.
        let mut ptr_bad = vec![0x40_u8];
        ptr_bad.extend_from_slice(&[0x88_u8; 28]);
        ptr_bad.extend_from_slice(&[0x05, 0x03]);
        let s = Addr(ptr_bad).to_string();
        assert!(s.contains("StakeRefPtr <malformed-ptr"), "got: {s}");

        // Byron bootstrap (bit 7 set).
        let boot = vec![0x82_u8, 0x11, 0x22, 0x33];
        let s = Addr(boot).to_string();
        assert!(
            s.starts_with("AddrBootstrap <hex 4 bytes: 82112233"),
            "got: {s}"
        );
    }

    #[test]
    fn shelley_tx_out_typed_round_trip() {
        // 2-array: [bytes(29) addr, coin=1_000_000]
        let mut cbor = vec![0x82_u8];
        cbor.push(0x58);
        cbor.push(29);
        cbor.push(0xE1);
        cbor.extend_from_slice(&[0x77_u8; 28]);
        cbor.extend_from_slice(&[0x1A, 0x00, 0x0F, 0x42, 0x40]); // 1_000_000
        use yggdrasil_ledger::Decoder;
        let mut dec = Decoder::new(&cbor);
        let tx_out = ShelleyTxOut::from_decoder(&mut dec).expect("ShelleyTxOut");
        assert_eq!(tx_out.addr.0.len(), 29);
        assert_eq!(tx_out.addr.0[0], 0xE1);
        assert_eq!(tx_out.coin, 1_000_000);
        let s = tx_out.to_string();
        // 0xE1 sets bit 7 → Byron bootstrap branch in the typed
        // header decoder. Display renders as `AddrBootstrap` with
        // the hex marker (full Byron typed parse pending).
        assert!(
            s.starts_with("(AddrBootstrap <hex 29 bytes: e177"),
            "got: {s}"
        );
        assert!(s.ends_with(", Coin 1000000)"), "got: {s}");
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
        // Header 0x61 = enterprise/key/Mainnet; body 28 bytes of
        // 0x77. Display renders the typed shape via the typed
        // Addr enum.
        assert!(
            s.starts_with("WrongNetwork Mainnet (NonEmptySet (fromList [Addr Mainnet (KeyHashObj (KeyHash {unKeyHash = \"77"),
            "got: {s}"
        );
        assert!(s.contains("StakeRefNull"), "got: {s}");
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
