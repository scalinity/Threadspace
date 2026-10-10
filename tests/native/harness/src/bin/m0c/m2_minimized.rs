//! F3 qualification only: repeated minimized-window Returns in the Dev app.
//! The product's one 2,000 ms deadline is unchanged. Native setup, independent
//! readback and cleanup have separate records and never turn a failed Return
//! into a pass. No native result is supplied by the portable oracle tests.

#[path = "m2_minimized_oracle.rs"]
mod oracle;

use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_harness::evidence::{Run, sha256_file};
use threadspace_harness::idle::wait_for_idle;
use threadspace_harness::procs;
use threadspace_harness::run::{Output, osascript, run};
use threadspace_harness::terminal::{self, Tab};
use threadspace_provider_claude::discovery::discover;
use threadspace_provider_claude::inventory::{ClaudeCli, ClaudeInstall};
use threadspace_provider_claude::profiles::version_from_executable;
use threadspace_surfaces_macos::ancestry::KernelSampler;
use threadspace_surfaces_macos::{process, tty};

use crate::ctx::Ctx;
use crate::m2::{self, Scratch};
use crate::terminal_gates::{StartedClaude, frontmost_bundle, raise_window, window_selection};

const TERMINAL_READ_BUDGET: Duration = Duration::from_secs(3);
const WINDOW_STATE: &str = r#"on run argv
set sep to character id 9
tell application "Terminal"
  repeat with w in windows
    if (id of w) is ((item 1 of argv) as integer) then
      set matched to 0
      repeat with t in tabs of w
        if tty of t is (item 2 of argv) then set matched to matched + 1
      end repeat
      return "FOUND" & sep & ((miniaturized of w) as text) & sep & ((count of tabs of w) as text) & sep & (matched as text)
    end if
  end repeat
end tell
return "MISSING"
end run"#;

// Cleanup checks the recorded window/TTY and refuses a new tab in that
// window. Values are argv. This never selects or types into any tab.
const CLOSE_EMPTY_OWNED_WINDOW: &str = r#"on run argv
tell application "Terminal"
  repeat with w in windows
    if (id of w) is ((item 1 of argv) as integer) then
      if (count of tabs of w) is not 1 then return "REFUSED"
      if tty of tab 1 of w is not (item 2 of argv) then return "REFUSED"
      close w
      return "CLOSED"
    end if
  end repeat
end tell
return "MISSING"
end run"#;

fn output(value: &Output) -> Value {
    json!({ "ok": value.ok, "status": value.status, "timedOut": value.timed_out,
        "elapsedMs": value.elapsed_ms, "senderPid": value.pid, "stdout": value.stdout, "stderr": value.stderr })
}

fn target_state(tab: &Tab) -> Value {
    let started_ms = threadspace_harness::now_ms();
    let out = osascript(
        WINDOW_STATE,
        &[&tab.window_id.to_string(), &tab.tty],
        TERMINAL_READ_BUDGET,
    );
    let fields: Vec<_> = out.stdout.trim().split('\t').collect();
    let mut record = json!({ "startedMs": started_ms, "endedMs": threadspace_harness::now_ms(),
        "ok": false, "raw": output(&out) });
    if out.ok && fields == ["MISSING"] {
        record["missing"] = json!(true);
    }
    if out.ok
        && fields.len() == 4
        && fields[0] == "FOUND"
        && matches!(fields[1], "true" | "false")
        && let (Ok(tabs), Ok(matches)) = (fields[2].parse::<u32>(), fields[3].parse::<u32>())
    {
        record["ok"] = json!(true);
        record["miniaturized"] = json!(fields[1] == "true");
        record["tabCount"] = json!(tabs);
        record["matchingTtyCount"] = json!(matches);
    }
    record
}

fn readback(tab: &Tab) -> Value {
    let started_ms = threadspace_harness::now_ms();
    let selected = window_selection();
    let frontmost = frontmost_bundle();
    let state = target_state(tab);
    json!({ "startedMs": started_ms, "endedMs": threadspace_harness::now_ms(),
        "selection": selected, "frontmostBundle": frontmost, "targetState": state,
        "note": "Independent read-only witness after the completed attempt; no retry can repair the product result or deadline." })
}

