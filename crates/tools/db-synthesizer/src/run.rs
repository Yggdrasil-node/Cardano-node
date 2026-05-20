//! ChainDB-open + synthesize supervisor for the `db-synthesizer` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/Run.hs.
//!
//! Direct port of upstream's `Cardano.Tools.DBSynthesizer.Run`:
//!
//! - [`pre_open_chain_db`] — mirror of upstream `preOpenChainDB`: the
//!   `DBSynthesizerOpenMode` (`OpenCreate`/`OpenCreateForce`/`OpenAppend`)
//!   directory-state handling, including the `checkIsDB` heuristic
//!   (a directory "looks like a ChainDB" iff every entry is one of
//!   `immutable`/`ledger`/`volatile`/`gsm`).
//! - [`synthesize`] — config-free structural helper: opens the
//!   ChainDB at `confDbDir`, finds the resume slot from the current
//!   tip, and drives [`crate::forging::run_structural_forge`].
//! - [`resolve_epoch_size_from_config`] — mirror of the genesis-loading
//!   half of upstream `initialize`/`initConf`: `config.json` →
//!   `NodeConfigStub` → Shelley genesis → real `epochLength` (R2).
//! - [`synthesize_from_config`] — the production entry point: resolve
//!   the consensus protocol, build the initial ledger / nonce forge
//!   state, read leader credentials, then drive the Praos forge loop.
//!
//! ## Slice boundary (post Phase 4 R3c-5)
//!
//! - **`initialize` — protocol-building half.** Upstream's
//!   `initProtocol` builds `CardanoProtocolParams` via
//!   `mkConsensusProtocolCardano` (the multi-era hard-fork plan).
//!   R3b-1 ported the genesis-reading half ([`load_genesis_bundle`]),
//!   R3b-2 the per-era protocol-config types, and R3b-3
//!   [`load_consensus_protocol`] / [`mk_consensus_protocol_cardano`]
//!   the [`CardanoProtocolParams`] aggregate. R3c-4 consumes it for
//!   Praos leader checking and KES-signed block forging.
//! - **Epoch-boundary stake rebuild** — R3c-5 derives the Praos leader
//!   sigma from the rotating ledger-view stake snapshots, including
//!   Shelley genesis staking pools and delegations.
//!
//! What this slice DOES port faithfully: the `preOpenChainDB` mode
//! semantics (directory creation, the `OpenCreate`-refuses-non-empty
//! rule, `OpenAppend`/`OpenCreateForce` on a ChainDB-shaped directory,
//! and the abort-on-foreign-directory rule) and the tip-driven
//! resume-slot derivation.

use std::collections::BTreeSet;
use std::path::Path;

use yggdrasil_consensus::{
    ActiveSlotCoeff, EpochSize, NonceDerivation, NonceEvolutionConfig, NonceEvolutionState,
};
use yggdrasil_ledger::{
    Address, Delegations, IndividualStake, StakeCredential, StakeSnapshot, StakeSnapshots,
};
use yggdrasil_ledger::{Era, LedgerState, Nonce, Point, SlotNo};
use yggdrasil_node_block_producer::{
    BlockProducerCredentials, BlockProducerError, load_block_producer_credentials,
    load_bulk_block_producer_credentials,
};
use yggdrasil_node_config::MAINNET_NETWORK_MAGIC;
use yggdrasil_node_genesis::{
    AlonzoGenesis, BaseLedgerStateInputs, ByronGenesisUtxoEntry, ConwayGenesis, ShelleyGenesis,
    ShelleyGenesisBootstrap, build_genesis_enact_state, build_protocol_parameters,
    build_shelley_genesis_bootstrap, compute_genesis_file_hash, genesis_extra_entropy_to_nonce,
    load_alonzo_genesis, load_byron_genesis_utxo, load_conway_genesis, load_shelley_genesis,
    shelley_genesis_hash_to_praos_nonce,
};
use yggdrasil_storage::{FileImmutable, ImmutableStore, StorageError};

use crate::forging::{
    ForgeRunOutcome, ForgeRuntimeConfig, ForgeState, run_forge, run_structural_forge,
};
use crate::orphans::{AdjustFilePaths, parse_node_config_stub};
use crate::types::{
    DBSynthesizerOpenMode, DBSynthesizerOptions, ForgeLimit, NodeByronProtocolConfiguration,
    NodeConfigStub, NodeCredentials, NodeHardForkProtocolConfiguration,
};

/// Fallback epoch length for the config-free [`synthesize_default`]
/// convenience.
///
/// `432000` is the mainnet Shelley `epochLength`. The production entry
/// point [`synthesize_from_config`] parses the real `sgEpochLength`
/// from the node config's Shelley genesis (R2); this constant only
/// backs the config-free test / convenience helper.
pub const DEFAULT_EPOCH_SIZE: u64 = 432_000;

/// Era tag stamped by the config-free structural helper.
///
/// The production path now forges Conway-shaped Praos blocks through
/// `crates/node/block-producer`; this tag remains only for
/// [`synthesize_default`] and focused structural tests.
pub const SYNTH_ERA: Era = Era::Shelley;

/// Directory names that mark a directory as a ChainDB.
///
/// Mirror of upstream `chainDBDirs = Set.fromList ["immutable",
/// "ledger", "volatile", "gsm"]`. Yggdrasil's `FileImmutable` also
/// writes a `.dirty` marker file plus per-block CBOR files inside
/// `immutable/`, so the directory-level heuristic is applied against
/// sub-directories only (matching upstream's `listSubdirectories`).
const CHAIN_DB_DIRS: &[&str] = &["immutable", "ledger", "volatile", "gsm"];

