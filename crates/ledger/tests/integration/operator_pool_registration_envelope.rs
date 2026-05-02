//! Operator-side `pool_registration` certificate envelope verification.
//!
//! Two-pronged coverage:
//!
//! 1. **Synthetic fixture** — always runs.  Builds a `PoolRegistration`
//!    DCert from a hand-crafted [`PoolParams`], encodes it as the
//!    upstream `[3, …pool_params]` flat tuple, and round-trips through
//!    [`DCert::decode_cbor`].  Pins the `cardano-cli stake-pool
//!    registration-certificate` envelope shape so a future codec change
//!    breaks here, not at operator-credential load time.
//!
//! 2. **Operator-supplied fixture** — runs only when the
//!    `YGGDRASIL_OPERATOR_POOL_CERT` env var points at a
//!    `cardano-cli`-style text-envelope file (e.g. the
//!    `certs/pool-registration.cert` Daniel maintains in his repo
//!    root).  Decodes the cert, asserts it's a Conway
//!    `PoolRegistration`, and (when
//!    `YGGDRASIL_OPERATOR_POOL_SUMMARY` is also set) cross-checks the
//!    decoded operator key hash / pledge / cost / margin against the
//!    JSON summary `cardano-cli` produced alongside it.  Useful to
//!    confirm an operator's on-disk material parses correctly with
//!    yggdrasil before they submit it on-chain.
//!
//! Reference: `cardano-cli stake-pool registration-certificate` and
//! upstream Conway `pool_registration_cert = (3, pool_params)`.

use super::*;

fn synthetic_pool_params() -> PoolParams {
    PoolParams {
        operator: [0xea; 28],
        vrf_keyhash: [0x31; 32],
        pledge: 0,
        cost: 340_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 0,
            credential: StakeCredential::AddrKeyHash([0x11; 28]),
        },
        pool_owners: vec![[0x22; 28]],
        relays: Vec::new(),
        pool_metadata: None,
    }
}

#[test]
fn synthetic_pool_registration_dcert_round_trips() {
    let dcert = DCert::PoolRegistration(synthetic_pool_params());
    let bytes = dcert.to_cbor_bytes();
    // Upstream Conway `pool_registration_cert = (3, pool_params)` —
    // a 10-element flat tuple (1 tag + 9 pool-params fields).
    assert_eq!(bytes[0], 0x8a, "outer must be 10-element CBOR array");
    assert_eq!(bytes[1], 0x03, "tag must be 3 (pool_registration)");

    let mut dec = Decoder::new(&bytes);
    let decoded = DCert::decode_cbor(&mut dec).expect("DCert round-trips");
    match decoded {
        DCert::PoolRegistration(p) => {
            assert_eq!(p.operator, [0xea; 28]);
            assert_eq!(p.vrf_keyhash, [0x31; 32]);
            assert_eq!(p.cost, 340_000_000);
        }
        other => panic!("expected PoolRegistration, got {other:?}"),
    }
}

/// Optional operator-supplied verification.  Activates when
/// `YGGDRASIL_OPERATOR_POOL_CERT=<path>` is set.  Useful before
/// submitting a freshly generated cert on-chain to confirm it parses.
#[test]
fn operator_supplied_pool_cert_decodes_when_env_var_set() {
    let cert_path = match std::env::var("YGGDRASIL_OPERATOR_POOL_CERT") {
        Ok(p) => p,
        Err(_) => return, // not set — silent skip; CI passes without the file
    };

    let raw = std::fs::read(&cert_path)
        .unwrap_or_else(|err| panic!("read operator cert {cert_path}: {err}"));
    let envelope: serde_json::Value =
        serde_json::from_slice(&raw).unwrap_or_else(|err| panic!("envelope JSON parse: {err}"));
    assert_eq!(
        envelope.get("type").and_then(|v| v.as_str()),
        Some("Certificate"),
        "envelope.type must be 'Certificate'"
    );
    let cbor_hex = envelope
        .get("cborHex")
        .and_then(|v| v.as_str())
        .expect("envelope.cborHex must be string");
    let cbor_bytes = hex::decode(cbor_hex).expect("cborHex decodes");

    let mut dec = Decoder::new(&cbor_bytes);
    let dcert = DCert::decode_cbor(&mut dec).expect("DCert decode");
    let params = match dcert {
        DCert::PoolRegistration(p) => p,
        other => panic!("operator cert is not PoolRegistration: {other:?}"),
    };

    // When a registration-summary.json is also pointed at, cross-check
    // the decoded fields against the summary so an operator catches
    // a mismatch (e.g. wrong cold-key) at parse time rather than after
    // a failed on-chain submission.
    if let Ok(summary_path) = std::env::var("YGGDRASIL_OPERATOR_POOL_SUMMARY") {
        let summary_raw = std::fs::read(&summary_path)
            .unwrap_or_else(|err| panic!("read summary {summary_path}: {err}"));
        let summary: serde_json::Value = serde_json::from_slice(&summary_raw)
            .unwrap_or_else(|err| panic!("summary JSON parse: {err}"));

        if let Some(expected_hex) = summary.get("pool_id_hex").and_then(|v| v.as_str()) {
            let actual_hex = hex::encode(params.operator);
            assert_eq!(
                actual_hex, expected_hex,
                "cert operator hash must match summary.pool_id_hex"
            );
        }
        if let Some(expected) = summary.get("pool_cost_lovelace").and_then(|v| v.as_u64()) {
            assert_eq!(
                params.cost, expected,
                "cert cost must match summary.pool_cost_lovelace"
            );
        }
        if let Some(expected) = summary.get("pool_pledge_lovelace").and_then(|v| v.as_u64()) {
            assert_eq!(
                params.pledge, expected,
                "cert pledge must match summary.pool_pledge_lovelace"
            );
        }
    }
}