fn sample(incarnation: &process::Incarnation) -> Value {
    let s = &incarnation.sample;
    json!({ "pid": s.pid, "ppid": s.ppid, "startSeconds": s.start_seconds.to_string(),
        "startMicroseconds": s.start_microseconds, "executable": incarnation.executable.canonical(),
        "controllingDevice": s.controlling_device, "pgid": s.pgid, "tpgid": s.tpgid,
        "status": s.status, "comm": s.comm })
}

/// Cleanup needs a complete successful PID inventory. The shared convenience
/// helper returns an empty vector on `ps` failure, which is not emptiness
/// proof. A nonzero/failed read stays unknown and cannot authorize a close.
fn cleanup_tty_pids(tab: &Tab) -> (Option<Vec<i32>>, Value) {
    let out = run(
        "/bin/ps",
        &["-t", tab.tty.trim_start_matches("/dev/"), "-o", "pid="],
        TERMINAL_READ_BUDGET,
    );
    let parsed = out
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().parse::<i32>().ok().filter(|pid| *pid > 1))
        .collect::<Option<Vec<_>>>();
    let pids = if out.ok && !out.timed_out {
        parsed
    } else {
        None
    };
    (pids, output(&out))
}

fn cli() -> Result<ClaudeCli, String> {
    let home = threadspace_relay::paths::home_dir().ok_or("no home")?;
    let install =
        ClaudeInstall::resolve(&m2::launcher()?).ok_or("Claude launcher cannot be resolved")?;
    Ok(ClaudeCli {
        binary: install.binary,
        home,
        timeout: Duration::from_secs(3),
        now_ms: threadspace_harness::now_ms,
    })
}

/// Independent native evidence. The locator selects candidates here only
/// after stat proves a character device and production discovery has joined
/// the full Session to a birth/image-stable process across two inventories.
/// Cwd and titles never establish Session identity or authorize Return.
fn native_witness(tab: &Tab, endpoint: &str, expected_session: Option<&str>) -> Value {
    let started_ms = threadspace_harness::now_ms();
    let attempt = || -> Result<Value, String> {
        let device = tty::character_device(&tab.tty).map_err(|error| error.to_string())?;
        let boot = process::boot_session_id().map_err(|error| error.to_string())?;
        let inventory = cli()?;
        let pass = discover(&inventory, &KernelSampler, |image| {
            version_from_executable(image).as_deref() == Some("2.1.295")
        })
        .map_err(|error| error.to_string())?;
        let joins: Vec<_> = pass
            .joins
            .iter()
            .filter(|join| {
                join.device == device
                    && expected_session.is_none_or(|id| id == join.native_session_id)
            })
            .collect();
        let [join] = joins.as_slice() else {
            return Err(format!(
                "expected exactly one qualified Session/process on the owned character device; got {}",
                joins.len()
            ));
        };
        if pass
            .joins
            .iter()
            .filter(|other| other.native_session_id == join.native_session_id)
            .count()
            != 1
        {
            return Err("native Session has multiple process attachments".into());
        }
        let second = pass.second.as_ref().ok_or("second inventory missing")?;
        let current_device = tty::character_device(&tab.tty).map_err(|error| error.to_string())?;
        if current_device != device || endpoint.is_empty() {
            return Err("device changed or local canonical endpoint missing".into());
        }
        Ok(
            json!({ "qualified": true, "nativeSessionId": join.native_session_id,
            "processKey": { "endpointId": endpoint, "bootId": boot, "pid": join.pid,
                "startSeconds": join.after.sample.start_seconds.to_string(), "startMicroseconds": join.after.sample.start_microseconds },
            "executable": join.after.executable.canonical(), "runningVersion": version_from_executable(&join.after.executable.path),
            "tty": tab.tty, "rdev": device, "before": sample(&join.before), "after": sample(&join.after),
            "firstInventory": pass.first.evidence_for(join.pid, &join.native_session_id),
            "confirmingInventory": second.evidence_for(join.pid, &join.native_session_id),
            "lookupBinary": inventory.binary, "evidenceSource": "production discover + KernelSampler + character_device; independent of route result" }),
        )
    };
    let mut record =
        attempt().unwrap_or_else(|error| json!({ "qualified": false, "error": error }));
    record["startedMs"] = json!(started_ms);
    record["endedMs"] = json!(threadspace_harness::now_ms());
    record
}

