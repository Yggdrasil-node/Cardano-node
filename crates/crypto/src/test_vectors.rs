/// An Ed25519 compatibility vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ed25519TestVector {
    pub name: &'static str,
    pub secret_key: [u8; 32],
    pub public_key: [u8; 32],
    pub message: Vec<u8>,
    pub signature: [u8; 64],
}

/// A Praos VRF compatibility vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VrfPraosTestVector {
    pub name: &'static str,
    pub secret_key: [u8; 64],
    pub public_key: [u8; 32],
    pub message: Vec<u8>,
    pub proof: [u8; 80],
    pub output: [u8; 64],
}

/// A batch-compatible Praos VRF compatibility vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VrfPraosBatchCompatTestVector {
    pub name: &'static str,
    pub secret_key: [u8; 64],
    pub public_key: [u8; 32],
    pub message: Vec<u8>,
    pub proof: [u8; 128],
    pub output: [u8; 64],
}

/// A deterministic two-period SimpleKES compatibility vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimpleKesTwoPeriodTestVector {
    pub name: &'static str,
    pub seeds: [[u8; 32]; 2],
    pub verification_keys: [[u8; 32]; 2],
    pub period: u32,
    pub message: Vec<u8>,
    pub signature: [u8; 64],
    pub indexed_signature: [u8; 68],
    pub compact_indexed_signature: [u8; 100],
}

/// Returns a small set of published RFC 8032 Ed25519 vectors used as baseline
/// compatibility fixtures.
pub fn ed25519_rfc8032_vectors() -> Vec<Ed25519TestVector> {
    vec![
        Ed25519TestVector {
            name: "rfc8032-1",
            secret_key: decode_hex_array(concat!(
                "9d61b19deffd5a60ba844af492ec2cc4",
                "4449c5697b326919703bac031cae7f60",
            )),
            public_key: decode_hex_array(concat!(
                "d75a980182b10ab7d54bfed3c964073a",
                "0ee172f3daa62325af021a68f707511a",
            )),
            message: Vec::new(),
            signature: decode_hex_array(concat!(
                "e5564300c360ac729086e2cc806e828a",
                "84877f1eb8e5d974d873e06522490155",
                "5fb8821590a33bacc61e39701cf9b46b",
                "d25bf5f0595bbe24655141438e7a100b",
            )),
        },
        Ed25519TestVector {
            name: "rfc8032-2",
            secret_key: decode_hex_array(concat!(
                "4ccd089b28ff96da9db6c346ec114e0f",
                "5b8a319f35aba624da8cf6ed4fb8a6fb",
            )),
            public_key: decode_hex_array(concat!(
                "3d4017c3e843895a92b70aa74d1b7ebc",
                "9c982ccf2ec4968cc0cd55f12af4660c",
            )),
            message: decode_hex_vec("72"),
            signature: decode_hex_array(concat!(
                "92a009a9f0d4cab8720e820b5f642540",
                "a2b27b5416503f8fb3762223ebdb69da",
                "085ac1e43e15996e458f3613d0f11d8c",
                "387b2eaeb4302aeeb00d291612bb0c00",
            )),
        },
        Ed25519TestVector {
            name: "rfc8032-3",
            secret_key: decode_hex_array(concat!(
                "c5aa8df43f9f837bedb7442f31dcb7b1",
                "66d38535076f094b85ce3a2e0b4458f7",
            )),
            public_key: decode_hex_array(concat!(
                "fc51cd8e6218a1a38da47ed00230f058",
                "0816ed13ba3303ac5deb911548908025",
            )),
            message: decode_hex_vec("af82"),
            signature: decode_hex_array(concat!(
                "6291d657deec24024827e69c3abe01a3",
                "0ce548a284743a445e3680d7db5ac3ac",
                "18ff9b538d16f290ae67f760984dc659",
                "4a7c15e9716ed28dc027beceea1ec40a",
            )),
        },
    ]
}

/// Returns a small set of published Praos VRF vectors mirrored from the
/// upstream `cardano-crypto-praos` fixtures.
pub fn vrf_praos_test_vectors() -> Vec<VrfPraosTestVector> {
    vec![
        VrfPraosTestVector {
            name: "vrf-ver03-standard-10",
            secret_key: decode_hex_array(concat!(
                "9d61b19deffd5a60ba844af492ec2cc4",
                "4449c5697b326919703bac031cae7f60",
                "d75a980182b10ab7d54bfed3c964073a",
                "0ee172f3daa62325af021a68f707511a",
            )),
            public_key: decode_hex_array(concat!(
                "d75a980182b10ab7d54bfed3c964073a",
                "0ee172f3daa62325af021a68f707511a",
            )),
            message: Vec::new(),
            proof: decode_hex_array(concat!(
                "b6b4699f87d56126c9117a7da55bd008",
                "5246f4c56dbc95d20172612e9d38e8d7",
                "ca65e573a126ed88d4e30a46f80a6668",
                "54d675cf3ba81de0de043c3774f06156",
                "0f55edc256a787afe701677c0f602900",
            )),
            output: decode_hex_array(concat!(
                "5b49b554d05c0cd5a5325376b3387de5",
                "9d924fd1e13ded44648ab33c21349a60",
                "3f25b84ec5ed887995b33da5e3bfcb87",
                "cd2f64521c4c62cf825cffabbe5d31cc",
            )),
        },
        VrfPraosTestVector {
            name: "vrf-ver03-standard-11",
            secret_key: decode_hex_array(concat!(
                "4ccd089b28ff96da9db6c346ec114e0f",
                "5b8a319f35aba624da8cf6ed4fb8a6fb",
                "3d4017c3e843895a92b70aa74d1b7ebc",
                "9c982ccf2ec4968cc0cd55f12af4660c",
            )),
            public_key: decode_hex_array(concat!(
                "3d4017c3e843895a92b70aa74d1b7ebc",
                "9c982ccf2ec4968cc0cd55f12af4660c",
            )),
            message: decode_hex_vec("72"),
            proof: decode_hex_array(concat!(
                "ae5b66bdf04b4c010bfe32b2fc126ead",
                "2107b697634f6f7337b9bff8785ee111",
                "200095ece87dde4dbe87343f6df3b107",
                "d91798c8a7eb1245d3bb9c5aafb09335",
                "8c13e6ae1111a55717e895fd15f99f07",
            )),
            output: decode_hex_array(concat!(
                "94f4487e1b2fec954309ef1289ecb2e1",
                "5043a2461ecc7b2ae7d4470607ef82eb",
                "1cfa97d84991fe4a7bfdfd715606bc27",
                "e2967a6c557cfb5875879b671740b7d8",
            )),
        },
    ]
}

