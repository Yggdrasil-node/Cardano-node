//! Version-handshake driver trace vocabulary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/VersionHandshake/Driver.hs.
//!
//! This module mirrors the data and pretty-rendering surface of
//! upstream `VersionHandshakeDriverTrace`. Raw bearer I/O and direct
//! codecs remain part of the daemon/socket follow-on.

use super::types::VersionIdentifier;

/// Logging messages that the version-handshake driver may send.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum VersionHandshakeDriverTrace {
    /// `VersionHandshakeDriverOfferingVersions`.
    VersionHandshakeDriverOfferingVersions(Vec<VersionIdentifier>),
    /// `VersionHandshakeDriverAcceptingVersion`.
    VersionHandshakeDriverAcceptingVersion(VersionIdentifier),
    /// `VersionHandshakeDriverRejectingVersion`.
    VersionHandshakeDriverRejectingVersion,
    /// `VersionHandshakeDriverMisc`.
    VersionHandshakeDriverMisc(String),
}

impl VersionHandshakeDriverTrace {
    /// Mirror of upstream `Pretty VersionHandshakeDriverTrace`.
    pub fn pretty(&self) -> String {
        match self {
            Self::VersionHandshakeDriverOfferingVersions(versions) => {
                let rendered = versions
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("offering versions: [{rendered}]")
            }
            Self::VersionHandshakeDriverAcceptingVersion(version) => {
                format!("accepting version: {version}")
            }
            Self::VersionHandshakeDriverMisc(message) => message.clone(),
            Self::VersionHandshakeDriverRejectingVersion => "rejecting version".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::types::mk_version_identifier;

    #[test]
    fn pretty_offering_versions_matches_upstream_instance() {
        let trace = VersionHandshakeDriverTrace::VersionHandshakeDriverOfferingVersions(vec![
            mk_version_identifier("Control:2.0"),
            mk_version_identifier("Control:3.0"),
        ]);
        assert_eq!(
            trace.pretty(),
            "offering versions: [Control:2.0, Control:3.0]"
        );
    }

    #[test]
    fn pretty_accept_reject_and_misc_match_upstream_instance() {
        assert_eq!(
            VersionHandshakeDriverTrace::VersionHandshakeDriverAcceptingVersion(
                mk_version_identifier("VersionHandshake:0.1")
            )
            .pretty(),
            "accepting version: VersionHandshake:0.1"
        );
        assert_eq!(
            VersionHandshakeDriverTrace::VersionHandshakeDriverRejectingVersion.pretty(),
            "rejecting version"
        );
        assert_eq!(
            VersionHandshakeDriverTrace::VersionHandshakeDriverMisc("raw".to_string()).pretty(),
            "raw"
        );
    }
}