struct OwnedTab {
    tab: Tab,
    directory: (u64, u64),
    terminal: procs::Incarnation,
    device: u32,
}

impl OwnedTab {
    fn acquire(tab: Tab) -> Result<Self, String> {
        let metadata = std::fs::symlink_metadata(&tab.dir).map_err(|error| error.to_string())?;
        if !metadata.is_dir() {
            return Err("new fixture directory is not an owned directory".into());
        }
        let terminals = terminal::terminal_process();
        let [generation] = terminals.as_slice() else {
            return Err("Terminal incarnation is ambiguous".into());
        };
        let device = tty::character_device(&tab.tty).map_err(|error| error.to_string())?;
        Ok(Self {
            tab,
            directory: (metadata.dev(), metadata.ino()),
            terminal: generation.clone(),
            device,
        })
    }

    fn ownership(&self) -> Value {
        let metadata_matches = std::fs::symlink_metadata(&self.tab.dir).is_ok_and(|metadata| {
            metadata.is_dir() && (metadata.dev(), metadata.ino()) == self.directory
        });
        let generation_matches = terminal::terminal_process().as_slice() == [self.terminal.clone()];
        let device_matches = tty::character_device(&self.tab.tty).ok() == Some(self.device);
        let state = target_state(&self.tab);
        let owned = self.tab.owned();
        let checks = json!({ "directoryIncarnation": metadata_matches, "terminalIncarnation": generation_matches,
            "characterDevice": device_matches, "recordedWindowAndSoleTab": state["ok"] == true
                && state["tabCount"] == 1 && state["matchingTtyCount"] == 1,
            "ownedWorkingDirectoryProcess": owned["owned"] == true });
        json!({ "owned": oracle::all_true(&checks), "checks": checks, "tab": self.tab,
            "terminalIncarnation": self.terminal, "device": self.device, "windowState": state, "helperOwnership": owned })
    }

