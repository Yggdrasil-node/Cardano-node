use eyre::{Result, eyre};
use std::{env, fs};
use yggdrasil_cddl_codegen::{generate_module, parse_schema};

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let flag = args.next();
    let path = args.next();

    match (flag.as_deref(), path.as_deref()) {
        (Some("--spec"), Some(path)) => {
            let schema = fs::read_to_string(path)?;
            let parsed = parse_schema(&schema)?;
            let module = generate_module(&parsed);
            println!("{}", module.source);
            Ok(())
        }
        _ => Err(eyre!("usage: cddl-codegen --spec <path>")),
    }
}
