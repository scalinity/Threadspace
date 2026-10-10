//! `threadspace-m0c`: the M0C qualification runner. Each subcommand exercises
//! one gate area against the installed artifacts and writes a self-contained
//! evidence run under `evidence/M0C/<area>/<UTC>-<label>/` (summary.json plus
//! raw records). A check that cannot run says so; nothing is reported as
//! passed without its evidence.
//!
//!   threadspace-m0c env <prod|dev>
//!   threadspace-m0c install <prod|dev> <built-bundle>
//!   threadspace-m0c g02-packaged <prod|dev> [count]
//!   threadspace-m0c g02-dev dev [count]
//!   threadspace-m0c g03-ipc <prod|dev> [rounds]
//!   threadspace-m0c g04-stream <prod|dev> [count] [duration-ms]
//!   threadspace-m0c view-recovery <prod|dev> [repeats]
//!   threadspace-m0c c04-verdict <prod|dev> <retained view-recovery run dir>
//!   threadspace-m0c g09-companion <prod|dev> [crashes]
//!   threadspace-m0c g10-live <prod|dev> [rounds]
//!   threadspace-m0c g11-restarts <prod|dev> [cycles]
//!   threadspace-m0c maintenance <prod|dev>
//!   threadspace-m0c g08-terminal <prod|dev>
//!   threadspace-m0c m2-cycles <prod|dev> [count]       # M2: integration install/reinstall/remove
//!   threadspace-m0c m2-vertical <prod|dev> [cycles]    # M2: observed session, Return, follow-up
//!   threadspace-m0c m2-routes <prod|dev> [count]       # M2: exact Returns through the UI
//!   threadspace-m0c m2-minimized dev [count>=5]        # F3: owned minimized positive and safe negative
//!   threadspace-m0c m2-faults <prod|dev> [all|case,...] # M2: fault and boundary cases
//!   threadspace-m0c g06-notifications prod
//!   threadspace-m0c g05-denied dev
//!   threadspace-m0c g13-renderer prod
//!   threadspace-m0c g15-graphics prod [minutes]
//!   threadspace-m0c g16-window prod
//!   threadspace-m0c g12-sleep-wake prod [cycles]
//!   threadspace-m0c wake-schedule prod
//!   threadspace-m0c motion-fixture prod
//!   threadspace-m0c clear-notifications <prod|dev>
//!   threadspace-m0c resolve-qualification <prod|dev> [reason]
//!   threadspace-m0c view-command <prod|dev> <command> [json-args]
//!   threadspace-m0c c02-supervision <prod|dev> [all|A,B,C,D,E]
//!   threadspace-m0c h10-terminal <prod|dev>
//!   threadspace-m0c h11-graphics prod
//!   threadspace-m0c h12-voiceover prod

mod bridge_gates;
mod cleanup;
mod ctx;
mod deadline;
mod graphics;
mod graphics_overlap;
mod handoff;
mod install;
mod launches;
mod m2;
mod m2_faults;
mod m2_latency;
mod m2_minimized;
mod notifications;
mod ownership;
mod power;
mod service_gates;
mod supervision;
mod terminal_gates;
mod voiceover;
mod window_gates;

use std::process::ExitCode;

use serde_json::json;

