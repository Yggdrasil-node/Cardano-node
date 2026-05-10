//! Typed configuration surface for the `kes-agent-control` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/cli/ControlMain.hs.
//!
//! Direct ports of upstream's data declarations:
//!
//! - [`CommonOptions`] — `data CommonOptions` 5-field record
//!   (control_path, verbosity, retry_delay, retry_exponential,
//!   retry_attempts; all optional with environment-variable + flag
//!   merge semantics).
//! - [`GenKeyOptions`] — `data GenKeyOptions = GenKeyOptions { gkoCommon, gkoKESVerificationKeyFile }`.
//! - [`QueryKeyOptions`] — `data QueryKeyOptions = QueryKeyOptions { qkoCommon, qkoKESVerificationKeyFile }`.
//! - [`DropStagedKeyOptions`] — `newtype DropStagedKeyOptions = DropStagedKeyOptions { dskoCommon }`.
//! - [`DropKeyOptions`] — `newtype DropKeyOptions = DropKeyOptions { dkoCommon }`.
//! - [`InstallKeyOptions`] — `data InstallKeyOptions = InstallKeyOptions { ikoCommon, ikoOpCertFile }`.
//! - [`ProgramOptions`] — `data ProgramOptions = RunGenKey | RunQueryKey | RunDropStagedKey | RunInstallKey | RunDropKey | RunGetInfo` (6-variant sum).
//!
//! Defaults match upstream's `defCommonOptions` / `defGenKeyOptions`
//! / etc. — `Default::default()` for each type yields the same values
//! upstream provides via `defXyzOptions`.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **Haskell `Semigroup` instances** for `CommonOptions` / `GenKey` /
//!   etc.: upstream uses `<>` to merge env-var-derived options with
//!   CLI-flag-derived options (env values fill in missing CLI values).
//!   Yggdrasil's port uses an explicit [`CommonOptions::merge`] method
//!   matching the upstream `(<>)` pattern but exposed as a regular
//!   inherent method. The merge semantics (left has priority over
//!   right; first non-`None` wins) are byte-equivalent.
//! - **`WithCommonOptions` typeclass**: upstream uses a typeclass to
//!   thread the common options into per-subcommand options. Yggdrasil
//!   exposes [`with_common_options`]-style helpers per options struct
//!   instead of a trait, since each implementation is one-line.

use std::path::PathBuf;

/// Options common to every subcommand. Mirrors upstream `CommonOptions`.
///
/// All fields are `Option<_>` so env-var-derived defaults can be
/// merged with CLI-flag-derived overrides via [`Self::merge`].
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct CommonOptions {
    /// Socket address for control connections to a running kes-agent
    /// process. Mirrors upstream `optControlPath` /
    /// `--control-address` / `$KES_AGENT_CONTROL_PATH`.
    pub control_path: Option<String>,
    /// Verbosity level (0 = quiet, higher = more chatter).
    /// Mirrors upstream `optVerbosity` / `--verbose`.
    pub verbosity: Option<i32>,
    /// Connection retry interval in milliseconds. Mirrors upstream
    /// `optRetryDelay` / `--retry-interval` /
    /// `$KES_AGENT_CONTROL_RETRY_INTERVAL`.
    pub retry_delay: Option<i64>,
    /// Whether to use exponential backoff between retries. Mirrors
    /// upstream `optRetryExponential` / `--retry-exponential`.
    pub retry_exponential: Option<bool>,
    /// Maximum retry count. Mirrors upstream `optRetryAttempts` /
    /// `--retry-attempts` / `$KES_AGENT_CONTROL_RETRY_ATTEMPTS`.
    pub retry_attempts: Option<i64>,
}

impl CommonOptions {
    /// Default values matching upstream `defCommonOptions`.
    ///
    /// ```text
    /// control_path     = Some("/tmp/kes-agent-control.socket")
    /// verbosity        = Some(0)
    /// retry_delay      = None
    /// retry_exponential = None
    /// retry_attempts   = None
    /// ```
    pub fn defaults() -> Self {
        CommonOptions {
            control_path: Some("/tmp/kes-agent-control.socket".to_string()),
            verbosity: Some(0),
            retry_delay: None,
            retry_exponential: None,
            retry_attempts: None,
        }
    }