/// Errors from the db-synthesizer run supervisor.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Underlying storage failure (ChainDB open / append).
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    /// Forge-loop failure after ChainDB open.
    #[error("forge error: {0}")]
    Forge(#[from] crate::forging::ForgeError),
    /// Consensus parameter validation failed while building the Praos
    /// forge runtime.
    #[error("initialize: invalid consensus parameter: {0}")]
    Consensus(#[from] yggdrasil_consensus::ConsensusError),
    /// `OpenCreate` was requested but the target directory already
    /// exists. Mirror of upstream's
    /// `"<loc> already exists. Use -f to overwrite or -a to append."`.
    #[error("preOpenChainDB: '{0}' already exists. Use -f to overwrite or -a to append.")]
    AlreadyExists(String),
    /// The target directory is non-empty and does not look like a
    /// ChainDB. Mirror of upstream's foreign-directory abort.
    #[error(
        "preOpenChainDB: '{0}' is non-empty and does not look like a ChainDB \
         (i.e. it contains directories other than 'immutable'/'ledger'/'volatile'/'gsm'). \
         Aborting."
    )]
    NotAChainDb(String),
    /// An I/O error while inspecting / preparing the target directory.
    #[error("preOpenChainDB: I/O error on '{path}': {source}")]
    Io {
        /// Target directory the error occurred on.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The node `config.json` could not be read. Mirror of upstream
    /// `initConf`'s `handleIOExceptT show (BS.readFile nfpConfig)`.
    #[error("initialize: cannot read config '{path}': {source}")]
    ConfigRead {
        /// Config-file path.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The node `config.json` is not valid JSON.
    #[error("initialize: config '{path}' is not valid JSON: {source}")]
    ConfigParse {
        /// Config-file path.
        path: String,
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// The node `config.json` is not a well-formed `NodeConfigStub`.
    /// Mirror of upstream `initConf`'s `readJson` failure path.
    #[error("initialize: {0}")]
    ConfigStub(#[from] crate::orphans::NodeConfigStubParseError),
    /// A genesis file referenced by the config could not be loaded.
    /// Mirror of upstream `initConf` / `mkConsensusProtocolCardano`
    /// genesis-read failures.
    #[error("initialize: cannot load genesis: {0}")]
    GenesisLoad(#[from] yggdrasil_node_genesis::GenesisLoadError),
    /// A `Node{Byron,HardFork}ProtocolConfiguration` could not be parsed
    /// from the node-config JSON. Mirror of upstream `initProtocol`'s
    /// `eitherParseJson` failure.
    #[error("initialize: cannot parse protocol configuration from node config: {source}")]
    ProtocolConfigParse {
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// A leader-credential file (a singleton key/cert file or the bulk
    /// credentials file) could not be loaded. Mirror of upstream
    /// `readLeaderCredentials` surfacing a `PraosLeaderCredentialsError`.
    #[error("initialize: cannot load leader credentials: {0}")]
    Credentials(#[from] BlockProducerError),
    /// Some — but not all — of the singleton operational-certificate /
    /// VRF-key / KES-key credential files were supplied. Mirror of
    /// upstream `readLeaderCredentialsSingleton`'s `OCertNotSpecified` /
    /// `VRFKeyNotSpecified` / `KESKeyNotSpecified` — the three singleton
    /// files are all-or-nothing.
    #[error("initialize: incomplete singleton leader credentials — {missing} not specified")]
    IncompleteCredentials {
        /// Which of the three singleton credential files is missing.
        missing: &'static str,
    },
}

/// Whether a directory's sub-directory set marks it as a ChainDB.
///
/// Mirror of upstream `checkIsDB ls = Set.fromList ls
/// \`Set.isSubsetOf\` chainDBDirs` — the directory looks like a
/// ChainDB iff every sub-directory is one of the known ChainDB
/// directories. An empty set is trivially a subset (a fresh ChainDB
/// dir before any block is written).
fn looks_like_chain_db(subdirs: &BTreeSet<String>) -> bool {
    let known: BTreeSet<String> = CHAIN_DB_DIRS.iter().map(|s| (*s).to_string()).collect();
    subdirs.is_subset(&known)
}

/// List the immediate sub-directory names of `path`.
///
/// Mirror of upstream `listSubdirectories` (`filterM isDir =<<
/// listDirectory path`).
fn list_subdirectories(path: &Path) -> Result<BTreeSet<String>, std::io::Error> {
    let mut out = BTreeSet::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            out.insert(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(out)
}

/// Prepare the target ChainDB directory according to `mode`.
///
/// Mirror of upstream `preOpenChainDB`:
///
/// - directory absent → create it (all modes).
/// - directory present + `OpenCreate` → fail [`RunError::AlreadyExists`].
/// - directory present + ChainDB-shaped + `OpenAppend` → leave intact.
/// - directory present + ChainDB-shaped + `OpenCreateForce` → wipe +
///   recreate.
/// - directory present + foreign-shaped → fail [`RunError::NotAChainDb`].
pub fn pre_open_chain_db(mode: DBSynthesizerOpenMode, db: &Path) -> Result<(), RunError> {
    let loc = db.display().to_string();
    let io_err = |source: std::io::Error| RunError::Io {
        path: loc.clone(),
        source,
    };

    if !db.exists() {
        std::fs::create_dir_all(db).map_err(io_err)?;
        return Ok(());
    }

    let subdirs = list_subdirectories(db).map_err(io_err)?;
    let is_chain_db = looks_like_chain_db(&subdirs);

    match mode {
        DBSynthesizerOpenMode::OpenCreate => Err(RunError::AlreadyExists(loc)),
        DBSynthesizerOpenMode::OpenAppend if is_chain_db => Ok(()),
        DBSynthesizerOpenMode::OpenCreateForce if is_chain_db => {
            std::fs::remove_dir_all(db).map_err(io_err)?;
            std::fs::create_dir_all(db).map_err(io_err)?;
            Ok(())
        }
        _ => Err(RunError::NotAChainDb(loc)),
    }
}

/// Outcome of a [`synthesize`] run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SynthesizeOutcome {
    /// The forge-loop outcome (block count + final accumulator).
    pub forge: ForgeRunOutcome,
    /// Slot the forge loop resumed from (0 for a fresh ChainDB,
    /// `tip_slot + 1` when appending).
    pub resumed_from: SlotNo,
    /// Whether this invocation opened the ChainDB.
    ///
    /// Upstream returns before opening the DB when the forger set is
    /// empty; the CLI uses this to report that exact no-forgers path
    /// without implying a DB mutation.
    pub chain_db_opened: bool,
}

/// Open the ChainDB and synthesize blocks up to `options.limit`.
///
/// Mirror of upstream `synthesize`: it `preOpenChainDB`s the target,
/// opens the ChainDB, computes the resume slot from the current tip
/// (`Origin → 0`, `At s → succ s`), and runs the forge loop.
///
/// `epoch_size` is the Shelley-genesis `sgEpochLength`. The production
/// path [`synthesize_from_config`] resolves it from the node config;
/// [`synthesize_default`] passes [`DEFAULT_EPOCH_SIZE`].
pub fn synthesize(
    options: DBSynthesizerOptions,
    db_dir: &Path,
    epoch_size: u64,
    era: Era,
) -> Result<SynthesizeOutcome, RunError> {
    pre_open_chain_db(options.open_mode, db_dir)?;

    let mut store = FileImmutable::open(db_dir)?;

    // Mirror of upstream's tip-driven resume:
    //   slotNo = case pointSlot tip of { Origin -> 0; At s -> succ s }
    let resumed_from = match store.get_tip() {
        Point::Origin => SlotNo(0),
        Point::BlockPoint(slot, _) => SlotNo(slot.0 + 1),
    };

    let forge = run_structural_forge(era, epoch_size, resumed_from, options.limit, &mut store)?;
    Ok(SynthesizeOutcome {
        forge,
        resumed_from,
        chain_db_opened: true,
    })
}

fn synthesize_with_forge_state(
    options: DBSynthesizerOptions,
    db_dir: &Path,
    epoch_size: u64,
    initial_state: InitialForgeState,
    runtime_config: ForgeRuntimeConfig,
    forgers: &mut [BlockProducerCredentials],
) -> Result<SynthesizeOutcome, RunError> {
    let forge_state = ForgeState::initial_with_stake_snapshots(
        initial_state.ledger_state,
        initial_state.nonce_evolution,
        initial_state.stake_snapshots,
    );

    if forgers.is_empty() {
        return Ok(SynthesizeOutcome {
            forge: ForgeRunOutcome {
                result: crate::types::ForgeResult { forged: 0 },
                final_state: forge_state,
            },
            resumed_from: SlotNo(0),
            chain_db_opened: false,
        });
    }

    pre_open_chain_db(options.open_mode, db_dir)?;

    let mut store = FileImmutable::open(db_dir)?;
    let resumed_from = match store.get_tip() {
        Point::Origin => SlotNo(0),
        Point::BlockPoint(slot, _) => SlotNo(slot.0 + 1),
    };

    let forge = run_forge(
        epoch_size,
        resumed_from,
        options.limit,
        &mut store,
        forge_state,
        runtime_config,
        forgers,
    )?;
    Ok(SynthesizeOutcome {
        forge,
        resumed_from,
        chain_db_opened: true,
    })
}

/// [`synthesize`] with [`DEFAULT_EPOCH_SIZE`] + [`SYNTH_ERA`].
///
/// A config-free convenience: forges without reading a node
/// `config.json`. The production run-loop dispatch ([`crate::run`])
/// goes through [`synthesize_from_config`], which resolves the real
/// genesis epoch length; `synthesize_default` backs unit tests and
/// callers without a config file.
pub fn synthesize_default(
    options: DBSynthesizerOptions,
    db_dir: &Path,
) -> Result<SynthesizeOutcome, RunError> {
    synthesize(options, db_dir, DEFAULT_EPOCH_SIZE, SYNTH_ERA)
}

/// Resolve the real Shelley-genesis epoch length from the node
/// `config.json`.
///
/// Mirror of the genesis-loading half of upstream
/// `Cardano.Tools.DBSynthesizer.Run.initialize` (`initConf`): read the
/// config file, parse it into a
/// [`NodeConfigStub`](crate::types::NodeConfigStub), resolve the
/// embedded genesis paths relative to the config file's own directory
/// (upstream `relativeToConfig`), load the Shelley genesis, and return
/// its `epochLength`. Upstream `synthesize` then uses
/// `epochSize = sgEpochLength confShelleyGenesis`.
///
/// The protocol-building half (`initProtocol` /
/// `mkConsensusProtocolCardano`) is the db-synthesizer R3 carve-out.
pub fn resolve_epoch_size_from_config(config_path: &Path) -> Result<u64, RunError> {
    let stub = resolve_node_config_stub(config_path)?;
    let genesis = load_shelley_genesis(&stub.shelley_genesis_file)?;
    Ok(genesis.epoch_length)
}

/// Read the node `config.json`, parse it into a [`NodeConfigStub`], and
/// resolve the embedded genesis paths relative to the config file's own
/// directory.
///
/// Mirror of the `relativeToConfig` half of upstream
/// `Cardano.Tools.DBSynthesizer.Run.initialize` — `relativeToConfig =
/// (</>) . takeDirectory <$> makeAbsolute nfpConfig` resolves genesis
/// paths against the config file's absolute directory. `PathBuf::join`
/// mirrors Haskell `(</>)`: an absolute genesis path is kept as-is.
fn resolve_node_config_stub(config_path: &Path) -> Result<NodeConfigStub, RunError> {
    let raw = std::fs::read_to_string(config_path).map_err(|source| RunError::ConfigRead {
        path: config_path.display().to_string(),
        source,
    })?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|source| RunError::ConfigParse {
            path: config_path.display().to_string(),
            source,
        })?;
    let stub = parse_node_config_stub(value)?;

    let abs_config = std::path::absolute(config_path).map_err(|source| RunError::ConfigRead {
        path: config_path.display().to_string(),
        source,
    })?;
    let config_dir = abs_config
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    Ok(stub.adjust_file_paths(|p| config_dir.join(p)))
}

/// Every-era genesis loaded from the node config, plus the initial
/// Praos nonce.
///
/// Upstream `mkConsensusProtocolCardano` (`Cardano.Node.Protocol.Cardano`)
/// reads the per-era genesis files inline while building
/// `CardanoProtocolParams`; there is no upstream `GenesisBundle` type.
/// This aggregate collects that genesis-reading step as a typed value
/// so the R3b-3 `mk_consensus_protocol_cardano` orchestration can fold
/// it. Dijkstra is omitted — that era is not yet activated in yggdrasil
/// (no `load_dijkstra_genesis`); upstream defaults it to an empty
/// genesis when the config omits it.
#[derive(Clone, Debug)]
pub struct GenesisBundle {
    /// Byron genesis UTxO entries (`nonAvvmBalances` + `avvmDistr`).
    pub byron: Vec<ByronGenesisUtxoEntry>,
    /// Parsed Shelley genesis.
    pub shelley: ShelleyGenesis,
    /// Parsed Alonzo genesis.
    pub alonzo: AlonzoGenesis,
    /// Parsed Conway genesis.
    pub conway: ConwayGenesis,
    /// Initial Praos nonce — the Blake2b-256 hash of the Shelley genesis
    /// file (upstream `genesisHashToPraosNonce`).
    pub praos_nonce: Nonce,
}

/// Load every era's genesis referenced by the node `config.json` and
/// derive the initial Praos nonce.
///
/// The genesis-reading half of upstream `mkConsensusProtocolCardano`
/// (`Cardano.Node.Protocol.Cardano`): with the config-relative genesis
/// paths resolved ([`resolve_node_config_stub`]), read the Byron /
/// Shelley / Alonzo / Conway genesis files and derive the initial Praos
/// nonce from the Shelley genesis file hash (`genesisHashToPraosNonce`).
///
/// The per-era protocol configs + hard-fork triggers (R3b-2) and the
/// `CardanoProtocolParams` aggregator (R3b-3) now fold this bundle in
/// [`load_consensus_protocol`].
pub fn load_genesis_bundle(config_path: &Path) -> Result<GenesisBundle, RunError> {
    let stub = resolve_node_config_stub(config_path)?;
    load_genesis_bundle_from_stub(&stub)
}

/// [`load_genesis_bundle`] from an already-resolved [`NodeConfigStub`].
fn load_genesis_bundle_from_stub(stub: &NodeConfigStub) -> Result<GenesisBundle, RunError> {
    let byron = load_byron_genesis_utxo(&stub.byron_genesis_file)?;
    let shelley = load_shelley_genesis(&stub.shelley_genesis_file)?;
    let alonzo = load_alonzo_genesis(&stub.alonzo_genesis_file)?;
    let conway = load_conway_genesis(&stub.conway_genesis_file)?;

    // Upstream `genesisHashToPraosNonce`: the initial Praos nonce is the
    // Blake2b-256 hash of the Shelley genesis file's raw bytes.
    let shelley_hash = compute_genesis_file_hash(&stub.shelley_genesis_file)?;
    let hash_hex: String = shelley_hash.iter().map(|b| format!("{b:02x}")).collect();
    let praos_nonce = shelley_genesis_hash_to_praos_nonce(&hash_hex)?;

    Ok(GenesisBundle {
        byron,
        shelley,
        alonzo,
        conway,
        praos_nonce,
    })
}

/// When an era's hard fork activates.
///
/// Synthesizer-scoped 2-variant mirror of upstream `CardanoHardForkTrigger`
/// (`Ouroboros.Consensus.Cardano.Node`): a fork fires either at its
/// default protocol-version bump, or — for testing — at an explicit
/// epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HardForkTrigger {
    /// Fork at the era's default protocol-version bump.
    AtDefaultVersion,
    /// Fork at this exact epoch (a `Test*HardForkAtEpoch` override).
    AtEpoch(u64),
}

impl HardForkTrigger {
    /// Map an optional `Test*HardForkAtEpoch` config value to a trigger.
    fn from_test_epoch(epoch: Option<u64>) -> Self {
        match epoch {
            Some(e) => Self::AtEpoch(e),
            None => Self::AtDefaultVersion,
        }
    }
}

/// Per-era hard-fork triggers.
///
/// Synthesizer-scoped mirror of upstream `CardanoHardForkTriggers`
/// (`Ouroboros.Consensus.Cardano.Node`) — upstream's is a typed `NP`
/// n-ary product over the hard-fork-combinator era list; this is a flat
/// per-Shelley-era struct of [`HardForkTrigger`]s case-mapped from
/// [`NodeHardForkProtocolConfiguration`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CardanoHardForkTriggers {
    /// Shelley hard-fork trigger.
    pub shelley: HardForkTrigger,
    /// Allegra hard-fork trigger.
    pub allegra: HardForkTrigger,
    /// Mary hard-fork trigger.
    pub mary: HardForkTrigger,
    /// Alonzo hard-fork trigger.
    pub alonzo: HardForkTrigger,
    /// Babbage hard-fork trigger.
    pub babbage: HardForkTrigger,
    /// Conway hard-fork trigger.
    pub conway: HardForkTrigger,
    /// Dijkstra hard-fork trigger.
    pub dijkstra: HardForkTrigger,
}

/// Shelley-based protocol parameters.
///
/// Synthesizer-scoped mirror of upstream `ProtocolParamsShelleyBased`
/// (`Ouroboros.Consensus.Shelley.Node.Common`). Upstream also carries
/// the Shelley leader credentials; the synthesizer threads credentials
/// separately, so this keeps just `shelleyBasedInitialNonce`.
#[derive(Clone, Debug)]
pub struct ShelleyBasedProtocolParams {
    /// Initial Praos nonce — `genesisHashToPraosNonce` of the Shelley
    /// genesis hash.
    pub initial_nonce: Nonce,
}

/// Block checkpoints.
///
/// Upstream `mkConsensusProtocolCardano` always supplies
/// `emptyCheckpointsMap`; the synthesizer carries this zero-field type
/// so [`CardanoProtocolParams`] keeps the upstream field name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckpointsMap;

/// Cardano consensus protocol parameters.
///
/// Synthesizer-scoped mirror of upstream `CardanoProtocolParams`
/// (`Ouroboros.Consensus.Cardano.Node`). Upstream's field types are the
/// full hard-fork-combinator machinery (`ProtocolParamsByron` carrying
/// the entire Byron `Genesis.Config`, `CardanoHardForkTriggers` as a
/// typed `NP`, an era-crossing `TransitionConfig`). The db-synthesizer
/// forges single-era and consumes none of that, so this keeps the six
/// upstream field *names* with synthesizer-scoped types wrapping the
/// already-built genesis bundle (R3b-1) and per-era configs (R3b-2).
///
/// ## Naming parity
///
/// **Strict mirror:** none. Synthesizer-scoped simplification of
/// `CardanoProtocolParams` + `ProtocolParamsShelleyBased` /
/// `CardanoHardForkTriggers` / `CheckpointsMap` from upstream
/// `Ouroboros.Consensus.Cardano.Node`.
#[derive(Clone, Debug)]
pub struct CardanoProtocolParams {
    /// Byron protocol configuration (R3b-2).
    pub byron_protocol_params: NodeByronProtocolConfiguration,
    /// Shelley-based protocol parameters.
    pub shelley_based_protocol_params: ShelleyBasedProtocolParams,
    /// Per-era hard-fork triggers.
    pub cardano_hard_fork_triggers: CardanoHardForkTriggers,
    /// Every era's genesis — the `mkLatestTransitionConfig` analog.
    pub cardano_ledger_transition_config: GenesisBundle,
    /// Block checkpoints — always empty for the synthesizer.
    pub cardano_checkpoints: CheckpointsMap,
    /// `(major, minor)` protocol version the synthesized chain declares.
    pub cardano_protocol_version: (u64, u64),
}

/// Fold the parsed config pieces into [`CardanoProtocolParams`].
///
/// Synthesizer-scoped mirror of upstream `mkConsensusProtocolCardano`
/// (`Cardano.Node.Protocol.Cardano`): assembles the Byron + hard-fork
/// protocol configs (R3b-2) and the multi-era genesis bundle (R3b-1)
/// into the [`CardanoProtocolParams`] aggregate. Operator credentials
/// are threaded into the forge separately (R3a / R3c), not here.
pub fn mk_consensus_protocol_cardano(
    byron: NodeByronProtocolConfiguration,
    hard_fork: NodeHardForkProtocolConfiguration,
    bundle: GenesisBundle,
) -> CardanoProtocolParams {
    let cardano_hard_fork_triggers = CardanoHardForkTriggers {
        shelley: HardForkTrigger::from_test_epoch(hard_fork.test_shelley_hard_fork_at_epoch),
        allegra: HardForkTrigger::from_test_epoch(hard_fork.test_allegra_hard_fork_at_epoch),
        mary: HardForkTrigger::from_test_epoch(hard_fork.test_mary_hard_fork_at_epoch),
        alonzo: HardForkTrigger::from_test_epoch(hard_fork.test_alonzo_hard_fork_at_epoch),
        babbage: HardForkTrigger::from_test_epoch(hard_fork.test_babbage_hard_fork_at_epoch),
        conway: HardForkTrigger::from_test_epoch(hard_fork.test_conway_hard_fork_at_epoch),
        dijkstra: HardForkTrigger::from_test_epoch(hard_fork.test_dijkstra_hard_fork_at_epoch),
    };

    // Upstream `Cardano.hs`: `ProtVer 11 0` when development hard-fork
    // eras are enabled, else `ProtVer 10 7`.
    let cardano_protocol_version = if hard_fork.test_enable_development_hard_fork_eras {
        (11, 0)
    } else {
        (10, 7)
    };

    let shelley_based_protocol_params = ShelleyBasedProtocolParams {
        initial_nonce: bundle.praos_nonce,
    };

    CardanoProtocolParams {
        byron_protocol_params: byron,
        shelley_based_protocol_params,
        cardano_hard_fork_triggers,
        cardano_ledger_transition_config: bundle,
        cardano_checkpoints: CheckpointsMap,
        cardano_protocol_version,
    }
}

/// Load the Cardano consensus protocol parameters from the operator's
/// node `config.json`.
///
/// The protocol-building half of upstream
/// `Cardano.Tools.DBSynthesizer.Run.initProtocol`: parses the Byron and
/// hard-fork protocol configurations from the node-config JSON, loads
/// the multi-era genesis bundle, and folds them via
/// [`mk_consensus_protocol_cardano`].
pub fn load_consensus_protocol(config_path: &Path) -> Result<CardanoProtocolParams, RunError> {
    let stub = resolve_node_config_stub(config_path)?;

    let byron: NodeByronProtocolConfiguration = serde_json::from_value(stub.node_config.clone())
        .map_err(|source| RunError::ProtocolConfigParse { source })?;
    let hard_fork: NodeHardForkProtocolConfiguration =
        serde_json::from_value(stub.node_config.clone())
            .map_err(|source| RunError::ProtocolConfigParse { source })?;

    let bundle = load_genesis_bundle_from_stub(&stub)?;
    Ok(mk_consensus_protocol_cardano(byron, hard_fork, bundle))
}

/// The synthesizer's seeded initial forge state.
///
/// The genesis-seeded initial `LedgerState` (built via the shared
/// `yggdrasil-node-genesis` builder) plus the Praos `NonceEvolutionState`
/// seeded from the genesis nonce — the synthesizer-side analog of the
/// node's initial `(LedgerState, ChainDepState)` pair, and the
/// `pInfoInitLedger` analog the R3c Praos forge loop will thread.
#[derive(Clone, Debug)]
pub struct InitialForgeState {
    /// Initial multi-era ledger state, genesis-seeded.
    pub ledger_state: LedgerState,
    /// Initial Praos nonce-evolution state.
    pub nonce_evolution: NonceEvolutionState,
    /// Initial forecast stake snapshots used for leader election before
    /// the first synthetic block activates pending Shelley genesis stake.
    pub stake_snapshots: StakeSnapshots,
}

fn stake_snapshots_from_shelley_bootstrap(bootstrap: &ShelleyGenesisBootstrap) -> StakeSnapshots {
    let mut stake = IndividualStake::new();
    let mut delegations = Delegations::new();

    for (stake_hash, pool_hash) in &bootstrap.staking {
        delegations.insert(StakeCredential::AddrKeyHash(*stake_hash), *pool_hash);
    }

    for (_, txout) in &bootstrap.initial_funds {
        let Some(Address::Base(base)) = Address::from_bytes(&txout.address) else {
            continue;
        };
        let StakeCredential::AddrKeyHash(stake_hash) = base.staking else {
            continue;
        };
        if bootstrap.staking.contains_key(&stake_hash) {
            stake.add(base.staking, txout.amount);
        }
    }

    let snapshot = StakeSnapshot {
        stake,
        delegations,
        pool_params: bootstrap.staking_pools.clone(),
    };
    StakeSnapshots {
        mark: snapshot.clone(),
        set: snapshot.clone(),
        go: snapshot,
        fee_pot: 0,
        previous_fee_pot: 0,
    }
}

/// Build the synthesizer's [`InitialForgeState`] from a loaded genesis
/// bundle.
///
/// The synthesizer-side analog of the node's `strict_base_ledger_state`:
/// it folds the [`GenesisBundle`] (R3b-1) through the shared
/// `yggdrasil_node_genesis::build_base_ledger_state` (R3c-1a) so the
/// db-synthesizer and the node seed a byte-identical initial ledger
/// state, and seeds `NonceEvolutionState` from the genesis Praos nonce.
fn build_initial_forge_state(bundle: &GenesisBundle) -> Result<InitialForgeState, RunError> {
    let shelley_bootstrap = build_shelley_genesis_bootstrap(&bundle.shelley)?;
    let stake_snapshots = stake_snapshots_from_shelley_bootstrap(&shelley_bootstrap);
    let inputs = BaseLedgerStateInputs {
        // The node derives the network id from the mandatory
        // `NodeConfigFile::network_magic`; the synthesizer falls back
        // through the optional Shelley-genesis `networkMagic` (present
        // in every vendored mainnet / preprod / preview genesis).
        expected_network_id: bundle
            .shelley
            .network_magic
            .map(|m| u8::from(m == MAINNET_NETWORK_MAGIC))
            .unwrap_or(0),
        byron_entries: bundle.byron.clone(),
        shelley_bootstrap: Some(shelley_bootstrap),
        protocol_params: Some(build_protocol_parameters(
            &bundle.shelley,
            &bundle.alonzo,
            Some(&bundle.conway),
        )?),
        enact_state: build_genesis_enact_state(Some(&bundle.conway))?,
        // The Byron→Shelley boundary scalars are yggdrasil-internal node
        // config keys, absent from every genesis file; the synthesizer
        // forges a single-era Shelley-stamped chain, so the defaults
        // (no boundary, the default Byron epoch length) are exact.
        byron_to_shelley_slot: None,
        first_shelley_epoch: None,
        byron_epoch_length: 21_600,
        active_slot_coeff: bundle.shelley.active_slots_coeff,
        security_param_k: bundle.shelley.security_param,
    };

    Ok(InitialForgeState {
        ledger_state: yggdrasil_node_genesis::build_base_ledger_state(inputs),
        nonce_evolution: NonceEvolutionState::new(bundle.praos_nonce),
        stake_snapshots,
    })
}

fn build_forge_runtime_config(
    bundle: &GenesisBundle,
    protocol_version: (u64, u64),
) -> Result<ForgeRuntimeConfig, RunError> {
    let stability_window = if bundle.shelley.active_slots_coeff > 0.0 {
        (3.0 * bundle.shelley.security_param as f64 / bundle.shelley.active_slots_coeff) as u64
    } else {
        0
    };
    let active_slot_coeff = ActiveSlotCoeff::new(bundle.shelley.active_slots_coeff)?;
    Ok(ForgeRuntimeConfig {
        nonce_config: NonceEvolutionConfig {
            epoch_size: EpochSize(bundle.shelley.epoch_length),
            stability_window,
            extra_entropy: genesis_extra_entropy_to_nonce(
                bundle.shelley.protocol_params.extra_entropy.as_ref(),
            )?,
            byron_shelley_transition: None,
        },
        nonce_derivation: NonceDerivation::Praos,
        active_slot_coeff,
        max_block_body_size: bundle.shelley.protocol_params.max_block_body_size,
        protocol_version,
    })
}

/// Load the synthesizer's [`InitialForgeState`] from the operator's node
/// `config.json`.
///
/// Mirror of the ledger-seeding half of upstream
/// `Cardano.Tools.DBSynthesizer.Run.synthesize` (the `pInfoInitLedger`
/// the forge loop runs on): resolves the config stub, loads the
/// multi-era genesis bundle, and builds the genesis-seeded initial
/// ledger + nonce state. The R3c Praos forge loop threads this state
/// slot-to-slot.
pub fn load_initial_forge_state(config_path: &Path) -> Result<InitialForgeState, RunError> {
    let stub = resolve_node_config_stub(config_path)?;
    let bundle = load_genesis_bundle_from_stub(&stub)?;
    build_initial_forge_state(&bundle)
}

/// Resolve the synthesizer's full set of block-producer leader
/// credentials from a [`NodeCredentials`].
///
/// Mirror of `readLeaderCredentials` in `Cardano.Node.Protocol.Shelley`
/// — upstream's `Run.hs` reaches it via `initProtocol`. The forger set
/// is the union of the singleton CLI cert/vrf/kes triple and the
/// bulk-credentials file; either, both, or neither may be supplied.
///
/// - The singleton triple is all-or-nothing — a partial set is
///   [`RunError::IncompleteCredentials`] (mirror of
///   `readLeaderCredentialsSingleton`).
/// - An absent bulk file contributes nothing (mirror of upstream
///   `readBulkFile Nothing = pure []`).
///
/// `slots_per_kes_period` / `max_kes_evolutions` come from the Shelley
/// genesis (`sgSlotsPerKESPeriod` / `sgMaxKESEvolutions`). The R3c Praos
/// forge loop picks the first slot-leader from the returned list.
pub fn read_leader_credentials(
    credentials: &NodeCredentials,
    slots_per_kes_period: u64,
    max_kes_evolutions: u64,
) -> Result<Vec<BlockProducerCredentials>, RunError> {
    let mut forgers = Vec::new();

    // Singleton: the CLI-supplied cert/vrf/kes triple. Upstream
    // `readLeaderCredentialsSingleton` accepts all three or none.
    match (
        &credentials.cert_file,
        &credentials.vrf_file,
        &credentials.kes_file,
    ) {
        (Some(cert), Some(vrf), Some(kes)) => {
            forgers.push(load_block_producer_credentials(
                kes,
                vrf,
                cert,
                slots_per_kes_period,
                max_kes_evolutions,
            )?);
        }
        (None, None, None) => {}
        (cert, vrf, _) => {
            // Pattern-match precedence mirrors upstream: certificate,
            // then VRF key, then KES key.
            let missing = if cert.is_none() {
                "operational certificate"
            } else if vrf.is_none() {
                "VRF key"
            } else {
                "KES key"
            };
            return Err(RunError::IncompleteCredentials { missing });
        }
    }

    // Bulk: the inline-triple JSON file. An absent file contributes
    // nothing (`readBulkFile Nothing = pure []`).
    if let Some(bulk) = &credentials.bulk_file {
        forgers.extend(load_bulk_block_producer_credentials(
            bulk,
            slots_per_kes_period,
            max_kes_evolutions,
        )?);
    }

    Ok(forgers)
}

/// [`synthesize`] driven by the operator's node `config.json`.
///
/// The production entry point [`crate::run`] uses: it loads the
/// consensus protocol via [`load_consensus_protocol`], builds the
/// genesis-seeded ledger / nonce state, reads leader credentials, and
/// forges with the Shelley-genesis `epochLength` — mirror of upstream
/// `app/db-synthesizer.hs`'s `initialize … >>= synthesize …` path.
///
/// If no forgers are supplied, this returns before opening the ChainDB,
/// matching upstream `Run.hs`. If forgers are supplied, the loop uses
/// the shared Praos leader-check + KES block forge path.
pub fn synthesize_from_config(
    options: DBSynthesizerOptions,
    credentials: &NodeCredentials,
    config_path: &Path,
    db_dir: &Path,
) -> Result<SynthesizeOutcome, RunError> {
    let protocol = load_consensus_protocol(config_path)?;
    let bundle = &protocol.cardano_ledger_transition_config;
    let epoch_size = bundle.shelley.epoch_length;
    let initial_state = build_initial_forge_state(bundle)?;
    let runtime_config = build_forge_runtime_config(bundle, protocol.cardano_protocol_version)?;
    let mut forgers = read_leader_credentials(
        credentials,
        bundle.shelley.slots_per_kes_period,
        bundle.shelley.max_kes_evolutions,
    )?;
    synthesize_with_forge_state(
        options,
        db_dir,
        epoch_size,
        initial_state,
        runtime_config,
        &mut forgers,
    )
}

/// Whether the supplied [`ForgeLimit`] would forge zero blocks.
///
/// A `--blocks 0` / `--slots 0` / `--epochs 0` invocation is a no-op
/// — the forge loop's `forging_done` predicate is satisfied
/// immediately. Surfaced for the dispatch layer's reporting.
pub fn is_noop_limit(limit: ForgeLimit) -> bool {
    matches!(
        limit,
        ForgeLimit::Block(0) | ForgeLimit::Slot(SlotNo(0)) | ForgeLimit::Epoch(0)
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::{DBSynthesizerOpenMode, DBSynthesizerOptions, ForgeLimit};

    fn opts(limit: ForgeLimit, mode: DBSynthesizerOpenMode) -> DBSynthesizerOptions {
        DBSynthesizerOptions {
            limit,
            open_mode: mode,
        }
    }

    #[test]
    fn read_leader_credentials_empty_when_nothing_supplied() {
        let creds = NodeCredentials::default();
        let forgers = read_leader_credentials(&creds, 129_600, 62).unwrap();
        assert!(
            forgers.is_empty(),
            "no credential files supplied — the forger set is empty"
        );
    }

    #[test]
    fn read_leader_credentials_rejects_partial_singleton() {
        // VRF + KES supplied, the operational certificate missing.
        let creds = NodeCredentials {
            cert_file: None,
            vrf_file: Some(PathBuf::from("/tmp/vrf.skey")),
            kes_file: Some(PathBuf::from("/tmp/kes.skey")),
            bulk_file: None,
        };
        let err = read_leader_credentials(&creds, 129_600, 62)
            .expect_err("a partial singleton credential set must be rejected");
        assert!(
            matches!(
                err,
                RunError::IncompleteCredentials { missing }
                    if missing == "operational certificate"
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn looks_like_chain_db_accepts_empty() {
        assert!(looks_like_chain_db(&BTreeSet::new()));
    }

    #[test]
    fn looks_like_chain_db_accepts_known_subset() {
        let mut s = BTreeSet::new();
        s.insert("immutable".to_string());
        s.insert("ledger".to_string());
        assert!(looks_like_chain_db(&s));
    }

    #[test]
    fn looks_like_chain_db_rejects_foreign_dir() {
        let mut s = BTreeSet::new();
        s.insert("immutable".to_string());
        s.insert("my-photos".to_string());
        assert!(!looks_like_chain_db(&s));
    }

    #[test]
    fn pre_open_creates_absent_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("fresh-db");
        assert!(!target.exists());
        pre_open_chain_db(DBSynthesizerOpenMode::OpenCreate, &target).unwrap();
        assert!(target.is_dir());
    }

    #[test]
    fn pre_open_create_rejects_existing_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let err =
            pre_open_chain_db(DBSynthesizerOpenMode::OpenCreate, tmp.path()).expect_err("rejects");
        assert!(matches!(err, RunError::AlreadyExists(_)));
    }

    #[test]
    fn pre_open_append_accepts_chain_db_shaped_directory() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("immutable")).unwrap();
        std::fs::create_dir(tmp.path().join("ledger")).unwrap();
        pre_open_chain_db(DBSynthesizerOpenMode::OpenAppend, tmp.path()).unwrap();
        // Untouched.
        assert!(tmp.path().join("immutable").is_dir());
    }

    #[test]
    fn pre_open_append_rejects_foreign_directory() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("not-a-chaindb")).unwrap();
        let err =
            pre_open_chain_db(DBSynthesizerOpenMode::OpenAppend, tmp.path()).expect_err("rejects");
        assert!(matches!(err, RunError::NotAChainDb(_)));
    }

    #[test]
    fn pre_open_force_wipes_chain_db_shaped_directory() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("immutable")).unwrap();
        std::fs::write(tmp.path().join("immutable").join("marker"), b"x").unwrap();
        pre_open_chain_db(DBSynthesizerOpenMode::OpenCreateForce, tmp.path()).unwrap();
        // Recreated empty.
        assert!(tmp.path().is_dir());
        assert!(!tmp.path().join("immutable").join("marker").exists());
    }

    #[test]
    fn pre_open_force_rejects_foreign_directory() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("photos")).unwrap();
        let err = pre_open_chain_db(DBSynthesizerOpenMode::OpenCreateForce, tmp.path())
            .expect_err("rejects");
        assert!(matches!(err, RunError::NotAChainDb(_)));
    }

    #[test]
    fn synthesize_default_creates_chain_db_with_n_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("synth-db");
        let outcome = synthesize_default(
            opts(ForgeLimit::Block(6), DBSynthesizerOpenMode::OpenCreate),
            &target,
        )
        .unwrap();
        assert_eq!(outcome.forge.result.forged, 6);
        assert_eq!(outcome.resumed_from, SlotNo(0));

        // Verification: reopen the ChainDB from disk and count.
        let reopened = FileImmutable::open(&target).unwrap();
        assert_eq!(reopened.len(), 6);
    }

    #[test]
    fn synthesize_append_resumes_from_tip() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("append-db");
        // First pass: create with 3 blocks.
        synthesize_default(
            opts(ForgeLimit::Block(3), DBSynthesizerOpenMode::OpenCreate),
            &target,
        )
        .unwrap();
        // Second pass: append 4 more.
        let outcome = synthesize_default(
            opts(ForgeLimit::Block(4), DBSynthesizerOpenMode::OpenAppend),
            &target,
        )
        .unwrap();
        assert_eq!(outcome.forge.result.forged, 4);
        // Resume slot is tip_slot + 1; first pass forged slots 0..2.
        assert_eq!(outcome.resumed_from, SlotNo(3));

        let reopened = FileImmutable::open(&target).unwrap();
        assert_eq!(reopened.len(), 7);
    }

    #[test]
    fn synthesize_force_overwrites_existing_chain_db() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("force-db");
        synthesize_default(
            opts(ForgeLimit::Block(10), DBSynthesizerOpenMode::OpenCreate),
            &target,
        )
        .unwrap();
        // Force-recreate with a smaller chain.
        let outcome = synthesize_default(
            opts(ForgeLimit::Block(2), DBSynthesizerOpenMode::OpenCreateForce),
            &target,
        )
        .unwrap();
        assert_eq!(outcome.forge.result.forged, 2);
        assert_eq!(outcome.resumed_from, SlotNo(0));

        let reopened = FileImmutable::open(&target).unwrap();
        assert_eq!(reopened.len(), 2);
    }

    #[test]
    fn is_noop_limit_detects_zero_limits() {
        assert!(is_noop_limit(ForgeLimit::Block(0)));
        assert!(is_noop_limit(ForgeLimit::Slot(SlotNo(0))));
        assert!(is_noop_limit(ForgeLimit::Epoch(0)));
        assert!(!is_noop_limit(ForgeLimit::Block(1)));
    }

    #[test]
    fn synthesize_zero_blocks_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("empty-db");
        let outcome = synthesize_default(
            opts(ForgeLimit::Block(0), DBSynthesizerOpenMode::OpenCreate),
            &target,
        )
        .unwrap();
        assert_eq!(outcome.forge.result.forged, 0);
        let reopened = FileImmutable::open(&target).unwrap();
        assert_eq!(reopened.len(), 0);
    }

    /// Minimal `AlonzoGenesis`-parseable JSON — `AlonzoGenesis` requires
    /// `executionPrices` / `maxTxExUnits` / `maxBlockExUnits` (no serde
    /// defaults), so an empty object does not parse.
    const MINIMAL_ALONZO_GENESIS: &str = r#"{"executionPrices":{"prMem":{"numerator":1,"denominator":1},"prSteps":{"numerator":1,"denominator":1}},"maxTxExUnits":{"exUnitsMem":1,"exUnitsSteps":1},"maxBlockExUnits":{"exUnitsMem":1,"exUnitsSteps":1}}"#;

    /// Write a `config.json` + every era's genesis into `dir`.
    /// `shelley_rel` is the (possibly nested) `ShelleyGenesisFile` path
    /// recorded in the config — relative paths exercise the
    /// config-directory resolution. The Byron / Alonzo / Conway genesis
    /// are minimal parseable fixtures. Returns the config path.
    fn write_config(dir: &Path, shelley_rel: &str, epoch_length: u64) -> std::path::PathBuf {
        let genesis = dir.join(shelley_rel);
        if let Some(parent) = genesis.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&genesis, format!(r#"{{"epochLength":{epoch_length}}}"#)).unwrap();
        // R3b-1: `synthesize_from_config` loads every era's genesis.
        std::fs::write(dir.join("byron.json"), "{}").unwrap();
        std::fs::write(dir.join("alonzo.json"), MINIMAL_ALONZO_GENESIS).unwrap();
        std::fs::write(dir.join("conway.json"), "{}").unwrap();
        let config = dir.join("config.json");
        let config_json = format!(
            r#"{{"Protocol":"Cardano","ByronGenesisFile":"byron.json","ShelleyGenesisFile":"{shelley_rel}","AlonzoGenesisFile":"alonzo.json","ConwayGenesisFile":"conway.json","LastKnownBlockVersion-Major":1,"LastKnownBlockVersion-Minor":0}}"#
        );
        std::fs::write(&config, config_json).unwrap();
        config
    }

    #[test]
    fn resolve_epoch_size_reads_non_default_epoch_length() {
        let tmp = tempfile::tempdir().unwrap();
        // 86_400 (the preview epoch length) is deliberately != the
        // 432_000 DEFAULT_EPOCH_SIZE, so a stubbed read would be caught.
        let config = write_config(tmp.path(), "shelley-genesis.json", 86_400);
        let epoch = resolve_epoch_size_from_config(&config).unwrap();
        assert_eq!(epoch, 86_400);
        assert_ne!(epoch, DEFAULT_EPOCH_SIZE);
    }

    #[test]
    fn resolve_epoch_size_resolves_genesis_path_relative_to_config_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // ShelleyGenesisFile sits in a sub-directory of the config dir;
        // upstream `relativeToConfig` resolves it there.
        let config = write_config(tmp.path(), "genesis/shelley.json", 21_600);
        assert_eq!(resolve_epoch_size_from_config(&config).unwrap(), 21_600);
    }

    #[test]
    fn resolve_epoch_size_errors_on_missing_config() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist.json");
        let err = resolve_epoch_size_from_config(&missing).expect_err("rejects");
        assert!(matches!(err, RunError::ConfigRead { .. }));
    }

    #[test]
    fn resolve_epoch_size_errors_on_missing_genesis() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config.json");
        std::fs::write(
            &config,
            r#"{"Protocol":"Cardano","ByronGenesisFile":"byron.json","ShelleyGenesisFile":"absent-shelley.json","AlonzoGenesisFile":"alonzo.json","ConwayGenesisFile":"conway.json"}"#,
        )
        .unwrap();
        let err = resolve_epoch_size_from_config(&config).expect_err("rejects");
        assert!(matches!(err, RunError::GenesisLoad(_)));
    }

    #[test]
    fn resolve_epoch_size_errors_on_non_cardano_protocol() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config.json");
        std::fs::write(&config, r#"{"Protocol":"Byron"}"#).unwrap();
        let err = resolve_epoch_size_from_config(&config).expect_err("rejects");
        assert!(matches!(err, RunError::ConfigStub(_)));
    }

    #[test]
    fn synthesize_from_config_without_forgers_leaves_chain_db_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), "shelley-genesis.json", 86_400);
        let target = tmp.path().join("synth-db");
        let outcome = synthesize_from_config(
            opts(ForgeLimit::Block(4), DBSynthesizerOpenMode::OpenCreate),
            &NodeCredentials::default(),
            &config,
            &target,
        )
        .unwrap();
        assert_eq!(outcome.forge.result.forged, 0);
        assert!(!outcome.chain_db_opened);
        assert!(
            !target.exists(),
            "upstream returns before opening the ChainDB when no forgers are available"
        );
    }

    #[test]
    fn load_genesis_bundle_loads_every_era_and_derives_nonce() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), "shelley-genesis.json", 86_400);

        let bundle = load_genesis_bundle(&config).expect("load genesis bundle");

        // The Shelley epoch length is the one written into the fixture.
        assert_eq!(bundle.shelley.epoch_length, 86_400);
        // Byron genesis `{}` has no nonAvvmBalances entries.
        assert!(bundle.byron.is_empty());
        // The initial Praos nonce is the Shelley genesis file hash —
        // a concrete hash, never the neutral nonce.
        assert!(
            matches!(bundle.praos_nonce, Nonce::Hash(_)),
            "praos nonce must be the Shelley genesis hash",
        );
    }

    #[test]
    fn load_genesis_bundle_errors_on_missing_era_genesis() {
        let tmp = tempfile::tempdir().unwrap();
        // A config whose Alonzo genesis file does not exist.
        std::fs::write(
            tmp.path().join("shelley-genesis.json"),
            r#"{"epochLength":432000}"#,
        )
        .unwrap();
        std::fs::write(tmp.path().join("byron.json"), "{}").unwrap();
        std::fs::write(tmp.path().join("conway.json"), "{}").unwrap();
        let config = tmp.path().join("config.json");
        std::fs::write(
            &config,
            r#"{"Protocol":"Cardano","ByronGenesisFile":"byron.json","ShelleyGenesisFile":"shelley-genesis.json","AlonzoGenesisFile":"absent-alonzo.json","ConwayGenesisFile":"conway.json"}"#,
        )
        .unwrap();
        let err = load_genesis_bundle(&config).expect_err("missing Alonzo genesis must fail");
        assert!(matches!(err, RunError::GenesisLoad(_)));
    }

    #[test]
    fn hard_fork_trigger_maps_test_epoch() {
        assert_eq!(
            HardForkTrigger::from_test_epoch(Some(42)),
            HardForkTrigger::AtEpoch(42),
        );
        assert_eq!(
            HardForkTrigger::from_test_epoch(None),
            HardForkTrigger::AtDefaultVersion,
        );
    }

    #[test]
    fn load_consensus_protocol_builds_cardano_protocol_params() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), "shelley-genesis.json", 86_400);

        let params = load_consensus_protocol(&config).expect("load consensus protocol");

        // Byron config — the required LastKnownBlockVersion keys parsed.
        assert_eq!(
            params
                .byron_protocol_params
                .byron_supported_protocol_version_major,
            1,
        );
        // The genesis bundle is threaded as the transition config.
        assert_eq!(
            params.cardano_ledger_transition_config.shelley.epoch_length,
            86_400,
        );
        // The Shelley-based initial nonce is the genesis hash.
        assert!(matches!(
            params.shelley_based_protocol_params.initial_nonce,
            Nonce::Hash(_),
        ));
        // No `Test*HardForkAtEpoch` overrides in the fixture config.
        assert_eq!(
            params.cardano_hard_fork_triggers.conway,
            HardForkTrigger::AtDefaultVersion,
        );
        // Development hard-fork eras off -> ProtVer 10.7.
        assert_eq!(params.cardano_protocol_version, (10, 7));
    }

    #[test]
    fn mk_consensus_protocol_cardano_maps_hard_fork_epochs_and_dev_version() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), "shelley-genesis.json", 432_000);
        let bundle = load_genesis_bundle(&config).expect("bundle");

        let byron: NodeByronProtocolConfiguration = serde_json::from_str(
            r#"{"ByronGenesisFile":"byron.json","LastKnownBlockVersion-Major":1,"LastKnownBlockVersion-Minor":0}"#,
        )
        .expect("byron config");
        let hard_fork: NodeHardForkProtocolConfiguration = serde_json::from_str(
            r#"{"TestEnableDevelopmentHardForkEras":true,"TestConwayHardForkAtEpoch":7}"#,
        )
        .expect("hard-fork config");

        let params = mk_consensus_protocol_cardano(byron, hard_fork, bundle);
        assert_eq!(
            params.cardano_hard_fork_triggers.conway,
            HardForkTrigger::AtEpoch(7),
        );
        assert_eq!(
            params.cardano_hard_fork_triggers.shelley,
            HardForkTrigger::AtDefaultVersion,
        );
        // Development hard-fork eras on -> ProtVer 11.0.
        assert_eq!(params.cardano_protocol_version, (11, 0));
    }

    #[test]
    fn load_initial_forge_state_builds_genesis_seeded_state() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), "shelley-genesis.json", 86_400);

        let forge_state = load_initial_forge_state(&config).expect("load initial forge state");

        // `build_base_ledger_state` roots the state at the Byron era
        // (Shelley genesis UTxO is staged for lazy materialization).
        assert_eq!(forge_state.ledger_state.current_era(), Era::Byron);
        // The nonce-evolution state is seeded from the genesis Praos
        // nonce — a concrete hash, never the neutral nonce.
        assert!(matches!(
            forge_state.nonce_evolution.epoch_nonce,
            Nonce::Hash(_),
        ));
    }
}
