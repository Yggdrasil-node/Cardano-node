//! Common protocol types used by control and service protocols.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/Types.hs.
//!
//! This module carries the pure enum/record vocabulary from upstream
//! `Cardano.KESAgent.Protocols.Types`. Concrete CBOR codecs and
//! socket drivers land in the daemon/socket follow-on.

use std::fmt;

use super::recv_result::RecvResult;

/// Timestamp used by upstream tagged key-bundle traces.
pub type Timestamp = u64;

/// Version identifier exchanged by versioned protocols. Mirrors
/// upstream `VersionIdentifier`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct VersionIdentifier(String);

impl VersionIdentifier {
    /// Construct a version identifier from the upstream textual tag.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the upstream textual tag.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VersionIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Idiomatic Rust casing for upstream `mkVersionIdentifier`.
pub fn mk_version_identifier(value: impl Into<String>) -> VersionIdentifier {
    VersionIdentifier::new(value)
}

/// Protocol command sent by the control client. Mirrors upstream `Command`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
#[repr(u8)]
pub enum Command {
    /// `GenStagedKeyCmd`.
    GenStagedKeyCmd = 0,
    /// `QueryStagedKeyCmd`.
    QueryStagedKeyCmd = 1,
    /// `DropStagedKeyCmd`.
    DropStagedKeyCmd = 2,
    /// `InstallKeyCmd`.
    InstallKeyCmd = 3,
    /// `RequestInfoCmd`.
    RequestInfoCmd = 4,
    /// `DropKeyCmd`.
    DropKeyCmd = 5,
}

impl Command {
    /// Discriminants in upstream declaration order.
    pub const ALL: [Self; 6] = [
        Self::GenStagedKeyCmd,
        Self::QueryStagedKeyCmd,
        Self::DropStagedKeyCmd,
        Self::InstallKeyCmd,
        Self::RequestInfoCmd,
        Self::DropKeyCmd,
    ];

    /// Upstream enum ordinal used by `ViaEnum Command`.
    pub const fn ordinal(self) -> u8 {
        self as u8
    }

    /// Decode an upstream enum ordinal.
    pub const fn from_ordinal(ordinal: u8) -> Option<Self> {
        match ordinal {
            0 => Some(Self::GenStagedKeyCmd),
            1 => Some(Self::QueryStagedKeyCmd),
            2 => Some(Self::DropStagedKeyCmd),
            3 => Some(Self::InstallKeyCmd),
            4 => Some(Self::RequestInfoCmd),
            5 => Some(Self::DropKeyCmd),
            _ => None,
        }
    }

    /// Mirror of upstream `Pretty Command`.
    pub const fn pretty(self) -> &'static str {
        match self {
            Self::GenStagedKeyCmd => "gen staged key",
            Self::QueryStagedKeyCmd => "query staged key",
            Self::DropStagedKeyCmd => "drop staged key",
            Self::InstallKeyCmd => "install key",
            Self::RequestInfoCmd => "request info",
            Self::DropKeyCmd => "drop key",
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.pretty())
    }
}

/// Representation of a key bundle in trace logs. Mirrors upstream
/// `BundleTrace`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BundleTrace {
    /// `keyIdentSerial`.
    pub key_ident_serial: u64,
    /// `keyIdentVKHex`.
    pub key_ident_vk_hex: Vec<u8>,
}

impl BundleTrace {
    /// Mirror of upstream `Pretty BundleTrace`.
    pub fn pretty(&self) -> String {
        format!(
            "#{}:{}",
            self.key_ident_serial,
            bytes_to_lower_hex(&self.key_ident_vk_hex)
        )
    }
}

/// Representation of a tagged key bundle in trace logs. Mirrors
/// upstream `TaggedBundleTrace`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TaggedBundleTrace {
    /// `keyMutationTimestamp`.
    pub key_mutation_timestamp: Timestamp,
    /// `keyMutationKey`.
    pub key_mutation_key: Option<BundleTrace>,
}

impl TaggedBundleTrace {
    /// Mirror of upstream `Pretty TaggedBundleTrace`.
    pub fn pretty(&self) -> String {
        let key = self
            .key_mutation_key
            .as_ref()
            .map_or_else(|| "<DROP KEY>".to_string(), BundleTrace::pretty);
        format!("{key}[{}]", self.key_mutation_timestamp)
    }
}