/// Returns a small set of batch-compatible Praos VRF vectors mirrored from the
/// upstream `cardano-crypto-praos` fixtures.
pub fn vrf_praos_batchcompat_test_vectors() -> Vec<VrfPraosBatchCompatTestVector> {
    vec![VrfPraosBatchCompatTestVector {
        name: "vrf-ver13-standard-10",
        secret_key: decode_hex_array(concat!(
            "9d61b19deffd5a60ba844af492ec2cc4",
            "4449c5697b326919703bac031cae7f60",
            "d75a980182b10ab7d54bfed3c964073a",
            "0ee172f3daa62325af021a68f707511a",
        )),
        public_key: decode_hex_array(concat!(
            "d75a980182b10ab7d54bfed3c964073a",
            "0ee172f3daa62325af021a68f707511a",
        )),
        message: Vec::new(),
        proof: decode_hex_array(concat!(
            "7d9c633ffeee27349264cf5c667579fc",
            "583b4bda63ab71d001f89c10003ab46f",
            "762f5c178b68f0cddcc1157918edf45e",
            "c334ac8e8286601a3256c3bbf858edd9",
            "4652eba1c4612e6fce762977a59420b4",
            "51e12964adbe4fbecd58a7aeff5860af",
            "cafa73589b023d14311c331a9ad15ff2",
            "fb37831e00f0acaa6d73bc9997b06501",
        )),
        output: decode_hex_array(concat!(
            "9d574bf9b8302ec0fc1e21c3ec536826",
            "9527b87b462ce36dab2d14ccf80c53cc",
            "cf6758f058c5b1c856b116388152bbe5",
            "09ee3b9ecfe63d93c3b4346c1fbc6c54",
        )),
    }]
}

/// Returns deterministic two-period SimpleKES fixtures derived from RFC 8032
/// Ed25519 vectors.
pub fn simple_kes_two_period_test_vectors() -> Vec<SimpleKesTwoPeriodTestVector> {
    vec![SimpleKesTwoPeriodTestVector {
        name: "simple-kes-2p-rfc8032-period1-msg72",
        seeds: [
            decode_hex_array(concat!(
                "9d61b19deffd5a60ba844af492ec2cc4",
                "4449c5697b326919703bac031cae7f60",
            )),
            decode_hex_array(concat!(
                "4ccd089b28ff96da9db6c346ec114e0f",
                "5b8a319f35aba624da8cf6ed4fb8a6fb",
            )),
        ],
        verification_keys: [
            decode_hex_array(concat!(
                "d75a980182b10ab7d54bfed3c964073a",
                "0ee172f3daa62325af021a68f707511a",
            )),
            decode_hex_array(concat!(
                "3d4017c3e843895a92b70aa74d1b7ebc",
                "9c982ccf2ec4968cc0cd55f12af4660c",
            )),
        ],
        period: 1,
        message: decode_hex_vec("72"),
        signature: decode_hex_array(concat!(
            "92a009a9f0d4cab8720e820b5f642540",
            "a2b27b5416503f8fb3762223ebdb69da",
            "085ac1e43e15996e458f3613d0f11d8c",
            "387b2eaeb4302aeeb00d291612bb0c00",
        )),
        indexed_signature: decode_hex_array(concat!(
            "00000001",
            "92a009a9f0d4cab8720e820b5f642540",
            "a2b27b5416503f8fb3762223ebdb69da",
            "085ac1e43e15996e458f3613d0f11d8c",
            "387b2eaeb4302aeeb00d291612bb0c00",
        )),
        compact_indexed_signature: decode_hex_array(concat!(
            "00000001",
            "92a009a9f0d4cab8720e820b5f642540",
            "a2b27b5416503f8fb3762223ebdb69da",
            "085ac1e43e15996e458f3613d0f11d8c",
            "387b2eaeb4302aeeb00d291612bb0c00",
            "3d4017c3e843895a92b70aa74d1b7ebc",
            "9c982ccf2ec4968cc0cd55f12af4660c",
        )),
    }]
}

fn decode_hex_array<const N: usize>(hex: &str) -> [u8; N] {
    let bytes = decode_hex_vec(hex);
    let actual = bytes.len();

    bytes.try_into().unwrap_or_else(|_| {
        panic!("expected {N} bytes from hex input, got {actual}")
    })
}

fn decode_hex_vec(hex: &str) -> Vec<u8> {
    let compact: String = hex.chars().filter(|ch| !ch.is_whitespace()).collect();
    assert!(compact.len() % 2 == 0, "hex input must have even length");

    compact
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).expect("hex data should be valid ASCII");
            u8::from_str_radix(pair, 16).expect("hex data should decode into bytes")
        })
        .collect()
}