    /// Merge this options struct with another; left wins on every
    /// field. Mirrors upstream `Semigroup` instance: each field of
    /// the result is `self.field.or(other.field)`.
    ///
    /// Used to thread environment-variable-derived defaults through
    /// CLI-flag-derived overrides:
    /// `cli_options.merge(env_options).merge(CommonOptions::defaults())`.
    pub fn merge(self, other: CommonOptions) -> CommonOptions {
        CommonOptions {
            control_path: self.control_path.or(other.control_path),
            verbosity: self.verbosity.or(other.verbosity),
            retry_delay: self.retry_delay.or(other.retry_delay),
            retry_exponential: self.retry_exponential.or(other.retry_exponential),
            retry_attempts: self.retry_attempts.or(other.retry_attempts),
        }
    }

    /// Derive a `CommonOptions` from process environment variables.
    /// Mirrors upstream `optFromEnv :: IO CommonOptions`.
    ///
    /// Reads:
    ///
    /// - `KES_AGENT_CONTROL_PATH` → [`Self::control_path`].
    /// - `KES_AGENT_CONTROL_RETRY_INTERVAL` → [`Self::retry_delay`]
    ///   (fails open: malformed numeric values are silently dropped,
    ///   matching upstream's `(>>= readMaybe)` behavior).
    /// - `KES_AGENT_CONTROL_RETRY_ATTEMPTS` → [`Self::retry_attempts`]
    ///   (same fail-open behavior).
    ///
    /// Verbosity and retry-exponential are NOT env-derivable
    /// upstream — those fields stay `None` here. The result is
    /// designed to be merged into the CLI-derived options as a
    /// lower-priority overlay before the defaults pass.
    pub fn from_env() -> Self {
        Self::from_env_lookup(|key| std::env::var(key).ok())
    }

    /// Test-friendly variant of [`Self::from_env`] that takes a
    /// closure for the variable lookup. Useful for unit tests that
    /// need to seed specific environment-variable values without
    /// mutating the process-wide environment.
    pub fn from_env_lookup<F>(lookup: F) -> Self
    where
        F: Fn(&str) -> Option<String>,
    {
        let control_path = lookup("KES_AGENT_CONTROL_PATH");
        let retry_delay = lookup("KES_AGENT_CONTROL_RETRY_INTERVAL")
            .as_deref()
            .and_then(|s| s.parse::<i64>().ok());
        let retry_attempts = lookup("KES_AGENT_CONTROL_RETRY_ATTEMPTS")
            .as_deref()
            .and_then(|s| s.parse::<i64>().ok());
        CommonOptions {
            control_path,
            verbosity: None,
            retry_delay,
            retry_exponential: None,
            retry_attempts,
        }
    }
}

/// `gen-staged-key` subcommand options. Mirrors upstream
/// `GenKeyOptions = GenKeyOptions { gkoCommon, gkoKESVerificationKeyFile }`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct GenKeyOptions {
    /// Common (control-socket / verbosity / retry) options.
    pub common: CommonOptions,
    /// Path to write the generated KES verification key. Mirrors
    /// upstream's `gkoKESVerificationKeyFile`. Default
    /// `Some("kes.vkey")`.
    pub kes_verification_key_file: Option<PathBuf>,
}

impl GenKeyOptions {
    /// Default values matching upstream `defGenKeyOptions`.
    pub fn defaults() -> Self {
        GenKeyOptions {
            common: CommonOptions::defaults(),
            kes_verification_key_file: Some(PathBuf::from("kes.vkey")),
        }
    }
}

/// `export-staged-vkey` subcommand options. Mirrors upstream
/// `QueryKeyOptions = QueryKeyOptions { qkoCommon, qkoKESVerificationKeyFile }`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct QueryKeyOptions {
    /// Common options.
    pub common: CommonOptions,
    /// Path to write the queried KES verification key. Default
    /// `Some("kes.vkey")`.
    pub kes_verification_key_file: Option<PathBuf>,
}

impl QueryKeyOptions {
    /// Default values matching upstream `defQueryKeyOptions`.
    pub fn defaults() -> Self {
        QueryKeyOptions {
            common: CommonOptions::defaults(),
            kes_verification_key_file: Some(PathBuf::from("kes.vkey")),
        }
    }
}

