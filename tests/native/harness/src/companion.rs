//! The companion as the harness sees it: its incarnation (from the owner-only
//! locator, verified against the kernel), the qualification client (only
//! qualification builds accept that role), and its JSON-lines log read from
//! a recorded position so each check sees only events after its own start.

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::Value;
use threadspace_contracts::control::{ClientRole, ControlRequestBody, ControlResponseBody};
use threadspace_relay::client::{BlockingClient, connect};
use threadspace_relay::locator;

use crate::identity::Identity;
use crate::procs::{self, Incarnation};

pub struct Companion<'a> {
    pub id: &'a Identity,
}

impl<'a> Companion<'a> {
    pub fn new(id: &'a Identity) -> Self {
        Self { id }
    }

    /// Every live process running this identity's companion executable.
    pub fn processes(&self) -> Vec<Incarnation> {
        procs::with_executable(&self.id.companion_executable)
    }

    /// The incarnation the locator names, if that exact process is alive.
    pub fn incarnation(&self) -> Option<Incarnation> {
        let locator = locator::read(&self.id.agent.locator).ok()?;
        let live = Incarnation::of(locator.companion.pid as i32)?;
        (live.start_seconds.to_string() == locator.companion.start_seconds
            && live.start_microseconds == locator.companion.start_microseconds)
            .then_some(live)
    }

    pub fn client(&self, timeout: Duration) -> Result<BlockingClient, String> {
        connect(&self.id.agent.locator, ClientRole::Qualification, timeout)
            .map(BlockingClient::new)
            .map_err(|error| error.to_string())
    }

    pub fn request(
        &self,
        body: ControlRequestBody,
        timeout: Duration,
    ) -> Result<ControlResponseBody, String> {
        self.client(timeout)?
            .request(body)
            .map_err(|error| error.to_string())
    }

    pub fn diagnostics(&self) -> Result<Value, String> {
        match self.request(ControlRequestBody::Diagnostics, Duration::from_secs(10))? {
            ControlResponseBody::Diagnostics { report } => {
                serde_json::to_value(&*report).map_err(|error| error.to_string())
            }
            other => Err(format!("unexpected {other:?}")),
        }
    }

    /// Waits for a live, connectable incarnation different from `old`.
    pub fn wait_new_incarnation(
        &self,
        old: Option<&Incarnation>,
        timeout: Duration,
    ) -> Option<(Incarnation, u64)> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if let Some(now) = self.incarnation()
                && old.is_none_or(|old| old != &now)
                && self.client(Duration::from_secs(2)).is_ok()
            {
                return Some((now, started.elapsed().as_millis() as u64));
            }
            crate::pause_ms(100);
        }
        None
    }

    /// Delivers a qualification command to hydrated views as a native intent.
    pub fn view_command(&self, command: &str, args: Value) -> Result<(String, u32), String> {
        match self.request(
            ControlRequestBody::QualifyViewCommand {
                command: command.to_owned(),
                args,
            },
            Duration::from_secs(10),
        )? {
            ControlResponseBody::ViewCommandQueued {
                intent_id,
                hydrated_views,
            } => Ok((intent_id, hydrated_views)),
            other => Err(format!("unexpected {other:?}")),
        }
    }

    pub fn log(&self) -> LogCursor {
        LogCursor::at_end(self.id.companion_log())
    }
}

/// Reads JSON lines appended after a recorded file position.
pub struct LogCursor {
    path: PathBuf,
    offset: u64,
    /// Lines read from the file but not yet handed out by `wait_for`.
    pending: std::collections::VecDeque<Value>,
}

impl LogCursor {
    pub fn at_end(path: PathBuf) -> Self {
        let offset = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        Self {
            path,
            offset,
            pending: std::collections::VecDeque::new(),
        }
    }

    /// New complete lines since the last read (a rotated file restarts at 0),
    /// including any a previous `wait_for` read but did not hand out.
    pub fn read_new(&mut self) -> Vec<Value> {
        let mut lines: Vec<Value> = self.pending.drain(..).collect();
        lines.extend(self.read_file());
        lines
    }

    fn read_file(&mut self) -> Vec<Value> {
        let Ok(mut file) = File::open(&self.path) else {
            return Vec::new();
        };
        let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        if len < self.offset {
            self.offset = 0;
        }
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut lines = Vec::new();
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        while let Ok(read) = reader.read_line(&mut line) {
            if read == 0 || !line.ends_with('\n') {
                break;
            }
            self.offset += read as u64;
            if let Ok(value) = serde_json::from_str::<Value>(line.trim()) {
                lines.push(value);
            }
            line.clear();
        }
        lines
    }

    /// Waits for the first new line with `event` that satisfies `matches`;
    /// every line read meanwhile is appended to `seen`.
    pub fn wait_for(
        &mut self,
        event: &str,
        matches: impl Fn(&Value) -> bool,
        timeout: Duration,
        seen: &mut Vec<Value>,
    ) -> Option<Value> {
        let started = Instant::now();
        loop {
            let mut batch: std::collections::VecDeque<Value> = self.read_new().into();
            while let Some(line) = batch.pop_front() {
                let hit = line["event"] == event && matches(&line);
                seen.push(line.clone());
                if hit {
                    // Keep the rest of this batch for the next wait.
                    self.pending.extend(batch);
                    return Some(line);
                }
            }
            if started.elapsed() >= timeout {
                return None;
            }
            crate::pause_ms(100);
        }
    }
}
