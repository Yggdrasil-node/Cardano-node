//! Benchmarking script runner for `tx-generator`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script.hs`.
//! Ports the `Script` alias and `runScript` dispatch boundary while
//! hosting the Rust submodules that mirror
//! `Cardano.Benchmarking.Script.*`.

pub mod action;
pub mod aeson;
pub mod core;
pub mod env;
pub mod types;

use crate::script::action::action;
use crate::script::env::{
    Env, Error, ProtocolParameterMode, get_env_threads, set_proto_param_mode,
};
use crate::script::types::Script;

/// Error emitted by `run_script`.
#[derive(Debug, thiserror::Error)]
pub enum RunScriptError {
    /// An action failed.
    #[error("action #{index} failed: {source}")]
    Action {
        /// 1-based action index.
        index: usize,
        /// Underlying script error.
        source: Error,
    },
    /// The script reached the upstream benchmark-control requirement.
    #[error("{0}")]
    Final(Error),
}

impl RunScriptError {
    /// Return the underlying script error.
    pub fn source_error(&self) -> &Error {
        match self {
            Self::Action { source, .. } | Self::Final(source) => source,
        }
    }
}

/// Mirror of upstream `runScript`.
pub fn run_script(env: &mut Env, script: &Script) -> Result<(), RunScriptError> {
    set_proto_param_mode(env, ProtocolParameterMode::ProtocolParameterQuery);
    for (index, script_action) in script.iter().enumerate() {
        action(env, script_action).map_err(|source| RunScriptError::Action {
            index: index + 1,
            source,
        })?;
    }
    if get_env_threads(env).is_some() {
        Ok(())
    } else {
        Err(RunScriptError::Final(Error::TxGenError(
            "Cardano.Benchmarking.Script.runScript: AsyncBenchmarkControl absent from map in execScript"
                .to_string(),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::types::{Action, SigningKeyEnvelope};

    #[test]
    fn run_script_sets_query_protocol_parameters_before_actions() {
        let mut env = Env::empty_env();
        let script = vec![
            Action::InitWallet("wallet".to_string()),
            Action::WaitBenchmark,
        ];

        let err = run_script(&mut env, &script).expect_err("missing benchmark control");

        assert_eq!(
            env.proto_params,
            Some(ProtocolParameterMode::ProtocolParameterQuery)
        );
        assert_eq!(
            err.to_string(),
            "action #2 failed: TxGenError: waitBenchmark: missing AsyncBenchmarkControl"
        );
    }

    #[test]
    fn run_script_reaches_final_async_control_requirement() {
        let mut env = Env::empty_env();
        let script = vec![
            Action::InitWallet("wallet".to_string()),
            Action::DefineSigningKey(
                "key".to_string(),
                SigningKeyEnvelope::payment_signing_key_shelley("5820abcd"),
            ),
        ];

        let err = run_script(&mut env, &script).expect_err("missing final benchmark control");

        assert_eq!(
            err.to_string(),
            "TxGenError: Cardano.Benchmarking.Script.runScript: AsyncBenchmarkControl absent from map in execScript"
        );
    }
}
