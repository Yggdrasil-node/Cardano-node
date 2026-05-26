//! Process harness helpers for the `cardano-testnet` port.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Module-tree parent for upstream
//! `cardano-testnet/src/Testnet/Process/*`.

pub mod cli;
pub mod run;
pub mod run_io;

#[cfg(test)]
mod run_io_tests {
    use super::run::ExecConfig;
    use super::run_io::{
        bin_flex_with_plan, exec_cli_unit_with_plan, exec_cli_with_plan, exec_flex_with_plan,
        find_default_plan_json_file_from, lift_io_annotated, mk_exec_config, proc_flex_with_plan,
    };
    use crate::filepath::Sprocket;

    use std::path::PathBuf;

    #[test]
    fn run_io_plan_json_discovery_matches_upstream_search_order() {
        let root = std::env::temp_dir().join(format!(
            "yggdrasil-cardano-testnet-run-io-{}",
            std::process::id()
        ));
        let nested = root.join("a").join("b");
        let plan = root.join("dist-newstyle").join("cache").join("plan.json");
        std::fs::create_dir_all(plan.parent().expect("plan parent")).expect("create plan dir");
        std::fs::create_dir_all(&nested).expect("create nested dir");
        std::fs::write(&plan, "{}").expect("write plan");

        let discovered = find_default_plan_json_file_from(&nested).expect("discovers plan");

        assert_eq!(discovered, plan);
        std::fs::remove_dir_all(&root).expect("cleanup temp tree");
    }

    #[test]
    fn run_io_proc_flex_prefers_env_then_plan_json_component() {
        let root = std::env::temp_dir().join(format!(
            "yggdrasil-cardano-testnet-run-io-plan-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        let plan = root.join("plan.json");
        std::fs::write(
            &plan,
            r#"{
              "install-plan": [
                {
                  "component-name": "lib:ignored",
                  "components": [
                    {
                      "component-name": "exe:cardano-node",
                      "bin-file": "dist/bin/cardano-node"
                    }
                  ]
                }
              ]
            }"#,
        )
        .expect("write plan");
        let config = ExecConfig {
            env: Some(vec![("A".to_string(), "B".to_string())]),
            cwd: Some(PathBuf::from("work")),
        };

        let env_plan = proc_flex_with_plan(
            &config,
            "cardano-node",
            "CARDANO_NODE",
            ["run"],
            |_| Some("override/cardano-node".to_string()),
            &plan,
        )
        .expect("uses env override");
        assert_eq!(env_plan.executable, "override/cardano-node");
        assert_eq!(env_plan.args, vec!["run"]);
        assert_eq!(env_plan.env, config.env);
        assert_eq!(env_plan.cwd, config.cwd);
        assert!(env_plan.create_group);

        let plan_binary = bin_flex_with_plan("cardano-node", "CARDANO_NODE", |_| None, &plan)
            .expect("uses plan component");
        assert!(plan_binary.ends_with("cardano-node") || plan_binary.ends_with("cardano-node.exe"));

        std::fs::remove_dir_all(&root).expect("cleanup temp tree");
    }

    #[test]
    fn run_io_execution_helpers_use_env_override_and_exec_config_shape() {
        let current_exe = std::env::current_exe()
            .expect("current test binary")
            .to_string_lossy()
            .into_owned();
        let stdout = exec_flex_with_plan(
            &ExecConfig::default(),
            "cardano-cli",
            "CARDANO_CLI",
            ["--help"],
            |_| Some(current_exe.clone()),
            PathBuf::from("unused-plan.json"),
        )
        .expect("executes env override");
        assert!(stdout.contains("Usage") || stdout.contains("USAGE"));

        let cli_stdout = exec_cli_with_plan(
            &ExecConfig::default(),
            ["--help"],
            |_| Some(current_exe.clone()),
            PathBuf::from("unused-plan.json"),
        )
        .expect("execCli' equivalent returns stdout");
        assert_eq!(stdout, cli_stdout);

        exec_cli_unit_with_plan(
            &ExecConfig::default(),
            ["--help"],
            |_| Some(current_exe.clone()),
            PathBuf::from("unused-plan.json"),
        )
        .expect("execCli_ equivalent discards stdout");

        let lifted = lift_io_annotated::<()>(Err(std::io::Error::other("boom")))
            .expect_err("wraps IO errors");
        assert!(format!("{lifted}").contains("boom"));

        let sprocket = Sprocket {
            base: "/tmp/testnet".to_string(),
            name: "socket/node-1".to_string(),
        };
        let config = mk_exec_config("/tmp/testnet", &sprocket, 7);
        let env = config.env.expect("mkExecConfig sets env");
        assert_eq!(
            &env[..2],
            [
                (
                    "CARDANO_NODE_SOCKET_PATH".to_string(),
                    "/tmp/testnet/socket/node-1".to_string(),
                ),
                ("CARDANO_NODE_NETWORK_ID".to_string(), "7".to_string()),
            ]
        );
        assert_eq!(config.cwd, Some(PathBuf::from("/tmp/testnet")));
    }
}
