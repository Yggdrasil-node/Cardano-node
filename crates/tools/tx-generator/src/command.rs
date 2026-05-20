//! Typed command surface for the `tx-generator` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Command.hs`.
//! Ports the upstream `Command` sum type and `commandParser` grammar:
//! `json`, `json_highlevel`, `compile`, `selftest`, and `version`.
//! Runtime execution of the parsed commands remains in later strict
//! slices for `Script`, `Compiler`, `Setup`, and `GeneratorTx`.

use std::path::PathBuf;

/// Mirror of upstream `data Command`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    /// `json FILEPATH` — run a low-level benchmarking script.
    Json(PathBuf),
    /// `json_highlevel FILEPATH ...` — run with flat high-level options.
    JsonHighLevel(JsonHighLevelCommand),
    /// `compile FILEPATH` — compile flat options to a benchmarking script.
    Compile(PathBuf),
    /// `selftest [FILEPATH]` — run the built-in selftest.
    Selftest(Option<PathBuf>),
    /// `version` — show the tx-generator version.
    Version,
}

impl Command {
    /// Upstream subcommand token.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Json(_) => "json",
            Command::JsonHighLevel(_) => "json_highlevel",
            Command::Compile(_) => "compile",
            Command::Selftest(_) => "selftest",
            Command::Version => "version",
        }
    }
}

/// Mirror of upstream `TestnetConfig`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestnetConfig {
    /// `--testnet-config-dir DIR`
    pub testnet_config_dir: PathBuf,
}

/// Arguments carried by upstream `JsonHL`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonHighLevelCommand {
    /// Positional high-level benchmarking options file.
    pub config_file: PathBuf,
    /// Optional discovered `cardano-testnet` output directory.
    pub testnet_config: Option<TestnetConfig>,
    /// Optional `--nodeConfig` / first `-n` override.
    pub node_config: Option<PathBuf>,
    /// Optional `--cardano-tracer` / second `-n` override.
    pub cardano_tracer: Option<PathBuf>,
}

/// Errors from the upstream-shaped command parser.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CommandParseError {
    /// No subcommand was supplied.
    #[error("Missing: COMMAND\n\nUsage: tx-generator COMMAND")]
    MissingCommand,
    /// The subcommand is not part of upstream `commandParser`.
    #[error("Invalid argument `{0}`\n\nUsage: tx-generator COMMAND")]
    UnknownCommand(String),
    /// A required positional argument was missing.
    #[error("Missing: {command} {metavar}")]
    MissingArgument {
        /// Subcommand being parsed.
        command: &'static str,
        /// Upstream metavar.
        metavar: &'static str,
    },
    /// A subcommand received too many positional arguments.
    #[error("Unexpected argument `{arg}` for command `{command}`")]
    UnexpectedArgument {
        /// Subcommand being parsed.
        command: &'static str,
        /// Unexpected argument.
        arg: String,
    },
    /// An option was not followed by its required value.
    #[error("Missing value for option `{0}`")]
    MissingOptionValue(String),
    /// A command-local option was not recognised.
    #[error("Invalid option `{option}` for command `{command}`")]
    UnknownOption {
        /// Subcommand being parsed.
        command: &'static str,
        /// Unknown option token.
        option: String,
    },
    /// An upstream singleton option appeared more than once.
    #[error("Duplicate option `{0}`")]
    DuplicateOption(&'static str),
}

/// Parse a command line after top-level help/version handling.
pub fn parse_command(args: &[String]) -> Result<Command, CommandParseError> {
    let Some((command, rest)) = args.split_first() else {
        return Err(CommandParseError::MissingCommand);
    };

    match command.as_str() {
        "json" => parse_one_file("json", rest).map(Command::Json),
        "json_highlevel" => parse_json_highlevel(rest),
        "compile" => parse_one_file("compile", rest).map(Command::Compile),
        "selftest" => parse_selftest(rest),
        "version" => parse_version(rest),
        other => Err(CommandParseError::UnknownCommand(other.to_owned())),
    }
}

fn parse_one_file(command: &'static str, rest: &[String]) -> Result<PathBuf, CommandParseError> {
    let Some(file) = rest.first() else {
        return Err(CommandParseError::MissingArgument {
            command,
            metavar: "FILEPATH",
        });
    };
    if rest.len() > 1 {
        return Err(CommandParseError::UnexpectedArgument {
            command,
            arg: rest[1].clone(),
        });
    }
    Ok(PathBuf::from(file))
}