/// `drop-staged-key` subcommand options. Mirrors upstream
/// `newtype DropStagedKeyOptions = DropStagedKeyOptions { dskoCommon }`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct DropStagedKeyOptions {
    /// Common options.
    pub common: CommonOptions,
}

impl DropStagedKeyOptions {
    /// Default values matching upstream `defDropStagedKeyOptions`.
    pub fn defaults() -> Self {
        DropStagedKeyOptions {
            common: CommonOptions::defaults(),
        }
    }
}

/// `drop-key` subcommand options. Mirrors upstream
/// `newtype DropKeyOptions = DropKeyOptions { dkoCommon }`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct DropKeyOptions {
    /// Common options.
    pub common: CommonOptions,
}

impl DropKeyOptions {
    /// Default values matching upstream `defDropKeyOptions`.
    pub fn defaults() -> Self {
        DropKeyOptions {
            common: CommonOptions::defaults(),
        }
    }
}

/// `install-key` subcommand options. Mirrors upstream
/// `InstallKeyOptions = InstallKeyOptions { ikoCommon, ikoOpCertFile }`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct InstallKeyOptions {
    /// Common options.
    pub common: CommonOptions,
    /// Path to the operational-certificate file. Mirrors upstream's
    /// `ikoOpCertFile`. Default `Some("kes.vkey")` (note: upstream's
    /// default literal is `"kes.vkey"` — likely an upstream bug since
    /// this should logically be `node.cert` or similar; preserved as-is
    /// for byte-equivalent default behavior).
    pub op_cert_file: Option<PathBuf>,
}

impl InstallKeyOptions {
    /// Default values matching upstream `defInstallKeyOptions`.
    pub fn defaults() -> Self {
        InstallKeyOptions {
            common: CommonOptions::defaults(),
            op_cert_file: Some(PathBuf::from("kes.vkey")),
        }
    }
}

/// Top-level subcommand dispatch. Mirrors upstream
/// ```haskell
/// data ProgramOptions
///   = RunGenKey GenKeyOptions
///   | RunQueryKey QueryKeyOptions
///   | RunDropStagedKey DropStagedKeyOptions
///   | RunInstallKey InstallKeyOptions
///   | RunDropKey DropKeyOptions
///   | RunGetInfo CommonOptions
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ProgramOptions {
    /// `gen-staged-key`: generate a KES key pair on the agent and
    /// stage it for later promotion.
    RunGenKey(GenKeyOptions),
    /// `export-staged-vkey`: query the staged KES verification key
    /// and write it to the supplied path.
    RunQueryKey(QueryKeyOptions),
    /// `drop-staged-key`: discard a staged KES key.
    RunDropStagedKey(DropStagedKeyOptions),
    /// `install-key`: promote a staged key + opcert to the active
    /// production slot.
    RunInstallKey(InstallKeyOptions),
    /// `drop-key`: revoke an installed key.
    RunDropKey(DropKeyOptions),
    /// `info`: query the agent's status / staged-key inventory.
    RunGetInfo(CommonOptions),
}