use ctx::Ctx;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        eprintln!("usage: threadspace-m0c <command> ...; see the module header");
        return ExitCode::from(64);
    };
    // Refuse before Ctx initialization can inspect any production surface.
    if command == "m2-minimized" && args.get(1).map(String::as_str) != Some("dev") {
        eprintln!("m2-minimized requires the explicit dev channel");
        return ExitCode::from(64);
    }
    if command == "m2-minimized"
        && (args.len() > 3 || args.get(2).is_some_and(|text| {
            text.parse::<u32>().map_or(true, |count| !(5..=100).contains(&count))
        }))
    {
        eprintln!("m2-minimized accepts one optional repetition count from 5 through 100");
        return ExitCode::from(64);
    }
    let ctx = match Ctx::new(args.get(1).map(String::as_str).unwrap_or("prod")) {
        Ok(ctx) => ctx,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(64);
        }
    };
    let outcome = match command {
        "env" => Ok(ctx.environment()),
        "g02-packaged" => launches::packaged(&ctx, number(&args, 2, 10)),
        "g02-dev" => launches::dev(&ctx, number(&args, 2, 10)),
        "g03-ipc" => bridge_gates::ipc(&ctx, number(&args, 2, 1000)),
        "g04-stream" => {
            bridge_gates::stream(&ctx, number(&args, 2, 10_000), number(&args, 3, 60_000))
        }
        "view-recovery" => bridge_gates::recovery(&ctx, number(&args, 2, 10)),
        "c04-verdict" => bridge_gates::c04_verdict(args.get(2).map_or("", String::as_str)),
        "g09-companion" => service_gates::companion_independence(&ctx, number(&args, 2, 3)),
        "g10-live" => service_gates::sqlite_live(&ctx, number(&args, 2, 5)),
        "g11-restarts" => service_gates::restarts(&ctx, number(&args, 2, 10)),
        "maintenance" => service_gates::maintenance(&ctx),
        "c02-supervision" => {
            supervision::c02(&ctx, args.get(2).map(String::as_str).unwrap_or("all"))
        }
        "g08-terminal" => terminal_gates::negatives(&ctx),
        "m2-cycles" => m2::cycles(&ctx, number(&args, 2, 10)),
        "m2-vertical" => m2::vertical(&ctx, number(&args, 2, 10)),
        "m2-routes" => m2::routes(&ctx, number(&args, 2, 30)),
        "m2-minimized" => m2_minimized::qualify(&ctx, number(&args, 2, 5)),
        "m2-faults" => m2_faults::faults(&ctx, args.get(2).map_or("all", String::as_str)),
        "m2-latency-begin" => m2_latency::begin(&ctx),
        "m2-latency-end" => m2_latency::end(&ctx, std::path::Path::new(args.get(2).map_or("", String::as_str))),
        "h10-terminal" => terminal_gates::remediation(&ctx),
        "c11-deadline" => deadline::c11(&ctx),
        "c02-durable" => ownership::c02_durable(&ctx, args.get(2).map_or("all", String::as_str)),
        "c02-consume-retry" => ownership::c02_durable(&ctx, "D"),
        "c11-receipt" => ownership::c11_receipt(&ctx),
        "c02-handoff" => handoff::c02_handoff(&ctx, args.get(2).map_or("all", String::as_str)),
        "h11-graphics" => graphics_overlap::overlap(&ctx),
        "h12-voiceover" => voiceover::smoke(&ctx),
        "g06-notifications" => notifications::lifecycle(&ctx),
        "g05-denied" => notifications::denied(&ctx),
        "g13-renderer" => graphics::renderer(&ctx),
        "g15-graphics" => graphics::sustained(&ctx, u64::from(number(&args, 2, 15))),
        "g16-window" => window_gates::matrix(&ctx),
        "g12-sleep-wake" => power::cycles(&ctx, number(&args, 2, 5)),
        "wake-schedule" => Ok(json!({ "futureWakes": power::scheduled_wakes() })),
        "motion-fixture" => {
            let mut raised = Vec::new();
            let centre = graphics::keep_centre_animated(&ctx, &mut raised);
            Ok(json!({ "centre": centre, "raised": raised }))
        }
        "clear-notifications" => cleanup::clear_notifications(&ctx),
        "resolve-qualification" => cleanup::resolve_qualification(
            &ctx,
            args.get(2)
                .map(String::as_str)
                .unwrap_or("qualification cleanup"),
        ),
        "synthetic" => ctx
            .companion()
            .request(
                threadspace_contracts::control::ControlRequestBody::QualifySyntheticChanges {
                    count: number(&args, 2, 60),
                    duration_ms: number(&args, 3, 2000),
                    sessions: 4,
                },
                std::time::Duration::from_secs(10),
            )
            .map(|reply| json!(format!("{reply:?}"))),
        "view-command" => match args.get(2) {
            Some(command) => {
                let input = args
                    .get(3)
                    .and_then(|text| serde_json::from_str(text).ok())
                    .unwrap_or(json!({}));
                ctx.app()
                    .view_command(command, input, std::time::Duration::from_secs(900))
            }
            None => Err("view-command needs a command".into()),
        },
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

fn number(args: &[String], index: usize, default: u32) -> u32 {
    args.get(index)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
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