fn parse_selftest(rest: &[String]) -> Result<Command, CommandParseError> {
    match rest {
        [] => Ok(Command::Selftest(None)),
        [file] => Ok(Command::Selftest(Some(PathBuf::from(file)))),
        [_, extra, ..] => Err(CommandParseError::UnexpectedArgument {
            command: "selftest",
            arg: extra.clone(),
        }),
    }
}

fn parse_version(rest: &[String]) -> Result<Command, CommandParseError> {
    if let Some(extra) = rest.first() {
        return Err(CommandParseError::UnexpectedArgument {
            command: "version",
            arg: extra.clone(),
        });
    }
    Ok(Command::Version)
}

fn parse_json_highlevel(rest: &[String]) -> Result<Command, CommandParseError> {
    let Some((config_file, options)) = rest.split_first() else {
        return Err(CommandParseError::MissingArgument {
            command: "json_highlevel",
            metavar: "FILEPATH",
        });
    };

    let mut parsed = JsonHighLevelCommand {
        config_file: PathBuf::from(config_file),
        testnet_config: None,
        node_config: None,
        cardano_tracer: None,
    };

    let mut index = 0;
    while index < options.len() {
        let option = &options[index];
        match option.as_str() {
            "--testnet-config-dir" => {
                let value = option_value(options, &mut index, option)?;
                if parsed.testnet_config.is_some() {
                    return Err(CommandParseError::DuplicateOption("--testnet-config-dir"));
                }
                parsed.testnet_config = Some(TestnetConfig {
                    testnet_config_dir: PathBuf::from(value),
                });
            }
            "--nodeConfig" => {
                let value = option_value(options, &mut index, option)?;
                if parsed.node_config.is_some() {
                    return Err(CommandParseError::DuplicateOption("--nodeConfig"));
                }
                parsed.node_config = Some(PathBuf::from(value));
            }
            "--cardano-tracer" => {
                let value = option_value(options, &mut index, option)?;
                if parsed.cardano_tracer.is_some() {
                    return Err(CommandParseError::DuplicateOption("--cardano-tracer"));
                }
                parsed.cardano_tracer = Some(PathBuf::from(value));
            }
            "-n" => {
                let value = option_value(options, &mut index, option)?;
                if parsed.node_config.is_none() {
                    parsed.node_config = Some(PathBuf::from(value));
                } else if parsed.cardano_tracer.is_none() {
                    parsed.cardano_tracer = Some(PathBuf::from(value));
                } else {
                    return Err(CommandParseError::DuplicateOption("-n"));
                }
            }
            other if other.starts_with("--testnet-config-dir=") => {
                if parsed.testnet_config.is_some() {
                    return Err(CommandParseError::DuplicateOption("--testnet-config-dir"));
                }
                parsed.testnet_config = Some(TestnetConfig {
                    testnet_config_dir: PathBuf::from(&other["--testnet-config-dir=".len()..]),
                });
            }
            other if other.starts_with("--nodeConfig=") => {
                if parsed.node_config.is_some() {
                    return Err(CommandParseError::DuplicateOption("--nodeConfig"));
                }
                parsed.node_config = Some(PathBuf::from(&other["--nodeConfig=".len()..]));
            }
            other if other.starts_with("--cardano-tracer=") => {
                if parsed.cardano_tracer.is_some() {
                    return Err(CommandParseError::DuplicateOption("--cardano-tracer"));
                }
                parsed.cardano_tracer = Some(PathBuf::from(&other["--cardano-tracer=".len()..]));
            }
            other if other.starts_with('-') => {
                return Err(CommandParseError::UnknownOption {
                    command: "json_highlevel",
                    option: other.to_owned(),
                });
            }
            other => {
                return Err(CommandParseError::UnexpectedArgument {
                    command: "json_highlevel",
                    arg: other.to_owned(),
                });
            }
        }
        index += 1;
    }

    Ok(Command::JsonHighLevel(parsed))
}

