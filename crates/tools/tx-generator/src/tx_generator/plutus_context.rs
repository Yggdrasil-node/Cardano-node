//! Plutus budgeting and script-data helpers.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/PlutusContext.hs`.
//! Ports the script-data loading and `scriptDataModifyNumber` helpers
//! consumed by `Script/Core.makePlutusContext`. Auto-budget fitting is
//! left on an explicit `plutusAutoScaleBlockfit` boundary.

use std::path::Path;

use num_bigint::BigInt;
use serde_json::Value;
use yggdrasil_ledger::PlutusData;

/// Mirror of upstream `readScriptData`.
pub fn read_script_data(path: &Path) -> Result<PlutusData, String> {
    if path.as_os_str().is_empty() {
        return Ok(PlutusData::integer(0));
    }

    let raw = std::fs::read_to_string(path)
        .map_err(|err| format!("readScriptData: {}: {err}", path.display()))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|err| format!("readScriptData: {}: {err}", path.display()))?;
    script_data_from_json_detailed_schema(&value)
}

/// Parse Cardano API's `ScriptDataJsonDetailedSchema` representation.
pub fn script_data_from_json_detailed_schema(value: &Value) -> Result<PlutusData, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "ScriptDataJsonDetailedSchema: expected object".to_string())?;

    if let Some(int) = object.get("int") {
        return parse_script_data_integer(int).map(PlutusData::integer);
    }
    if let Some(bytes) = object.get("bytes") {
        let hex = bytes
            .as_str()
            .ok_or_else(|| "ScriptDataJsonDetailedSchema.bytes: expected string".to_string())?;
        return hex::decode(hex)
            .map(PlutusData::Bytes)
            .map_err(|err| format!("ScriptDataJsonDetailedSchema.bytes: invalid hex: {err}"));
    }
    if let Some(list) = object.get("list") {
        let values = list
            .as_array()
            .ok_or_else(|| "ScriptDataJsonDetailedSchema.list: expected array".to_string())?
            .iter()
            .map(script_data_from_json_detailed_schema)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(PlutusData::List(values));
    }
    if let Some(map) = object.get("map") {
        let entries = map
            .as_array()
            .ok_or_else(|| "ScriptDataJsonDetailedSchema.map: expected array".to_string())?
            .iter()
            .map(|entry| {
                let entry = entry.as_object().ok_or_else(|| {
                    "ScriptDataJsonDetailedSchema.map: expected object entry".to_string()
                })?;
                let key = entry
                    .get("k")
                    .ok_or_else(|| "ScriptDataJsonDetailedSchema.map: missing k".to_string())?;
                let value = entry
                    .get("v")
                    .ok_or_else(|| "ScriptDataJsonDetailedSchema.map: missing v".to_string())?;
                Ok((
                    script_data_from_json_detailed_schema(key)?,
                    script_data_from_json_detailed_schema(value)?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        return Ok(PlutusData::Map(entries));
    }

    if let Some(constructor) = object.get("constructor") {
        let alt = parse_u64_field(constructor, "ScriptDataJsonDetailedSchema.constructor")?;
        let fields = object
            .get("fields")
            .ok_or_else(|| "ScriptDataJsonDetailedSchema.constructor: missing fields".to_string())?
            .as_array()
            .ok_or_else(|| {
                "ScriptDataJsonDetailedSchema.constructor.fields: expected array".to_string()
            })?
            .iter()
            .map(script_data_from_json_detailed_schema)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(PlutusData::Constr(alt, fields));
    }

    Err(
        "ScriptDataJsonDetailedSchema: expected one of int, bytes, list, map, constructor"
            .to_string(),
    )
}

/// Mirror of upstream `scriptDataModifyNumber`.
pub fn script_data_modify_number(data: &PlutusData, f: impl Fn(&BigInt) -> BigInt) -> PlutusData {
    fn go(data: &PlutusData, f: &dyn Fn(&BigInt) -> BigInt) -> PlutusData {
        match data {
            PlutusData::Integer(value) => PlutusData::Integer(f(value)),
            PlutusData::Constr(alt, fields) => PlutusData::Constr(*alt, go_list(fields, f)),
            PlutusData::List(values) => PlutusData::List(go_list(values, f)),
            PlutusData::Map(entries) => {
                let values = entries
                    .iter()
                    .map(|(_key, value)| value.clone())
                    .collect::<Vec<_>>();
                let changed_values = go_list(&values, f);
                PlutusData::Map(
                    entries
                        .iter()
                        .zip(changed_values)
                        .map(|((key, _), value)| (key.clone(), value))
                        .collect(),
                )
            }
            PlutusData::Bytes(bytes) => PlutusData::Bytes(bytes.clone()),
        }
    }

    fn go_list(values: &[PlutusData], f: &dyn Fn(&BigInt) -> BigInt) -> Vec<PlutusData> {
        let mut out = Vec::with_capacity(values.len());
        for (idx, value) in values.iter().enumerate() {
            let changed = go(value, f);
            if changed == *value {
                out.push(value.clone());
            } else {
                out.push(changed);
                out.extend_from_slice(&values[idx + 1..]);
                return out;
            }
        }
        out
    }

    go(data, &f)
}

/// Boundary for upstream `plutusAutoScaleBlockfit`.
pub fn plutus_auto_scale_blockfit() -> Result<(), String> {
    Err("plutusAutoScaleBlockfit: auto-budget fitting is not yet implemented".to_string())
}

fn parse_script_data_integer(value: &Value) -> Result<BigInt, String> {
    match value {
        Value::Number(number) => {
            if let Some(n) = number.as_i64() {
                Ok(BigInt::from(n))
            } else if let Some(n) = number.as_u64() {
                Ok(BigInt::from(n))
            } else {
                Err("ScriptDataJsonDetailedSchema.int: floating values are not valid".to_string())
            }
        }
        Value::String(text) => text
            .parse::<BigInt>()
            .map_err(|err| format!("ScriptDataJsonDetailedSchema.int: invalid integer: {err}")),
        _ => {
            Err("ScriptDataJsonDetailedSchema.int: expected integer or decimal string".to_string())
        }
    }
}

fn parse_u64_field(value: &Value, field: &str) -> Result<u64, String> {
    value
        .as_u64()
        .ok_or_else(|| format!("{field}: expected unsigned integer"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_script_data_path_is_integer_zero() {
        assert_eq!(
            read_script_data(Path::new("")).expect("empty value"),
            PlutusData::integer(0)
        );
    }

    #[test]
    fn detailed_schema_parses_constructor_map_list_bytes_and_int() {
        let value = json!({
            "constructor": 1,
            "fields": [
                {"int": "18446744073709551616"},
                {"bytes": "aabb"},
                {"list": [{"int": -1}]},
                {"map": [{"k": {"bytes": "00"}, "v": {"int": 7}}]}
            ]
        });

        assert_eq!(
            script_data_from_json_detailed_schema(&value).expect("script data"),
            PlutusData::Constr(
                1,
                vec![
                    PlutusData::integer("18446744073709551616".parse::<BigInt>().expect("big int")),
                    PlutusData::Bytes(vec![0xaa, 0xbb]),
                    PlutusData::List(vec![PlutusData::integer(-1)]),
                    PlutusData::Map(vec![(PlutusData::Bytes(vec![0]), PlutusData::integer(7))]),
                ],
            )
        );
    }

    #[test]
    fn script_data_modify_number_updates_first_changed_value_only() {
        let data = PlutusData::Map(vec![
            (PlutusData::integer(1), PlutusData::Bytes(vec![1])),
            (
                PlutusData::integer(2),
                PlutusData::List(vec![PlutusData::integer(3)]),
            ),
            (PlutusData::integer(4), PlutusData::integer(5)),
        ]);

        let changed = script_data_modify_number(&data, |n| n + 10);

        assert_eq!(
            changed,
            PlutusData::Map(vec![
                (PlutusData::integer(1), PlutusData::Bytes(vec![1])),
                (
                    PlutusData::integer(2),
                    PlutusData::List(vec![PlutusData::integer(13)])
                ),
                (PlutusData::integer(4), PlutusData::integer(5)),
            ])
        );
    }
}
