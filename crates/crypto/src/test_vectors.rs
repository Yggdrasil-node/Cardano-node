#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ed25519TestVector {
    pub name: &'static str,
    pub secret_key: [u8; 32],
    pub public_key: [u8; 32],
    pub message: Vec<u8>,
    pub signature: [u8; 64],
}

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