/// Logging messages that the ControlDriver may send.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ControlDriverTrace {
    /// `ControlDriverSendingVersionID`.
    ControlDriverSendingVersionID(VersionIdentifier),
    /// `ControlDriverReceivingVersionID`.
    ControlDriverReceivingVersionID,
    /// `ControlDriverReceivedVersionID`.
    ControlDriverReceivedVersionID(VersionIdentifier),
    /// `ControlDriverInvalidVersion`.
    ControlDriverInvalidVersion(VersionIdentifier, VersionIdentifier),
    /// `ControlDriverSendingCommand`.
    ControlDriverSendingCommand(Command),
    /// `ControlDriverSentCommand`.
    ControlDriverSentCommand(Command),
    /// `ControlDriverReceivingKey`.
    ControlDriverReceivingKey,
    /// `ControlDriverReceivedKey`.
    ControlDriverReceivedKey(Vec<u8>),
    /// `ControlDriverInvalidKey`.
    ControlDriverInvalidKey,
    /// `ControlDriverReceivingCommand`.
    ControlDriverReceivingCommand,
    /// `ControlDriverReceivedCommand`.
    ControlDriverReceivedCommand(Command),
    /// `ControlDriverConfirmingKey`.
    ControlDriverConfirmingKey,
    /// `ControlDriverConfirmedKey`.
    ControlDriverConfirmedKey,
    /// `ControlDriverDecliningKey`.
    ControlDriverDecliningKey,
    /// `ControlDriverDeclinedKey`.
    ControlDriverDeclinedKey,
    /// `ControlDriverConfirmingKeyDrop`.
    ControlDriverConfirmingKeyDrop,
    /// `ControlDriverConfirmedKeyDrop`.
    ControlDriverConfirmedKeyDrop,
    /// `ControlDriverDecliningKeyDrop`.
    ControlDriverDecliningKeyDrop,
    /// `ControlDriverDeclinedKeyDrop`.
    ControlDriverDeclinedKeyDrop,
    /// `ControlDriverNoPublicKeyToReturn`.
    ControlDriverNoPublicKeyToReturn,
    /// `ControlDriverNoPublicKeyToDrop`.
    ControlDriverNoPublicKeyToDrop,
    /// `ControlDriverReturningPublicKey`.
    ControlDriverReturningPublicKey,
    /// `ControlDriverConnectionClosed`.
    ControlDriverConnectionClosed,
    /// `ControlDriverCRefEvent`.
    ControlDriverCRefEvent(String),
    /// `ControlDriverInvalidCommand`.
    ControlDriverInvalidCommand,
    /// `ControlDriverProtocolError`.
    ControlDriverProtocolError(String),
    /// `ControlDriverMisc`.
    ControlDriverMisc(String),
}

impl ControlDriverTrace {
    /// Mirror of selected upstream `Pretty ControlDriverTrace` cases.
    pub fn pretty(&self) -> String {
        match self {
            Self::ControlDriverSendingVersionID(v) => format!("sending version ID {v}"),
            Self::ControlDriverReceivedVersionID(v) => format!("received version ID {v}"),
            Self::ControlDriverInvalidVersion(v1, v2) => {
                format!("invalid version {v1} {v2}")
            }
            Self::ControlDriverSendingCommand(command) => {
                format!("sending command {}", command.pretty())
            }
            Self::ControlDriverSentCommand(command) => format!("sent command{}", command.pretty()),
            Self::ControlDriverReceivedKey(key) => {
                format!("received key {}", bytes_to_lower_hex(key))
            }
            Self::ControlDriverReceivedCommand(command) => {
                format!("received command {}", command.pretty())
            }
            Self::ControlDriverMisc(message) => message.clone(),
            other => prettify_debug_constructor("ControlDriver", &format!("{other:?}")),
        }
    }
}

/// Logging messages that the ServiceDriver may send.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ServiceDriverTrace {
    /// `ServiceDriverSendingVersionID`.
    ServiceDriverSendingVersionID(VersionIdentifier),
    /// `ServiceDriverReceivingVersionID`.
    ServiceDriverReceivingVersionID,
    /// `ServiceDriverReceivedVersionID`.
    ServiceDriverReceivedVersionID(VersionIdentifier),
    /// `ServiceDriverInvalidVersion`.
    ServiceDriverInvalidVersion(VersionIdentifier, VersionIdentifier),
    /// `ServiceDriverSendingKey`.
    ServiceDriverSendingKey(TaggedBundleTrace),
    /// `ServiceDriverSentKey`.
    ServiceDriverSentKey(TaggedBundleTrace),
    /// `ServiceDriverReceivingKey`.
    ServiceDriverReceivingKey,
    /// `ServiceDriverReceivedKey`.
    ServiceDriverReceivedKey(TaggedBundleTrace),
    /// `ServiceDriverConfirmingKey`.
    ServiceDriverConfirmingKey,
    /// `ServiceDriverConfirmedKey`.
    ServiceDriverConfirmedKey,
    /// `ServiceDriverDecliningKey`.
    ServiceDriverDecliningKey(RecvResult),
    /// `ServiceDriverDeclinedKey`.
    ServiceDriverDeclinedKey(RecvResult),
    /// `ServiceDriverRequestingKeyDrop`.
    ServiceDriverRequestingKeyDrop(Timestamp),
    /// `ServiceDriverRequestedKeyDrop`.
    ServiceDriverRequestedKeyDrop(Timestamp),
    /// `ServiceDriverReceivingKeyDrop`.
    ServiceDriverReceivingKeyDrop,
    /// `ServiceDriverReceivedKeyDrop`.
    ServiceDriverReceivedKeyDrop(Timestamp),
    /// `ServiceDriverConnectionClosed`.
    ServiceDriverConnectionClosed,
    /// `ServiceDriverCRefEvent`.
    ServiceDriverCRefEvent(String),
    /// `ServiceDriverProtocolError`.
    ServiceDriverProtocolError(String),
    /// `ServiceDriverMisc`.
    ServiceDriverMisc(String),
}

