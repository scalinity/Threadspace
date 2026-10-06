//! Probe context: evidence paths, the disposable Codex home, bounded command
//! records and `$HOME` redaction. Every Codex CLI invocation runs with a
//! cleared environment whose `CODEX_HOME`, `HOME` and `TMPDIR` point into a
//! disposable directory the probe created, and with that directory as its
//! working directory: the released CLI writes `CODEX_HOME/tmp/arg0` on every
//! start, so it is never pointed at the owner's `~/.codex`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use threadspace_surfaces_macos::exec::{BoundedCommand, run_bounded};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// RFC 3339 UTC rendering of epoch milliseconds.
pub fn utc(ms: u64) -> String {
    let seconds = ms / 1000;
    let days = (seconds / 86_400) as i64;
    let rem = seconds % 86_400;
    // Civil-from-days (H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        ms % 1000
    )
}

pub fn stamp(ms: u64) -> Value {
    json!({ "ms": ms, "utc": utc(ms) })
}

pub fn sha256_file(path: &Path) -> Result<(String, u64), String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("{:?}", e.kind()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("{:?}", e.kind()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    Ok((hex(&hasher.finalize()), total))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub struct Disposable {
    pub root: PathBuf,
    pub home: PathBuf,
    pub tmp: PathBuf,
}

/// Output of one bounded command plus its evidence record.
pub struct Ran {
    pub record: Value,
    pub stdout: String,
    pub stderr: String,
    pub ok: bool,
}

pub struct Ctx {
    pub out: PathBuf,
    pub home: String,
    pub codex: PathBuf,
    pub scratch: PathBuf,
    pub hook_schemas: Option<PathBuf>,
    disposable: Option<Disposable>,
    disposable_record: Option<Value>,
    pub stable_schema: Option<(PathBuf, Value)>,
    pub experimental_schema: Option<(PathBuf, Value)>,
}

impl Ctx {
    pub fn new(
        out: PathBuf,
        home: String,
        codex: PathBuf,
        scratch: PathBuf,
        hook_schemas: Option<PathBuf>,
    ) -> Self {
        Self {
            out,
            home,
            codex,
            scratch,
            hook_schemas,
            disposable: None,
            disposable_record: None,
            stable_schema: None,
            experimental_schema: None,
        }
    }

    pub fn owner_path(&self, relative: &str) -> PathBuf {
        Path::new(&self.home).join(relative)
    }

    /// Replaces the owner's home prefix with `~`.
    pub fn redact(&self, text: &str) -> String {
        if self.home.is_empty() || self.home == "/" {
            return text.to_string();
        }
        let prefix = format!("{}/", self.home);
        let replaced = text.replace(&prefix, "~/");
        if replaced == self.home {
            "~".into()
        } else {
            replaced
        }
    }

    pub fn redact_value(&self, value: &mut Value) {
        match value {
            Value::String(text) => *text = self.redact(text),
            Value::Array(items) => items.iter_mut().for_each(|item| self.redact_value(item)),
            Value::Object(map) => map.values_mut().for_each(|item| self.redact_value(item)),
            _ => {}
        }
    }

    /// Writes a raw output (redacted) under `<out>/raw/` and returns its
    /// evidence-relative path.
    pub fn save_raw(&self, relative: &str, text: &str) -> String {
        let path = self.out.join("raw").join(relative);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&path, self.redact(text)) {
            Ok(()) => format!("raw/{relative}"),
            Err(error) => format!("WRITE_FAILED:{:?}", error.kind()),
        }
    }

    pub fn save_raw_json(&self, relative: &str, value: &Value) -> String {
        let mut value = value.clone();
        self.redact_value(&mut value);
        let text = serde_json::to_string_pretty(&value).unwrap_or_default();
        self.save_raw(relative, &format!("{text}\n"))
    }

    /// The disposable directory, created on first use under the scratch base
    /// and made the probe's working directory so child processes inherit it.
    pub fn disposable(&mut self) -> Result<&Disposable, String> {
        if self.disposable.is_none() {
            std::fs::create_dir_all(&self.scratch)
                .map_err(|e| format!("scratch: {:?}", e.kind()))?;
            let id = uuid::Uuid::new_v4().simple().to_string();
            let root = self.scratch.join(format!("cp-{}", &id[..8]));
            std::fs::create_dir(&root).map_err(|e| format!("create: {:?}", e.kind()))?;
            let home = root.join("h");
            let tmp = root.join("t");
            std::fs::create_dir(&home).map_err(|e| format!("home: {:?}", e.kind()))?;
            std::fs::create_dir(&tmp).map_err(|e| format!("tmp: {:?}", e.kind()))?;
            std::env::set_current_dir(&root).map_err(|e| format!("chdir: {:?}", e.kind()))?;
            self.disposable_record = Some(json!({
                "root": root.display().to_string(),
                "codexHome": home.display().to_string(),
                "createdAt": stamp(now_ms()),
            }));
            self.disposable = Some(Disposable { root, home, tmp });
        }
        self.disposable
            .as_ref()
            .ok_or_else(|| "disposable unavailable".to_string())
    }

    /// Lists the disposable home tree (paths only) to show where the CLI wrote.
    pub fn disposable_home_tree(&self) -> Value {
        let Some(disposable) = &self.disposable else {
            return Value::Null;
        };
        let mut entries = Vec::new();
        let mut stack = vec![disposable.home.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in read.flatten() {
                let path = entry.path();
                let relative = path
                    .strip_prefix(&disposable.home)
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let kind = match entry.file_type() {
                    Ok(t) if t.is_symlink() => "symlink",
                    Ok(t) if t.is_dir() => "dir",
                    Ok(_) => "file",
                    Err(_) => "unknown",
                };
                if kind == "dir" && entries.len() < 256 {
                    stack.push(path.clone());
                }
                if entries.len() < 256 {
                    entries.push(json!({ "path": relative, "kind": kind }));
                }
            }
        }
        entries.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
        Value::Array(entries)
    }

    /// Removes the disposable directory after proving it is the one this run
    /// created (direct child of the scratch base, `cp-` prefix, same path).
    pub fn cleanup(&mut self) -> Value {
        let Some(disposable) = self.disposable.take() else {
            return Value::Null;
        };
        let _ = std::env::set_current_dir(&self.scratch);
        let owned = disposable.root.parent() == Some(self.scratch.as_path())
            && disposable
                .root
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("cp-"));
        let removed = owned && std::fs::remove_dir_all(&disposable.root).is_ok();
        let mut record = self.disposable_record.take().unwrap_or_else(|| json!({}));
        if let Value::Object(map) = &mut record {
            map.insert("removed".into(), json!(removed));
            map.insert("existsAfterCleanup".into(), json!(disposable.root.exists()));
            map.insert("removedAt".into(), stamp(now_ms()));
        }
        record
    }

    /// Runs an absolute program with a cleared environment (LANG only).
    pub fn run_plain(&self, program: &str, args: &[&str], timeout: Duration, max: usize) -> Ran {
        self.run_inner(program, args, &[], timeout, max)
    }

    /// Runs a Codex executable inside the disposable environment.
    pub fn run_codex(
        &mut self,
        program: &Path,
        args: &[&str],
        timeout: Duration,
        max: usize,
    ) -> Ran {
        let env = match self.disposable() {
            Ok(d) => vec![
                ("CODEX_HOME".to_string(), d.home.display().to_string()),
                ("HOME".to_string(), d.home.display().to_string()),
                ("TMPDIR".to_string(), format!("{}/", d.tmp.display())),
                (
                    "PATH".to_string(),
                    "/usr/bin:/bin:/usr/sbin:/sbin".to_string(),
                ),
            ],
            Err(error) => {
                return Ran {
                    record: json!({ "argv": [program.display().to_string()], "error": error }),
                    stdout: String::new(),
                    stderr: String::new(),
                    ok: false,
                };
            }
        };
        let program = program.display().to_string();
        self.run_inner(&program, args, &env, timeout, max)
    }

    fn run_inner(
        &self,
        program: &str,
        args: &[&str],
        env: &[(String, String)],
        timeout: Duration,
        max: usize,
    ) -> Ran {
        let mut command = BoundedCommand::new(program, timeout, max);
        for arg in args {
            command = command.arg(*arg);
        }
        for (key, value) in env {
            command = command.env(key, value);
        }
        let mut argv = vec![program.to_string()];
        argv.extend(args.iter().map(|a| a.to_string()));
        let mut record = Map::new();
        record.insert("argv".into(), json!(argv));
        if !env.is_empty() {
            let mut shown: Map<String, Value> =
                env.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
            shown.insert("LANG".into(), json!("en_US.UTF-8"));
            record.insert("env".into(), Value::Object(shown));
            record.insert("environmentCleared".into(), json!(true));
            if let Some(d) = &self.disposable {
                record.insert("cwd".into(), json!(d.root.display().to_string()));
            }
        }
        let started = now_ms();
        record.insert("startedAt".into(), stamp(started));
        let (stdout, stderr, ok) = match run_bounded(&command) {
            Ok(output) => {
                record.insert("pid".into(), json!(output.pid));
                record.insert("exitStatus".into(), json!(output.status));
                record.insert("timedOut".into(), json!(output.timed_out));
                record.insert("elapsedMs".into(), json!(output.elapsed.as_millis() as u64));
                record.insert("stdoutBytes".into(), json!(output.stdout.len()));
                record.insert("stderrBytes".into(), json!(output.stderr.len()));
                record.insert("stdoutTruncated".into(), json!(output.stdout_truncated));
                record.insert("stderrTruncated".into(), json!(output.stderr_truncated));
                (
                    String::from_utf8_lossy(&output.stdout).into_owned(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                    output.succeeded(),
                )
            }
            Err(error) => {
                record.insert("spawnError".into(), json!(error.to_string()));
                (String::new(), String::new(), false)
            }
        };
        Ran {
            record: Value::Object(record),
            stdout,
            stderr,
            ok,
        }
    }

    /// Generates (once per run) the app-server JSON Schema bundle with the
    /// installed CLI into the disposable directory. The command record is
    /// kept even when generation fails; only a successful bundle is returned.
    pub fn schema_bundle(&mut self, experimental: bool) -> Option<(PathBuf, Value)> {
        let cached = if experimental {
            &self.experimental_schema
        } else {
            &self.stable_schema
        };
        if let Some((dir, record)) = cached {
            let ok = record.get("exitStatus").and_then(Value::as_i64) == Some(0);
            return ok.then(|| (dir.clone(), record.clone()));
        }
        let dir = self.disposable().ok()?.root.join(if experimental {
            "schema-experimental"
        } else {
            "schema-stable"
        });
        std::fs::create_dir(&dir).ok()?;
        let dir_text = dir.display().to_string();
        let mut args = vec!["app-server", "generate-json-schema"];
        if experimental {
            args.push("--experimental");
        }
        args.extend(["--out", dir_text.as_str()]);
        let codex = self.codex.clone();
        let ran = self.run_codex(&codex, &args, Duration::from_secs(60), 64 * 1024);
        let entry = (dir, ran.record);
        if experimental {
            self.experimental_schema = Some(entry.clone());
        } else {
            self.stable_schema = Some(entry.clone());
        }
        ran.ok.then_some(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_formats_known_instants() {
        assert_eq!(utc(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(utc(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(utc(1_791_293_512_345), "2026-10-06T13:31:52.345Z");
    }

    #[test]
    fn redaction_replaces_home_prefix_only() {
        let ctx = Ctx::new(
            PathBuf::from("/x"),
            "/Users/someone".into(),
            PathBuf::from("/x"),
            PathBuf::from("/x"),
            None,
        );
        assert_eq!(ctx.redact("/Users/someone/.codex/a"), "~/.codex/a");
        assert_eq!(ctx.redact("/Users/someone"), "~");
        assert_eq!(ctx.redact("/Users/someoneelse/a"), "/Users/someoneelse/a");
        let mut value = json!({ "a": ["/Users/someone/x", 1], "b": "/Users/someone/y" });
        ctx.redact_value(&mut value);
        assert_eq!(value, json!({ "a": ["~/x", 1], "b": "~/y" }));
    }
}