    /// No pattern signals, no borrowed PID authority, no broad Terminal close.
    /// Each signal needs a fresh same-incarnation/image/device/cwd proof.
    /// Unverifiable cleanup retains the scratch acquisition for diagnosis.
    fn cleanup(&self, ctx: &Ctx) -> Value {
        let guard = match ctx.gui("m2 minimized owned cleanup") {
            Ok(guard) => guard,
            Err(error) => return json!({ "closed": false, "refused": error }),
        };
        let _guard = guard;
        let initial = self.ownership();
        if initial["owned"] != true {
            // A previously closed window is already complete, but only while
            // the same Terminal incarnation still exists.
            let gone = initial["checks"]["terminalIncarnation"] == true
                && initial["checks"]["directoryIncarnation"] == true
                && initial["windowState"]["missing"] == true;
            return json!({ "closed": gone, "ownership": initial, "refused": (!gone).then_some("ownership unproven") });
        }
        let directory = self.tab.dir.canonicalize().ok();
        let mut captured = Vec::new();
        let (pids, pid_inventory) = cleanup_tty_pids(&self.tab);
        let Some(pids) = pids else {
            return json!({ "closed": false, "refused": "TTY PID inventory failed; no signal or close sent",
                "pidInventory": pid_inventory, "ownership": initial });
        };
        for pid in pids {
            let Some(incarnation) = process::sample_incarnation(pid).ok() else {
                return json!({ "closed": false, "refused": "process capture unavailable", "pid": pid, "ownership": initial });
            };
            let cwd = procs::cwd(pid).and_then(|path| PathBuf::from(path).canonicalize().ok());
            if directory.is_none()
                || cwd != directory
                || incarnation.sample.controlling_device != Some(self.device)
            {
                return json!({ "closed": false, "refused": "a current TTY process is not proven to belong to this disposable directory", "pid": pid, "ownership": initial });
            }
            captured.push(incarnation);
        }
        let mut signals = Vec::new();
        for before in captured {
            let pid = before.sample.pid;
            let now = match process::sample_incarnation(pid) {
                Ok(now) => now,
                Err(process::ProcessError::Vanished { .. }) => continue,
                Err(error) => {
                    return json!({ "closed": false, "refused": error.to_string(), "signals": signals, "ownership": initial });
                }
            };
            let cwd = procs::cwd(pid).and_then(|path| PathBuf::from(path).canonicalize().ok());
            if !before.same_process_and_image(&now)
                || now.sample.controlling_device != Some(self.device)
                || cwd != directory
                || self.ownership()["owned"] != true
            {
                return json!({ "closed": false, "refused": "incarnation/image/TTY/directory changed before signal", "signals": signals, "ownership": initial });
            }
            // The ownership/cwd reads above can take time. Re-sample after
            // them, immediately before signalling this exact incarnation.
            let last = match process::sample_incarnation(pid) {
                Ok(last)
                    if before.same_process_and_image(&last)
                        && last.sample.controlling_device == Some(self.device) =>
                {
                    last
                }
                Err(process::ProcessError::Vanished { .. }) => continue,
                _ => {
                    return json!({ "closed": false, "refused": "process changed at final signal boundary", "signals": signals, "ownership": initial });
                }
            };
            let signalled = procs::signal(pid, libc::SIGHUP);
            signals.push(json!({ "before": sample(&before), "verifiedBeforeSignal": sample(&last), "signal": "SIGHUP", "sent": signalled }));
            if !signalled {
                return json!({ "closed": false, "refused": "owned process signal failed", "signals": signals, "ownership": initial });
            }
        }
        threadspace_harness::pause_ms(800);
        let current = target_state(&self.tab);
        if current["missing"] == true {
            return json!({ "closed": true, "alreadyClosedAfterExit": true, "signals": signals, "ownership": initial, "after": current });
        }
        let (remaining, remaining_inventory) = cleanup_tty_pids(&self.tab);
        if terminal::terminal_process().as_slice() != [self.terminal.clone()]
            || !remaining.as_ref().is_some_and(Vec::is_empty)
            || current["ok"] != true
            || current["tabCount"] != 1
            || current["matchingTtyCount"] != 1
        {
            return json!({ "closed": false, "refused": "window/process state changed, TTY still occupied or PID inventory unavailable; no close sent", "signals": signals, "ownership": initial,
                "pidInventory": pid_inventory, "remainingPidInventory": remaining_inventory, "after": current });
        }
        let closed = osascript(
            CLOSE_EMPTY_OWNED_WINDOW,
            &[&self.tab.window_id.to_string(), &self.tab.tty],
            TERMINAL_READ_BUDGET,
        );
        let after = target_state(&self.tab);
        json!({ "closed": closed.ok && after["missing"] == true,
            "signals": signals, "ownership": initial, "pidInventory": pid_inventory,
            "remainingPidInventory": remaining_inventory, "close": output(&closed), "after": after })
    }
}

fn scrub(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(threadspace_relay::paths::redact_home(&text)),
        Value::Array(values) => Value::Array(values.into_iter().map(scrub).collect()),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, scrub(value)))
                .collect(),
        ),
        value => value,
    }
}

