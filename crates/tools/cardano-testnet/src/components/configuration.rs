//! cardano-testnet configuration-creation constants.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side component placement for
//! upstream `cardano-testnet/src/Testnet/Components/Configuration.hs`.
//! This slice ports `createConfigJson`, `createConfigJsonNoHash`, the
//! genesis-hash helpers, the start-time / UTxO constants, and the
//! era-string helpers. The remaining `createSPOGenesisAndFiles`,
//! `getDefaultShelleyGenesis`, `getDefaultAlonzoGenesis`, and
//! Dijkstra genesis construction surface is IO- or ledger-genesis-
//! coupled and lands once those crate-boundary types are exposed.

use serde_json::{Map, Value};

use crate::defaults;
use crate::filepath::TmpAbsolutePath;
use crate::types::{CardanoEra, ShelleyBasedEra};

fn join_temp_file(temp_abs_path: &TmpAbsolutePath, file_name: &str) -> String {
    format!(
        "{}/{}",
        temp_abs_path.as_str().trim_end_matches('/'),
        file_name
    )
}

fn singleton_hash(key: &str, hash: [u8; 32]) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(key.to_string(), Value::String(hex::encode(hash)));
    map
}

fn insert_all(target: &mut Map<String, Value>, source: Map<String, Value>) {
    for (key, value) in source {
        target.insert(key, value);
    }
}

/// Build the generated node configuration with genesis hashes.
///
/// Mirror of upstream `createConfigJson`: compute the Byron genesis
/// hash from canonical JSON, compute Shelley-family genesis hashes
/// from raw file bytes, then merge those hash fields with
/// `defaultYamlHardforkViaConfig`.
pub fn create_config_json(
    temp_abs_path: &TmpAbsolutePath,
    era: ShelleyBasedEra,
) -> eyre::Result<Map<String, Value>> {
    let mut config = Map::new();
    insert_all(
        &mut config,
        get_byron_genesis_hash(join_temp_file(temp_abs_path, "byron-genesis.json"))?,
    );
    for (era, key) in [
        (CardanoEra::Shelley, "ShelleyGenesisHash"),
        (CardanoEra::Alonzo, "AlonzoGenesisHash"),
        (CardanoEra::Conway, "ConwayGenesisHash"),
        (CardanoEra::Dijkstra, "DijkstraGenesisHash"),
    ] {
        let file_name = defaults::default_genesis_filepath(era);
        insert_all(
            &mut config,
            get_shelley_genesis_hash(join_temp_file(temp_abs_path, &file_name), key)?,
        );
    }
    insert_all(&mut config, defaults::default_yaml_hardfork_via_config(era));
    Ok(config)
}

/// Compute the upstream Byron genesis hash JSON field.
///
/// Mirror of upstream `getByronGenesisHash`, which delegates to
/// `readGenesisData` and serializes the resulting `GenesisHash`.
pub fn get_byron_genesis_hash(
    path: impl AsRef<std::path::Path>,
) -> eyre::Result<Map<String, Value>> {
    let hash = yggdrasil_node_genesis::compute_byron_genesis_file_hash(path.as_ref())?;
    Ok(singleton_hash("ByronGenesisHash", hash))
}

/// Compute one Shelley-family genesis hash JSON field.
///
/// Mirror of upstream `getShelleyGenesisHash`: read the exact file
/// bytes, hash them with Blake2b-256, and return a singleton object
/// under the caller-supplied key.
pub fn get_shelley_genesis_hash(
    path: impl AsRef<std::path::Path>,
    key: &str,
) -> eyre::Result<Map<String, Value>> {
    let hash = yggdrasil_node_genesis::compute_genesis_file_hash(path.as_ref())?;
    Ok(singleton_hash(key, hash))
}

/// Seconds added to "now" when computing a fresh testnet's genesis
/// start time.
///
/// Mirror of upstream
/// `startTimeOffsetSeconds = if OS.isWin32 then 90 else 15` — CLI
/// commands are markedly slower on Windows, so testnet setup is given
/// more headroom there.
pub const START_TIME_OFFSET_SECONDS: i32 = if cfg!(windows) { 90 } else { 15 };

/// The number of UTxO keys a freshly-created testnet seeds.
///
/// Mirror of upstream `numSeededUTxOKeys = 3`.
pub const NUM_SEEDED_UTXO_KEYS: i32 = 3;

/// Build the generated node configuration without genesis hashes.
///
/// Mirror of upstream `createConfigJsonNoHash =
/// Defaults.defaultYamlHardforkViaConfig`.
pub fn create_config_json_no_hash(era: ShelleyBasedEra) -> Map<String, Value> {
    defaults::default_yaml_hardfork_via_config(era)
}