impl ServiceDriverTrace {
    /// Mirror of selected upstream `Pretty ServiceDriverTrace` cases.
    pub fn pretty(&self) -> String {
        match self {
            Self::ServiceDriverSendingVersionID(v) => format!("sending version ID {v}"),
            Self::ServiceDriverReceivedVersionID(v) => format!("received version ID {v}"),
            Self::ServiceDriverInvalidVersion(v1, v2) => {
                format!("invalid version {v1} {v2}")
            }
            Self::ServiceDriverSendingKey(key) => format!("sending key {}", key.pretty()),
            Self::ServiceDriverSentKey(key) => format!("sent key {}", key.pretty()),
            Self::ServiceDriverReceivedKey(key) => format!("received key {}", key.pretty()),
            Self::ServiceDriverConfirmingKey => "confirming key".to_string(),
            Self::ServiceDriverConfirmedKey => "confirmed key".to_string(),
            Self::ServiceDriverDecliningKey(result) => format!("declining key {result}"),
            Self::ServiceDriverDeclinedKey(result) => format!("declined key {result}"),
            Self::ServiceDriverRequestingKeyDrop(ts) => format!("requesting key drop {ts}"),
            Self::ServiceDriverRequestedKeyDrop(ts) => format!("requested key drop {ts}"),
            Self::ServiceDriverReceivedKeyDrop(ts) => format!("received key drop {ts}"),
            Self::ServiceDriverCRefEvent(event) => format!("CRef event {event}"),
            Self::ServiceDriverProtocolError(err) => format!("protocol error {err}"),
            Self::ServiceDriverMisc(message) => message.clone(),
            other => prettify_debug_constructor("ServiceDriver", &format!("{other:?}")),
        }
    }
}

fn bytes_to_lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn prettify_debug_constructor(prefix: &str, debug: &str) -> String {
    let constructor = debug
        .split(['(', '{'])
        .next()
        .unwrap_or(debug)
        .strip_prefix(prefix)
        .unwrap_or(debug);
    split_pascal_words(constructor)
        .join(" ")
        .to_ascii_lowercase()
}

fn split_pascal_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for ch in input.chars() {
        if ch.is_uppercase() && !current.is_empty() {
            words.push(current);
            current = String::new();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_ordinals_match_upstream_via_enum_order() {
        for (idx, command) in Command::ALL.iter().copied().enumerate() {
            let ordinal = idx as u8;
            assert_eq!(command.ordinal(), ordinal);
            assert_eq!(Command::from_ordinal(ordinal), Some(command));
        }
        assert_eq!(Command::from_ordinal(6), None);
    }

    #[test]
    fn command_pretty_matches_upstream_instance() {
        assert_eq!(Command::GenStagedKeyCmd.pretty(), "gen staged key");
        assert_eq!(Command::QueryStagedKeyCmd.to_string(), "query staged key");
        assert_eq!(Command::RequestInfoCmd.to_string(), "request info");
    }

    #[test]
    fn tagged_bundle_trace_pretty_matches_upstream_shape() {
        let trace = TaggedBundleTrace {
            key_mutation_timestamp: 42,
            key_mutation_key: Some(BundleTrace {
                key_ident_serial: 7,
                key_ident_vk_hex: vec![0xab, 0xcd],
            }),
        };
        assert_eq!(trace.pretty(), "#7:abcd[42]");

        let dropped = TaggedBundleTrace {
            key_mutation_timestamp: 99,
            key_mutation_key: None,
        };
        assert_eq!(dropped.pretty(), "<DROP KEY>[99]");
    }

    #[test]
    fn selected_driver_pretty_cases_match_upstream_strings() {
        assert_eq!(
            ControlDriverTrace::ControlDriverSendingVersionID(mk_version_identifier("3")).pretty(),
            "sending version ID 3"
        );
        assert_eq!(
            ControlDriverTrace::ControlDriverReceivedCommand(Command::DropKeyCmd).pretty(),
            "received command drop key"
        );
        assert_eq!(
            ServiceDriverTrace::ServiceDriverDecliningKey(RecvResult::RecvErrorNoKey).pretty(),
            "declining key NoKey"
        );
        assert_eq!(
            ServiceDriverTrace::ServiceDriverProtocolError("boom".to_string()).pretty(),
            "protocol error boom"
        );
    }
}