impl ProgramOptions {
    /// Apply common-option overrides to whichever subcommand is
    /// selected. Mirrors upstream `programOptionsWithCommonOptions`.
    pub fn with_common_options(self, common: CommonOptions) -> Self {
        match self {
            ProgramOptions::RunGenKey(o) => ProgramOptions::RunGenKey(GenKeyOptions {
                common: common.merge(o.common),
                ..o
            }),
            ProgramOptions::RunQueryKey(o) => ProgramOptions::RunQueryKey(QueryKeyOptions {
                common: common.merge(o.common),
                ..o
            }),
            ProgramOptions::RunDropStagedKey(o) => {
                ProgramOptions::RunDropStagedKey(DropStagedKeyOptions {
                    common: common.merge(o.common),
                })
            }
            ProgramOptions::RunInstallKey(o) => ProgramOptions::RunInstallKey(InstallKeyOptions {
                common: common.merge(o.common),
                ..o
            }),
            ProgramOptions::RunDropKey(o) => ProgramOptions::RunDropKey(DropKeyOptions {
                common: common.merge(o.common),
            }),
            ProgramOptions::RunGetInfo(o) => ProgramOptions::RunGetInfo(common.merge(o)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_options_defaults_match_upstream() {
        let d = CommonOptions::defaults();
        assert_eq!(
            d.control_path.as_deref(),
            Some("/tmp/kes-agent-control.socket")
        );
        assert_eq!(d.verbosity, Some(0));
        assert!(d.retry_delay.is_none());
        assert!(d.retry_exponential.is_none());
        assert!(d.retry_attempts.is_none());
    }

    #[test]
    fn common_options_merge_left_priority() {
        let cli = CommonOptions {
            control_path: Some("/tmp/cli.sock".to_string()),
            verbosity: None,
            retry_delay: Some(500),
            retry_exponential: None,
            retry_attempts: None,
        };
        let env = CommonOptions {
            control_path: Some("/tmp/env.sock".to_string()),
            verbosity: Some(2),
            retry_delay: Some(1000),
            retry_exponential: Some(true),
            retry_attempts: Some(5),
        };
        let merged = cli.merge(env);
        assert_eq!(merged.control_path.as_deref(), Some("/tmp/cli.sock"));
        assert_eq!(merged.verbosity, Some(2));
        assert_eq!(merged.retry_delay, Some(500));
        assert_eq!(merged.retry_exponential, Some(true));
        assert_eq!(merged.retry_attempts, Some(5));
    }

    #[test]
    fn from_env_lookup_returns_all_none_when_no_vars_set() {
        let env = CommonOptions::from_env_lookup(|_| None);
        assert!(env.control_path.is_none());
        assert!(env.verbosity.is_none());
        assert!(env.retry_delay.is_none());
        assert!(env.retry_exponential.is_none());
        assert!(env.retry_attempts.is_none());
    }

    #[test]
    fn from_env_lookup_reads_control_path() {
        let env = CommonOptions::from_env_lookup(|key| match key {
            "KES_AGENT_CONTROL_PATH" => Some("/var/run/k.sock".to_string()),
            _ => None,
        });
        assert_eq!(env.control_path.as_deref(), Some("/var/run/k.sock"));
    }

    #[test]
    fn from_env_lookup_reads_retry_interval() {
        let env = CommonOptions::from_env_lookup(|key| match key {
            "KES_AGENT_CONTROL_RETRY_INTERVAL" => Some("750".to_string()),
            _ => None,
        });
        assert_eq!(env.retry_delay, Some(750));
    }

    #[test]
    fn from_env_lookup_reads_retry_attempts() {
        let env = CommonOptions::from_env_lookup(|key| match key {
            "KES_AGENT_CONTROL_RETRY_ATTEMPTS" => Some("10".to_string()),
            _ => None,
        });
        assert_eq!(env.retry_attempts, Some(10));
    }

    #[test]
    fn from_env_lookup_silently_drops_malformed_numbers() {
        // Mirrors upstream's `(>>= readMaybe)` behavior: malformed
        // numeric env values are silently dropped, NOT propagated as
        // an error.
        let env = CommonOptions::from_env_lookup(|key| match key {
            "KES_AGENT_CONTROL_RETRY_INTERVAL" => Some("not-a-number".to_string()),
            "KES_AGENT_CONTROL_RETRY_ATTEMPTS" => Some("also-not-a-number".to_string()),
            _ => None,
        });
        assert!(env.retry_delay.is_none());
        assert!(env.retry_attempts.is_none());
    }

    #[test]
    fn from_env_lookup_does_not_set_verbosity_or_retry_exponential() {
        // Upstream's optFromEnv does NOT consult env vars for verbosity
        // or retry-exponential — those are CLI/default-only.
        let env = CommonOptions::from_env_lookup(|key| match key {
            "KES_AGENT_CONTROL_PATH" => Some("/k.sock".to_string()),
            "KES_AGENT_CONTROL_RETRY_INTERVAL" => Some("100".to_string()),
            "KES_AGENT_CONTROL_RETRY_ATTEMPTS" => Some("5".to_string()),
            _ => None,
        });
        assert!(env.verbosity.is_none());
        assert!(env.retry_exponential.is_none());
    }

    #[test]
    fn from_env_lookup_full_resolution_chain() {
        // Simulate the lib.rs::run_main() resolution order:
        //   cli.merge(env).merge(defaults)
        // CLI sets control_path; env sets retry_attempts; defaults
        // fill in everything else.
        let cli = CommonOptions {
            control_path: Some("/cli.sock".to_string()),
            verbosity: Some(3),
            ..CommonOptions::default()
        };
        let env = CommonOptions::from_env_lookup(|key| match key {
            "KES_AGENT_CONTROL_RETRY_ATTEMPTS" => Some("7".to_string()),
            _ => None,
        });
        let resolved = cli.merge(env).merge(CommonOptions::defaults());
        // CLI wins on control_path + verbosity.
        assert_eq!(resolved.control_path.as_deref(), Some("/cli.sock"));
        assert_eq!(resolved.verbosity, Some(3));
        // Env wins on retry_attempts (CLI didn't set it).
        assert_eq!(resolved.retry_attempts, Some(7));
        // Defaults fill in retry_delay (None from CLI + None from env)
        // — defaults also has None for retry_delay so it stays None.
        assert!(resolved.retry_delay.is_none());
    }

    #[test]
    fn gen_key_options_defaults_match_upstream() {
        let d = GenKeyOptions::defaults();
        assert_eq!(
            d.kes_verification_key_file
                .as_deref()
                .and_then(|p| p.to_str()),
            Some("kes.vkey")
        );
        assert_eq!(d.common.verbosity, Some(0));
    }

    #[test]
    fn query_key_options_defaults_match_upstream() {
        let d = QueryKeyOptions::defaults();
        assert_eq!(
            d.kes_verification_key_file
                .as_deref()
                .and_then(|p| p.to_str()),
            Some("kes.vkey")
        );
    }

    #[test]
    fn drop_staged_key_options_defaults_carry_common() {
        let d = DropStagedKeyOptions::defaults();
        assert_eq!(d.common.verbosity, Some(0));
    }

    #[test]
    fn drop_key_options_defaults_carry_common() {
        let d = DropKeyOptions::defaults();
        assert_eq!(d.common.verbosity, Some(0));
    }

    #[test]
    fn install_key_options_defaults_match_upstream_quirk() {
        let d = InstallKeyOptions::defaults();
        // Upstream defaults op_cert_file to "kes.vkey" — preserved
        // verbatim for byte-equivalent default behavior.
        assert_eq!(
            d.op_cert_file.as_deref().and_then(|p| p.to_str()),
            Some("kes.vkey")
        );
    }

    #[test]
    fn program_options_run_gen_key_round_trip() {
        let p = ProgramOptions::RunGenKey(GenKeyOptions::defaults());
        match p {
            ProgramOptions::RunGenKey(o) => {
                assert_eq!(
                    o.kes_verification_key_file
                        .as_deref()
                        .and_then(|p| p.to_str()),
                    Some("kes.vkey")
                );
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn program_options_run_get_info_round_trip() {
        let p = ProgramOptions::RunGetInfo(CommonOptions::defaults());
        match p {
            ProgramOptions::RunGetInfo(c) => {
                assert_eq!(c.verbosity, Some(0));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn with_common_options_applies_to_run_gen_key() {
        let cli = CommonOptions {
            control_path: Some("/tmp/cli.sock".to_string()),
            ..CommonOptions::default()
        };
        let p = ProgramOptions::RunGenKey(GenKeyOptions::defaults());
        let merged = p.with_common_options(cli);
        match merged {
            ProgramOptions::RunGenKey(o) => {
                assert_eq!(o.common.control_path.as_deref(), Some("/tmp/cli.sock"));
                // The default verbosity from defaults() should still be present.
                assert_eq!(o.common.verbosity, Some(0));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn with_common_options_applies_to_run_get_info() {
        let cli = CommonOptions {
            verbosity: Some(5),
            ..CommonOptions::default()
        };
        let p = ProgramOptions::RunGetInfo(CommonOptions::defaults());
        let merged = p.with_common_options(cli);
        match merged {
            ProgramOptions::RunGetInfo(c) => {
                assert_eq!(c.verbosity, Some(5));
                assert_eq!(
                    c.control_path.as_deref(),
                    Some("/tmp/kes-agent-control.socket")
                );
            }
            _ => panic!("wrong variant"),
        }
    }
}
