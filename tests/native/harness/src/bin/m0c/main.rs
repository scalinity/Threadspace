//! `threadspace-m0c`: the M0C qualification runner. Each subcommand exercises
//! one gate area against the installed artifacts and writes a self-contained
//! evidence run under `evidence/M0C/<area>/<UTC>-<label>/` (summary.json plus
//! raw records). A check that cannot run says so; nothing is reported as
//! passed without its evidence.
//!
//!   threadspace-m0c env <prod|dev>
//!   threadspace-m0c install <prod|dev> <built-bundle>

mod ctx;
mod install;

use std::process::ExitCode;

use serde_json::json;

use ctx::Ctx;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        eprintln!("usage: threadspace-m0c <command> ...; see the module header");
        return ExitCode::from(64);
    };
    let ctx = match Ctx::new(args.get(1).map(String::as_str).unwrap_or("prod")) {
        Ok(ctx) => ctx,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(64);
        }
    };
    let outcome = match command {
        "env" => Ok(ctx.environment()),
        "install" => match args.get(2) {
            Some(built) => install::install(&ctx, std::path::Path::new(built), &rollback_root()),
            None => Err("install needs the built bundle path".into()),
        },
        other => Err(format!("unknown command {other}")),
    };
    match outcome {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            );
            if value["pass"] == json!(false) {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

/// UTC stamp for directory names.
pub fn stamp() -> String {
    threadspace_harness::evidence::utc_stamp(threadspace_harness::now_ms())
}

/// Rollback copies of replaced bundles live outside the repository.
fn rollback_root() -> std::path::PathBuf {
    threadspace_relay::paths::home_dir()
        .unwrap_or_default()
        .join("Library/Application Support/threadspace-qualification/rollback")
}