fn option_value<'a>(
    options: &'a [String],
    index: &mut usize,
    option: &str,
) -> Result<&'a str, CommandParseError> {
    let next = *index + 1;
    let Some(value) = options.get(next) else {
        return Err(CommandParseError::MissingOptionValue(option.to_owned()));
    };
    *index = next;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn parse_json_command() {
        assert_eq!(
            parse_command(&strings(&["json", "script.json"])).expect("parses"),
            Command::Json(PathBuf::from("script.json"))
        );
    }

    #[test]
    fn parse_compile_command() {
        assert_eq!(
            parse_command(&strings(&["compile", "options.json"])).expect("parses"),
            Command::Compile(PathBuf::from("options.json"))
        );
    }

    #[test]
    fn parse_selftest_command_with_and_without_file() {
        assert_eq!(
            parse_command(&strings(&["selftest"])).expect("parses"),
            Command::Selftest(None)
        );
        assert_eq!(
            parse_command(&strings(&["selftest", "out.json"])).expect("parses"),
            Command::Selftest(Some(PathBuf::from("out.json")))
        );
    }

    #[test]
    fn parse_version_command() {
        assert_eq!(
            parse_command(&strings(&["version"])).expect("parses"),
            Command::Version
        );
    }

    #[test]
    fn parse_json_highlevel_long_options() {
        assert_eq!(
            parse_command(&strings(&[
                "json_highlevel",
                "config.json",
                "--testnet-config-dir",
                "testnet",
                "--nodeConfig",
                "node.json",
                "--cardano-tracer",
                "tracer.sock",
            ]))
            .expect("parses"),
            Command::JsonHighLevel(JsonHighLevelCommand {
                config_file: PathBuf::from("config.json"),
                testnet_config: Some(TestnetConfig {
                    testnet_config_dir: PathBuf::from("testnet"),
                }),
                node_config: Some(PathBuf::from("node.json")),
                cardano_tracer: Some(PathBuf::from("tracer.sock")),
            })
        );
    }

    #[test]
    fn parse_json_highlevel_equals_options() {
        assert_eq!(
            parse_command(&strings(&[
                "json_highlevel",
                "config.json",
                "--testnet-config-dir=testnet",
                "--nodeConfig=node.json",
                "--cardano-tracer=tracer.sock",
            ]))
            .expect("parses"),
            Command::JsonHighLevel(JsonHighLevelCommand {
                config_file: PathBuf::from("config.json"),
                testnet_config: Some(TestnetConfig {
                    testnet_config_dir: PathBuf::from("testnet"),
                }),
                node_config: Some(PathBuf::from("node.json")),
                cardano_tracer: Some(PathBuf::from("tracer.sock")),
            })
        );
    }

    #[test]
    fn parse_json_highlevel_short_n_is_sequential() {
        assert_eq!(
            parse_command(&strings(&[
                "json_highlevel",
                "config.json",
                "-n",
                "node.json",
                "-n",
                "tracer.sock",
            ]))
            .expect("parses"),
            Command::JsonHighLevel(JsonHighLevelCommand {
                config_file: PathBuf::from("config.json"),
                testnet_config: None,
                node_config: Some(PathBuf::from("node.json")),
                cardano_tracer: Some(PathBuf::from("tracer.sock")),
            })
        );
    }

    #[test]
    fn missing_command_is_rejected() {
        assert_eq!(parse_command(&[]), Err(CommandParseError::MissingCommand));
    }

    #[test]
    fn unknown_command_is_rejected() {
        assert_eq!(
            parse_command(&strings(&["bad"])),
            Err(CommandParseError::UnknownCommand("bad".to_string()))
        );
    }

    #[test]
    fn missing_required_file_is_rejected() {
        assert_eq!(
            parse_command(&strings(&["json"])),
            Err(CommandParseError::MissingArgument {
                command: "json",
                metavar: "FILEPATH",
            })
        );
    }

    #[test]
    fn unexpected_argument_is_rejected() {
        assert_eq!(
            parse_command(&strings(&["compile", "a", "b"])),
            Err(CommandParseError::UnexpectedArgument {
                command: "compile",
                arg: "b".to_string(),
            })
        );
    }

    #[test]
    fn json_highlevel_unknown_option_is_rejected() {
        assert_eq!(
            parse_command(&strings(&["json_highlevel", "config.json", "--bad"])),
            Err(CommandParseError::UnknownOption {
                command: "json_highlevel",
                option: "--bad".to_string(),
            })
        );
    }

    #[test]
    fn json_highlevel_missing_option_value_is_rejected() {
        assert_eq!(
            parse_command(&strings(&["json_highlevel", "config.json", "--nodeConfig"])),
            Err(CommandParseError::MissingOptionValue(
                "--nodeConfig".to_string()
            ))
        );
    }
}
