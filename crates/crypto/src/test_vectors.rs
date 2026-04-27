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
        VrfPraosTestVector {
            name: "vrf-ver03-standard-12",
            secret_key: decode_hex_array(concat!(
                "c5aa8df43f9f837bedb7442f31dcb7b1",
                "66d38535076f094b85ce3a2e0b4458f7",
                "fc51cd8e6218a1a38da47ed00230f058",
                "0816ed13ba3303ac5deb911548908025",
            )),
            public_key: decode_hex_array(concat!(
                "fc51cd8e6218a1a38da47ed00230f058",
                "0816ed13ba3303ac5deb911548908025",
            )),
            message: decode_hex_vec("af82"),
            proof: decode_hex_array(concat!(
                "dfa2cba34b611cc8c833a6ea83b8eb1b",
                "b5e2ef2dd1b0c481bc42ff36ae7847f6",
                "ab52b976cfd5def172fa412defde270c",
                "8b8bdfbaae1c7ece17d9833b1bcf3106",
                "4fff78ef493f820055b561ece45e1009",
            )),
            output: decode_hex_array(concat!(
                "2031837f582cd17a9af9e0c7ef5a6540",
                "e3453ed894b62c293686ca3c1e319dde",
                "9d0aa489a4b59a9594fc2328bc3deff3",
                "c8a0929a369a72b1180a596e016b5ded",
            )),
        },
        VrfPraosTestVector {
            name: "vrf-ver03-generated-1",
            secret_key: decode_hex_array(concat!(
                "00000000000000000000000000000000",
                "00000000000000000000000000000000",
                "3b6a27bcceb6a42d62a3a8d02a6f0d73",
                "653215771de243a63ac048a18b59da29",
            )),
            public_key: decode_hex_array(concat!(
                "3b6a27bcceb6a42d62a3a8d02a6f0d73",
                "653215771de243a63ac048a18b59da29",
            )),
            message: decode_hex_vec("00"),
            proof: decode_hex_array(concat!(
                "000f006e64c91f84212919fe0899970c",
                "d341206fc081fe599339c8492e2cea32",
                "99ae9de4b6ce21cda0a975f65f45b70f",
                "82b3952ba6d0dbe11a06716e67aca233",
                "c0d78f115a655aa1952ada9f3d692a0a",
            )),
            output: decode_hex_array(concat!(
                "9930b5dddc0938f01cf6f9746eded569",
                "ee676bd6ff3b4f19233d74b903ec53a4",
                "5c5728116088b7c622b6d6c354f7125c",
                "7d09870b56ec6f1e4bf4970f607e04b2",
            )),
        },
        VrfPraosTestVector {
            name: "vrf-ver03-generated-2",
            secret_key: decode_hex_array(concat!(
                "00000000000000000000000000000000",
                "00000000000000000000000000000000",
                "3b6a27bcceb6a42d62a3a8d02a6f0d73",
                "653215771de243a63ac048a18b59da29",
            )),
            public_key: decode_hex_array(concat!(
                "3b6a27bcceb6a42d62a3a8d02a6f0d73",
                "653215771de243a63ac048a18b59da29",
            )),
            message: decode_hex_vec("00010203040506070809"),
            proof: decode_hex_array(concat!(
                "0031f929352875995e3d55c4abdac7bf",
                "b92e706beb182999dd7d78f61e1bdc3f",
                "83b746a9ae6caee317a7c47597ece180",
                "1799c06ca2180cdb5392677cd8815353",
                "c1d0d5691956b3be52b322be049fc20c",
            )),
            output: decode_hex_array(concat!(
                "ca4171883d173a3f03bdb87c45ce349f",
                "0bb168ca8171d64f9b9aeaf20d0869ba",
                "b9f74e819ccdc6754656468ccc2aa85e",
                "5f903a31375a39be84464fa515b51512",
            )),
        },
        VrfPraosTestVector {
            name: "vrf-ver03-generated-3",
            secret_key: decode_hex_array(concat!(
                "a70b8f607568df8ae26cf438b1057d8d",
                "0a94b7f3ac44cd984577fc43c2da55b7",
                "f1eb347d5c59e24f9f5f33c80cfd866e",
                "79fd72e0c370da3c011b1c9f045e23f1",
            )),
            public_key: decode_hex_array(concat!(
                "f1eb347d5c59e24f9f5f33c80cfd866e",
                "79fd72e0c370da3c011b1c9f045e23f1",
            )),
            message: decode_hex_vec("00"),
            proof: decode_hex_array(concat!(
                "aa349327d919c8c96de316855de6fe5f",
                "a841ef25af913cfb9b33d6b663c425bd",
                "024456ca193f10da319a2205c67222e8",
                "a62da87101904f453de0beb79568902c",
                "edeea891f3db8202690f51c8e7d3210b",
            )),
            output: decode_hex_array(concat!(
                "d4b4deef941fc3ece4e86f837c784951",
                "b4a0cbc4accd79cdcbc882123befeb17",
                "c63b329730c59bbe9253294496f73042",
                "8d588b9221832cb336bfd9d67754030f",
            )),
        },
        VrfPraosTestVector {
            name: "vrf-ver03-generated-4",
            secret_key: decode_hex_array(concat!(
                "a70b8f607568df8ae26cf438b1057d8d",
                "0a94b7f3ac44cd984577fc43c2da55b7",
                "f1eb347d5c59e24f9f5f33c80cfd866e",
                "79fd72e0c370da3c011b1c9f045e23f1",
            )),
            public_key: decode_hex_array(concat!(
                "f1eb347d5c59e24f9f5f33c80cfd866e",
                "79fd72e0c370da3c011b1c9f045e23f1",
            )),
            message: decode_hex_vec("00010203040506070809"),
            proof: decode_hex_array(concat!(
                "989c0c477b4a0c07e0dabd7b73cdb42b",
                "eb4b4e09471377e6d0b75e8ffd5d0917",
                "04394c5ea4e2be5d5244b02c03cf8598",
                "4adfa12c61280bc8c6e46f02035ee57d",
                "6cd18b96695ea04ff5ec541869ea890a",
            )),
            output: decode_hex_array(concat!(
                "933f886e8648796a968dccc71a3ce09a",
                "8026b28fdf5ffcc50be4b97431f3e390",
                "4375870b0bd196509dc33606846bb148",
                "20acdf36170e1667dbe9d3a940717bbd",
            )),
        },
    ]
}