fn write(run: &Run, name: &str, value: &Value) -> Result<(), String> {
    run.write_json(name, &scrub(value.clone()))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn append(run: &Run, value: &Value) -> Result<(), String> {
    run.append("cases.jsonl", &scrub(value.clone()))
        .map_err(|error| error.to_string())
}

fn source_identity(ctx: &Ctx) -> Value {
    let sources = [
        "tests/native/harness/src/bin/m0c/m2_minimized.rs",
        "tests/native/harness/src/bin/m0c/m2_minimized_oracle.rs",
        "tests/native/harness/src/bin/m0c/deadline.rs",
        "tests/native/harness/src/bin/m0c/terminal_gates.rs",
        "tests/native/harness/src/terminal.rs",
        "apps/agent-macos/core/src/route.rs",
        "crates/surfaces/src/lib.rs",
        "apps/agent-macos/Resources/terminal-focus.applescript",
        "apps/agent-macos/Resources/terminal-inventory.applescript",
    ];
    let binary = std::env::current_exe().ok();
    let git = |args: &[&str]| {
        run("/usr/bin/git", args, Duration::from_secs(5))
            .stdout
            .trim()
            .to_owned()
    };
    let build_info = std::fs::read(ctx.repo.join("apps/agent-macos/build/dev/build-info.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    json!({ "harnessCommit": git(&["rev-parse", "HEAD"]), "worktreeStatus": git(&["status", "--porcelain"]),
        "harnessExecutable": binary, "harnessExecutableSha256": binary.as_deref().and_then(sha256_file),
        "sourceFiles": sources.into_iter().map(|path| json!({ "path": path, "sha256": sha256_file(&ctx.repo.join(path)) })).collect::<Vec<_>>(),
        "devBuildInfo": build_info, "installed": ctx.environment(),
        "helperSha256": ctx.id.companion_executable.parent().and_then(|path| sha256_file(&path.join("threadspace-hook"))),
        "sourceAttribution": "Build-info is retained as a claim; installed executable hashes must be matched to the reviewed build record before independent acceptance." })
}

/// Native-only entry point. An explicit `dev` is mandatory at dispatch too.
pub fn qualify(ctx: &Ctx, repetitions: u32) -> Result<Value, String> {
    if ctx.channel_name() != "dev" || ctx.id.app_identifier != "ai.scalinity.threadspace.dev" {
        return Err("m2-minimized is Dev-only; production use is refused".into());
    }
    if !(5..=100).contains(&repetitions) {
        return Err(
            "m2-minimized needs 5..=100 repetitions; no positive-case count is inferred".into(),
        );
    }
    let run = Run::create(
        &ctx.repo.join("evidence/M2/remediation-1/f3"),
        "native",
        &format!("dev-{}", uuid::Uuid::new_v4()),
    )
    .map_err(|error| error.to_string())?;
    write(&run, "source-identity.json", &source_identity(ctx))?;
    let mut scratch = Scratch::new(ctx, "f3-minimized")?;
    let mut owned: Vec<OwnedTab> = Vec::new();
    let mut uncertain_open = false;
    let mut cases = Vec::new();
    let endpoint_rows: Value = serde_json::from_str(&m2::journal_query(
        ctx,
        "SELECT value AS endpointId FROM store_meta WHERE key = 'endpoint_id'",
    )?)
    .map_err(|error| error.to_string())?;
    let endpoint = endpoint_rows[0]["endpointId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("Dev journal has no local endpoint identity")?
        .to_owned();

    let operation = (|| -> Result<(), String> {
        let activation = m2::activate(ctx, &mut scratch)?;
        write(&run, "integration-acquisition.json", &activation.record)?;
        let idle = wait_for_idle(&ctx.native, 10.0, Duration::from_secs(180));
        write(&run, "idle-gate.json", &json!(idle))?;
        if !idle.satisfied {
            return Err("owner-idle gate did not pass; no Terminal fixture opened".into());
        }
        let wait_started = Instant::now();
        let scripting = terminal::wait_scriptable(m2::TERMINAL_WAIT);
        write(
            &run,
            "terminal-setup-readiness.json",
            &json!({ "elapsedMs": wait_started.elapsed().as_millis(),
            "result": scripting.as_ref().ok(), "error": scripting.as_ref().err(), "refusalCounters": terminal::refusals() }),
        )?;
        scripting?;
        let target_dir = scratch.dir.join("target");
        std::fs::create_dir(&target_dir).map_err(|error| error.to_string())?;
        let command = format!(
            "cd {} && export CLAUDE_CODE_PLUGIN_DIRS={} && exec {} --settings {} --model {}",
            terminal::shell_quote(&target_dir.display().to_string()),
            terminal::shell_quote(&activation.plugin_dir),
            terminal::shell_quote(&m2::launcher()?.display().to_string()),
            terminal::shell_quote(&activation.settings.display().to_string()),
            m2::MODEL
        );
        {
            let _gui = ctx.gui("m2 minimized launch owned Claude")?;
            // On a malformed open receipt the fixture may exist without a
            // proven window handle. Retain the scratch, never broad cleanup.
            uncertain_open = true;
            let tab = Tab::open(target_dir, &command)?;
            let resource = OwnedTab::acquire(tab)?;
            owned.push(resource);
            uncertain_open = false;
            threadspace_harness::pause_ms(4000);
            let ownership = owned[0].ownership();
            write(&run, "launch-ownership.json", &ownership)?;
            if ownership["owned"] != true {
                return Err("new Claude window ownership unproven; no input sent".into());
            }
            // Folder trust is setup for this disposable directory only.
            if !owned[0].tab.type_line("\u{1b}[B") {
                return Err("owned folder-trust setup refused".into());
            }
        }
        let started = Instant::now();
        let first = loop {
            let witness = native_witness(&owned[0].tab, &endpoint, None);
            run.append("discovery-attempts.jsonl", &scrub(witness.clone()))
                .map_err(|error| error.to_string())?;
            if witness["qualified"] == true {
                break witness;
            }
            if started.elapsed() >= Duration::from_secs(60) {
                return Err("no qualified real Claude 2.1.295 Session/process/TTY join".into());
            }
            threadspace_harness::pause_ms(1000);
        };
        let native_id = first["nativeSessionId"]
            .as_str()
            .ok_or("native identity missing")?
            .to_owned();
        let started = StartedClaude {
            tab: owned[0].tab.clone(),
            native_session_id: native_id.clone(),
            pid: first["processKey"]["pid"]
                .as_i64()
                .ok_or("native PID missing")? as i32,
        };
        let session = started
            .bound_session(ctx, Duration::from_secs(90))
            .ok_or("Dev companion never bound the actual Session to the owned TTY")?;
        let view = m2::session_view(ctx, &native_id)?;
        if view.session_id != session {
            return Err("canonical Session changed during setup".into());
        }
        write(&run, "canonical-session-at-start.json", &json!(view))?;
        let spare_dir = scratch.dir.join("spare");
        std::fs::create_dir(&spare_dir).map_err(|error| error.to_string())?;
        {
            let _gui = ctx.gui("m2 minimized launch owned spare")?;
            uncertain_open = true;
            owned.push(OwnedTab::acquire(Tab::open_inert(spare_dir)?)?);
            uncertain_open = false;
        }
        let target = &owned[0];
        let spare = &owned[1];
        let expected = json!({ "sessionId": session, "windowId": target.tab.window_id, "native": first,
            "otherWindowId": spare.tab.window_id, "otherTty": spare.tab.tty });
        write(
            &run,
            "fixture.json",
            &json!({ "expected": expected, "target": target.tab, "spare": spare.tab,
            "launch": "independently started in a new ordinary Terminal window; no Threadspace provider-launch command", "modelTurnsRequested": 0 }),
        )?;

        for index in 1..=repetitions {
            let idle = wait_for_idle(&ctx.native, 10.0, Duration::from_secs(180));
            if !idle.satisfied {
                return Err(format!("owner-idle gate failed before repetition {index}"));
            }
            let _gui = ctx.gui("m2 minimized positive setup and explicit Return")?;
            let target_owned = target.ownership();
            let spare_owned = spare.ownership();
            let native = native_witness(&target.tab, &endpoint, Some(&native_id));
            if target_owned["owned"] != true
                || spare_owned["owned"] != true
                || !oracle::same_identity(&expected["native"], &native)
            {
                let failed = json!({ "case": "minimize-then-return", "repetition": index, "pass": false,
                    "explicitReturn": false, "expected": expected, "refused": "owned resource or original native identity no longer proven",
                    "preconditions": { "targetOwnership": target_owned, "spareOwnership": spare_owned, "native": native } });
                append(&run, &failed)?;
                cases.push(failed);
                break;
            }
            // Retain the original G08 moved-window geometry, then prove an
            // owned spare is foreground and the target definitely minimized.
            let moved_bounds = target.tab.set_bounds(120, 140, 900, 640);
            let spare_selected = spare.tab.select();
            let spare_raised = raise_window(spare.tab.window_id);
            let minimized = target.tab.set_miniaturized(true);
            threadspace_harness::pause_ms(1000);
            let native_before_setup = native;
            let native = native_witness(&target.tab, &endpoint, Some(&native_id));
            let before = readback(&target.tab);
            let pre_checks = json!({ "targetOwned": target_owned["owned"] == true, "spareOwned": spare_owned["owned"] == true,
                "inheritedMovedBoundsSetup": moved_bounds,
                "sameNativeProcessSessionAndTty": oracle::same_identity(&expected["native"], &native),
                "minimizeCommandReadback": minimized == Some(true), "targetDefinitelyMinimized": before["targetState"]["ok"] == true
                    && before["targetState"]["miniaturized"] == true,
                "otherWindowDefinitelyForeground": spare_selected && spare_raised && before["selection"]["error"].is_null()
                    && before["selection"]["front"] == spare.tab.window_id
                    && before["selection"]["selected"][spare.tab.window_id.to_string()] == spare.tab.tty
                    && before["frontmostBundle"] == "com.apple.Terminal" });
            let mut case = json!({ "case": "minimize-then-return", "repetition": index, "expected": expected,
                "preconditions": { "checks": pre_checks, "targetOwnership": target_owned, "spareOwnership": spare_owned,
                    "native": native, "nativeBeforeSetup": native_before_setup,
                    "readback": before, "idleGate": idle, "requestedBounds": [120, 140, 900, 640],
                    "setupSettleMs": 1000 }, "explicitReturn": false, "pass": false });
            if oracle::all_true(&pre_checks) {
                case["explicitReturn"] = json!(true);
                // Includes all product evidence, including failure evidence.
                case["route"] = crate::deadline::route_full(ctx, &session);
                case["independent"] = readback(&target.tab);
                case["independentCurrent"] =
                    native_witness(&target.tab, &endpoint, Some(&native_id));
                case["sideEffects"] = json!({
                    "focusAppleEventReported": case["route"]["result"]["focusPerformed"],
                    "frontWindowBefore": before["selection"]["front"], "frontWindowAfter": case["independent"]["selection"]["front"],
                    "targetMiniaturizedBefore": before["targetState"]["miniaturized"],
                    "targetMiniaturizedAfter": case["independent"]["targetState"]["miniaturized"],
                    "selectedTabsBefore": before["selection"]["selected"], "selectedTabsAfter": case["independent"]["selection"]["selected"],
                    "unrelatedSelectionChanges": oracle::unrelated_changes(&before["selection"], &case["independent"]["selection"], &[target.tab.window_id, spare.tab.window_id]),
                    "interpretation": "Reported focus and actual readback are retained on every outcome. TIMEOUT does not prove no focus side effect." });
            } else {
                case["refused"] = json!(
                    "minimized and alternate-foreground preconditions not established; no Return issued"
                );
            }
            let checks = oracle::positive_checks(&case);
            case["pass"] = json!(oracle::all_true(&checks));
            case["checks"] = checks;
            append(&run, &case)?;
            cases.push(case);
        }

        // A safe conservative negative: an independently verified absent
        // canonical Session cannot focus any window. No fabricated identity,
        // process suspension, provider switch or unrelated window is needed.
        let _gui = ctx.gui("m2 minimized unknown-session conservative negative")?;
        if target.ownership()["owned"] != true || spare.ownership()["owned"] != true {
            return Err(
                "ownership changed before unknown-session negative; negative not run".into(),
            );
        }
        let absent = uuid::Uuid::new_v4().to_string();
        let rows: Value = serde_json::from_str(&m2::journal_query(
            ctx,
            &format!("SELECT id FROM sessions WHERE id = '{absent}'"),
        )?)
        .map_err(|error| error.to_string())?;
        let absent_verified = rows.as_array().is_some_and(Vec::is_empty);
        let before = readback(&target.tab);
        let mut negative = json!({ "case": "unknown-session-refusal", "sessionId": absent,
            "sessionAbsent": absent_verified, "explicitReturn": false, "before": before });
        if absent_verified {
            negative["explicitReturn"] = json!(true);
            negative["route"] = crate::deadline::route_full(ctx, &absent);
            negative["after"] = readback(&target.tab);
        }
        let checks = oracle::negative_checks(&negative);
        negative["pass"] = json!(oracle::all_true(&checks));
        negative["checks"] = checks;
        append(&run, &negative)?;
        cases.push(negative);
        Ok(())
    })();

    let cleanup: Vec<Value> = owned
        .iter()
        .rev()
        .map(|resource| resource.cleanup(ctx))
        .collect();
    let all_closed = !uncertain_open && cleanup.iter().all(|entry| entry["closed"] == true);
    let retained = (!all_closed).then(|| scratch.dir.clone());
    let integration_removed = if all_closed {
        Some(scratch.remove_integration())
    } else {
        None
    };
    let integration_closed = integration_removed.as_ref().is_some_and(|result| {
        result.as_ref().is_ok_and(|value| {
            value["ok"] == true
                && (value["detail"]["complete"] == true || value["detail"]["installed"] == false)
        })
    });
    let cleanup_record = json!({ "windows": cleanup, "uncertainOpen": uncertain_open,
        "retainedScratch": retained, "integrationRemoval": integration_removed.as_ref().map(|result| match result {
            Ok(value) => value.clone(), Err(error) => json!({ "error": error }) }),
        "allOwnedResourcesClosed": all_closed && integration_closed });
    if !all_closed {
        std::mem::forget(scratch);
    }
    write(&run, "cleanup.json", &cleanup_record)?;
    let positive_count = cases
        .iter()
        .filter(|case| case["case"] == "minimize-then-return" && case["pass"] == true)
        .count();
    let routed: Vec<_> = cases
        .iter()
        .filter(|case| case["case"] == "minimize-then-return" && case["explicitReturn"] == true)
        .collect();
    let final_surface_matches = |case: &&Value| {
        case["independent"]["selection"]["front"] == case["expected"]["windowId"]
            && case["independent"]["selection"]["selected"]
                .get(case["expected"]["windowId"].to_string())
                == Some(&case["expected"]["native"]["tty"])
            && case["independent"]["frontmostBundle"] == "com.apple.Terminal"
    };
    let final_surface_known = |case: &&Value| {
        case["independent"]["selection"]["front"].as_i64().is_some()
            && case["independent"]["selection"]["error"].is_null()
            && case["independent"]["frontmostBundle"].as_str().is_some()
    };
    let all_pass = operation.is_ok()
        && positive_count == repetitions as usize
        && cases.iter().all(|case| case["pass"] == true)
        && integration_closed
        && all_closed;
    let summary = json!({ "finding": "F3", "pass": all_pass, "status": if all_pass { "FOCUSED_NATIVE_RUN_PASS_PENDING_INDEPENDENT_ADJUDICATION" } else { "F3_STILL_BLOCKING" },
        "runDirectory": run.dir, "requestedPositiveRepetitions": repetitions, "successfulPositiveRepetitions": positive_count,
        "minimumRequiredPositiveRepetitions": 5, "routeDeadlineMs": oracle::BUDGET_MS,
        "requestThroughReceiptBudgetMs": oracle::BUDGET_MS, "cases": cases.len(), "operationError": operation.err(),
        "routeOutcomeAccounting": { "positiveCasesWithReturnIssued": routed.len(),
            "exactClaimsWithDifferentOrUnknownIndependentSurface": routed.iter().filter(|case| case["route"]["result"]["surfaceResult"] == "EXACT_NATIVE_SURFACE" && !final_surface_matches(case)).count(),
            "nonTargetFinalReadbacksIncludingHonestTimeouts": routed.iter().filter(|case| final_surface_known(case) && !final_surface_matches(case)).count(),
            "unknownFinalReadbacks": routed.iter().filter(|case| !final_surface_known(case)).count(),
            "interpretation": "A non-target final readback after TIMEOUT is a failed positive, not automatically a wrong-target route claim or proof that no side effect occurred." },
        "cleanup": cleanup_record, "terminalRefusalCounters": terminal::refusals(),
        "timingCoverage": { "fullProductPhasesRetained": true,
            "finerAppleEventDurations": "UNAVAILABLE in unchanged product: tab selection/restoration/raise/activation/readback share the focus phase",
            "independentReadback": "read-only after returned result, timed separately; cannot extend or repair the route attempt",
            "rootCause": "NOT_INFERRED_BY_RUNNER" },
        "additionalExistingNegativeCommand": "THREADSPACE_EVIDENCE_MILESTONE=M2 threadspace-m0c g08-terminal dev",
        "fullTerminalRestart": "Not executed; retained separate M15 limitation",
        "routeProductCodeChanged": false });
    write(&run, "summary.json", &summary)?;
    Ok(scrub(summary))
}