/// Lower-case Shelley-based era name used in generated file names and
/// cardano-cli subcommands.
///
/// Mirror of upstream `eraToString`.
pub fn era_to_string(era: ShelleyBasedEra) -> &'static str {
    era.era_to_string()
}

/// Lower-case Cardano era name used in generated file names.
///
/// Mirror of upstream `anyEraToString`.
pub fn any_era_to_string(era: CardanoEra) -> &'static str {
    era.era_to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn start_time_offset_matches_upstream_for_this_platform() {
        let expected = if cfg!(windows) { 90 } else { 15 };
        assert_eq!(START_TIME_OFFSET_SECONDS, expected);
    }

    #[test]
    fn num_seeded_utxo_keys_matches_upstream() {
        assert_eq!(NUM_SEEDED_UTXO_KEYS, 3);
    }

    #[test]
    fn create_config_json_no_hash_delegates_to_hardfork_config() {
        let config = create_config_json_no_hash(ShelleyBasedEra::Conway);
        let expected = defaults::default_yaml_hardfork_via_config(ShelleyBasedEra::Conway);

        assert_eq!(config, expected);
        assert_eq!(config["Protocol"], json!("Cardano"));
        assert_eq!(config["ExperimentalProtocolsEnabled"], json!(true));
        assert_eq!(config["TestConwayHardForkAtEpoch"], json!(0));
        assert!(
            !config.contains_key("ShelleyGenesisHash"),
            "no-hash config must not inject genesis hashes"
        );
    }

    #[test]
    fn era_string_helpers_match_upstream_lowercase_names() {
        assert_eq!(era_to_string(ShelleyBasedEra::Shelley), "shelley");
        assert_eq!(era_to_string(ShelleyBasedEra::Conway), "conway");

        assert_eq!(any_era_to_string(CardanoEra::Byron), "byron");
        assert_eq!(any_era_to_string(CardanoEra::Alonzo), "alonzo");
        assert_eq!(any_era_to_string(CardanoEra::Conway), "conway");
        assert_eq!(any_era_to_string(CardanoEra::Dijkstra), "dijkstra");
    }

    #[test]
    fn shelley_genesis_hash_reads_exact_raw_file_bytes() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "yggdrasil-cardano-testnet-shelley-genesis-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("configuration")
        ));
        let bytes = b"{\n  \"systemStart\": \"2026-06-03T00:00:00Z\"\n}\n";
        std::fs::write(&path, bytes).expect("write genesis fixture");

        let actual = get_shelley_genesis_hash(&path, "ShelleyGenesisHash").expect("hash fixture");
        let expected_hash = yggdrasil_node_genesis::compute_genesis_file_hash(&path)
            .expect("compute expected hash");
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            actual["ShelleyGenesisHash"],
            Value::String(hex::encode(expected_hash))
        );
    }

    #[test]
    fn create_config_json_merges_hashes_with_hardfork_config() {
        let base = std::env::temp_dir().join(format!(
            "yggdrasil-cardano-testnet-config-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("configuration")
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("create temp dir");
        let write = |name: &str, bytes: &[u8]| {
            std::fs::write(base.join(name), bytes).expect("write genesis fixture")
        };
        write("byron-genesis.json", br#"{"protocolConsts":{"k":2160}}"#);
        write("shelley-genesis.json", br#"{"era":"shelley"}"#);
        write("alonzo-genesis.json", br#"{"era":"alonzo"}"#);
        write("conway-genesis.json", br#"{"era":"conway"}"#);
        write("dijkstra-genesis.json", br#"{"era":"dijkstra"}"#);

        let tmp = TmpAbsolutePath(base.to_string_lossy().into_owned());
        let actual = create_config_json(&tmp, ShelleyBasedEra::Conway).expect("create config");

        for (file, key, byron) in [
            ("byron-genesis.json", "ByronGenesisHash", true),
            ("shelley-genesis.json", "ShelleyGenesisHash", false),
            ("alonzo-genesis.json", "AlonzoGenesisHash", false),
            ("conway-genesis.json", "ConwayGenesisHash", false),
            ("dijkstra-genesis.json", "DijkstraGenesisHash", false),
        ] {
            let path = base.join(file);
            let expected = if byron {
                yggdrasil_node_genesis::compute_byron_genesis_file_hash(&path).expect("byron hash")
            } else {
                yggdrasil_node_genesis::compute_genesis_file_hash(&path)
                    .expect("shelley-family hash")
            };
            assert_eq!(actual[key], Value::String(hex::encode(expected)));
        }
        assert_eq!(actual["Protocol"], Value::String("Cardano".to_string()));
        assert_eq!(actual["TestConwayHardForkAtEpoch"], serde_json::json!(0));
        let _ = std::fs::remove_dir_all(&base);
    }
}