/// Returns a small set of batch-compatible Praos VRF vectors mirrored from the
/// upstream `cardano-crypto-praos` fixtures.
pub fn vrf_praos_batchcompat_test_vectors() -> Vec<VrfPraosBatchCompatTestVector> {
    vec![
        VrfPraosBatchCompatTestVector {
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
        },
        VrfPraosBatchCompatTestVector {
            name: "vrf-ver13-standard-11",
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
                "47b327393ff2dd81336f8a2ef1033911",
                "2401253b3c714eeda879f12c509072ef",
                "8ec26e77b8cb3114dd2265fe1564a4ef",
                "b40d109aa3312536d93dfe3d8d80a061",
                "fe799eb5770b4e3a5a27d22518bb631d",
                "b183c8316bb552155f442c62a47d1c8b",
                "d60e93908f93df1623ad78a86a028d6b",
                "c064dbfc75a6a57379ef855dc6733801",
            )),
            output: decode_hex_array(concat!(
                "38561d6b77b71d30eb97a062168ae12b",
                "667ce5c28caccdf76bc88e093e463598",
                "7cd96814ce55b4689b3dd2947f80e59a",
                "ac7b7675f8083865b46c89b2ce9cc735",
            )),
        },
        VrfPraosBatchCompatTestVector {
            name: "vrf-ver13-standard-12",
            secret_key: decode_hex_array(concat!(
                "c5aa8df43f9f837bedb7442f31dcb7b1",
                "66d38535076f094b85ce3a2e0b4458f7",
                "fc51cd8e6218a1a38da47ed00230f058",
                "0816ed13ba3303ac5deb911548908025",
            )),
            public_key: decode_hex_array(concat!(
                "fc51cd8e6218a1a38da47ed00230f058",
                "0816ed13ba3303ac5deb911548908025",
            )),
            message: decode_hex_vec("af82"),
            proof: decode_hex_array(concat!(
                "926e895d308f5e328e7aa159c06eddbe",
                "56d06846abf5d98c2512235eaa57fdce",
                "a012f35433df219a88ab0f9481f4e006",
                "5d00422c3285f3d34a8b0202f20bac60",
                "fb613986d171b3e98319c7ca4dc44c5d",
                "d8314a6e5616c1a4f16ce72bd7a0c25a",
                "374e7ef73027e14760d42e77341fe054",
                "67bb286cc2c9d7fde29120a0b2320d04",
            )),
            output: decode_hex_array(concat!(
                "121b7f9b9aaaa29099fc04a94ba52784",
                "d44eac976dd1a3cca458733be5cd090a",
                "7b5fbd148444f17f8daf1fb55cb04b1a",
                "e85a626e30a54b4b0f8abf4a43314a58",
            )),
        },
        VrfPraosBatchCompatTestVector {
            name: "vrf-ver13-generated-1",
            secret_key: decode_hex_array(concat!(
                "00000000000000000000000000000000",
                "00000000000000000000000000000000",
                "3b6a27bcceb6a42d62a3a8d02a6f0d73",
                "653215771de243a63ac048a18b59da29",
            )),
            public_key: decode_hex_array(concat!(
                "3b6a27bcceb6a42d62a3a8d02a6f0d73",
                "653215771de243a63ac048a18b59da29",
            )),
            message: decode_hex_vec("00"),
            proof: decode_hex_array(concat!(
                "93d70c5ed59ccb21ca9991be56175693",
                "9ff9753bf85764d2a7b937d6fbf91834",
                "43cd118bee8a0f61e8bdc5403c03d6c9",
                "4ead31956e98bfd6a5e02d3be5900d17",
                "a540852d586f0891caed3e3b0e0871d6",
                "a741fb0edcdb586f7f10252f79c35176",
                "474ece4936e0190b5167832c10712884",
                "ad12acdfff2e434aacb165e1f789660f",
            )),
            output: decode_hex_array(concat!(
                "9a4d34f87003412e413ca42feba3b615",
                "8bdf11db41c2bbde98961c5865400cfd",
                "ee07149b928b376db365c5d68459378b",
                "0981f1cb0510f1e0c194c4a17603d44d",
            )),
        },
        VrfPraosBatchCompatTestVector {
            name: "vrf-ver13-generated-2",
            secret_key: decode_hex_array(concat!(
                "00000000000000000000000000000000",
                "00000000000000000000000000000000",
                "3b6a27bcceb6a42d62a3a8d02a6f0d73",
                "653215771de243a63ac048a18b59da29",
            )),
            public_key: decode_hex_array(concat!(
                "3b6a27bcceb6a42d62a3a8d02a6f0d73",
                "653215771de243a63ac048a18b59da29",
            )),
            message: decode_hex_vec("00010203040506070809"),
            proof: decode_hex_array(concat!(
                "235d7f05374c05e2ca22017575c572d7",
                "08b0fbd22c90d1ca5a94d0596b28a6cb",
                "d2e5de31550e43281ebe23b7b1393e16",
                "6b796a1193ff3cb41900082688a191a8",
                "ee8431e51c0a007a5860f8e72a9a1ed4",
                "aa1535d3161b462bf8a0bc54dae8df59",
                "20598aeb7752acfdfe56a158e754d9ee",
                "48e345aa65128348d0dc7953add5ad0a",
            )),
            output: decode_hex_array(concat!(
                "a8ad413d234680303a14203ca624cabe",
                "5f061798a7c248f687883993b1ac7cf8",
                "08868efcc47f5cf565bca51cb95cb7d8",
                "d18f2eb4c7ad3e648c369b477a7d45cd",
            )),
        },
        VrfPraosBatchCompatTestVector {
            name: "vrf-ver13-generated-3",
            secret_key: decode_hex_array(concat!(
                "a70b8f607568df8ae26cf438b1057d8d",
                "0a94b7f3ac44cd984577fc43c2da55b7",
                "f1eb347d5c59e24f9f5f33c80cfd866e",
                "79fd72e0c370da3c011b1c9f045e23f1",
            )),
            public_key: decode_hex_array(concat!(
                "f1eb347d5c59e24f9f5f33c80cfd866e",
                "79fd72e0c370da3c011b1c9f045e23f1",
            )),
            message: decode_hex_vec("00"),
            proof: decode_hex_array(concat!(
                "fe7fe305611dbd8402bf580ceaa4775b",
                "573a3be110bc30901880cfd81903852b",
                "306d432fc2d197b79a690ba8af62d166",
                "134ad57ec546b4675554207465e5d92d",
                "5570ba7336636f78afdf4ed2362c2205",
                "72c2735752b975773ec3289c803689cb",
                "fa9b8d841d2e603e3d9376c9c884a156",
                "c70cfd0a4293cc4edcd8902da8972f04",
            )),
            output: decode_hex_array(concat!(
                "05cff584ea083ae01537fc43a2456f70",
                "cbd0d1abc60b8f62170b83b647a00228",
                "40c27f747134e16641428d6cc6f66675",
                "b13fff7f975a5c6891172360417ac62d",
            )),
        },
        VrfPraosBatchCompatTestVector {
            name: "vrf-ver13-generated-4",
            secret_key: decode_hex_array(concat!(
                "a70b8f607568df8ae26cf438b1057d8d",
                "0a94b7f3ac44cd984577fc43c2da55b7",
                "f1eb347d5c59e24f9f5f33c80cfd866e",
                "79fd72e0c370da3c011b1c9f045e23f1",
            )),
            public_key: decode_hex_array(concat!(
                "f1eb347d5c59e24f9f5f33c80cfd866e",
                "79fd72e0c370da3c011b1c9f045e23f1",
            )),
            message: decode_hex_vec("00010203040506070809"),
            proof: decode_hex_array(concat!(
                "2ad402fec38563095e0a355fe5800848",
                "12d7728f613da256ddd01140c29d5ec9",
                "f76dcef18ef955bf74db970736e12b50",
                "968444fd7e69ebd15b83cbd27bb6cc27",
                "d49a39e8eb6c1242d9ccc9c0bab9eebb",
                "dd81eed1571316e2f9644fda6519e674",
                "0556a8d28c38ccddb23978d2e1c180af",
                "acea6e7fff589772ff10a1ea5cfc8700",
            )),
            output: decode_hex_array(concat!(
                "52f6d5f46c02df6231503b8ef6dbf870",
                "726235e41063e8698d69a72c17c05040",
                "e0cfe86215f4497747dff787a03470d2",
                "85d05f5a7c88d545e2e28baf2ceeaa2a",
            )),
        },
    ]
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

    bytes
        .try_into()
        .unwrap_or_else(|_| panic!("expected {N} bytes from hex input, got {actual}"))
}

fn decode_hex_vec(hex: &str) -> Vec<u8> {
    let compact: String = hex.chars().filter(|ch| !ch.is_whitespace()).collect();
    assert!(
        compact.len().is_multiple_of(2),
        "hex input must have even length"
    );

    compact
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).expect("hex data should be valid ASCII");
            u8::from_str_radix(pair, 16).expect("hex data should decode into bytes")
        })
        .collect()
}
