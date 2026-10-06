//! Desktop-shell preferences and log (SPEC §18.8, §19.4). Window bounds live
//! in the main app's own support directory so first-run and
//! observation-disabled UI can restore them; restored bounds are constrained
//! onto a current display. The shell log is a small capped JSON-lines file.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Manager, Runtime};

const BOUNDS_FILE: &str = "window-bounds.json";
const LOG_FILE: &str = "desktop.log";
const LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
const SAVE_INTERVAL: Duration = Duration::from_millis(500);
/// A restored window must keep at least this much of itself on a display.
const MIN_VISIBLE: f64 = 120.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

static LAST_SAVE: Mutex<Option<Instant>> = Mutex::new(None);

fn support_dir<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path().app_config_dir().ok()
}

pub fn load_bounds<R: Runtime>(app: &AppHandle<R>) -> Option<Bounds> {
    let text = fs::read_to_string(support_dir(app)?.join(BOUNDS_FILE)).ok()?;
    let bounds: Bounds = serde_json::from_str(&text).ok()?;
    [bounds.x, bounds.y, bounds.width, bounds.height]
        .iter()
        .all(|value| value.is_finite())
        .then_some(bounds)
}

/// Throttled atomic save of the latest on-screen bounds.
pub fn save_bounds<R: Runtime>(app: &AppHandle<R>, bounds: Bounds) {
    if let Ok(mut last) = LAST_SAVE.lock() {
        if last.is_some_and(|at| at.elapsed() < SAVE_INTERVAL) {
            return;
        }
        *last = Some(Instant::now());
    }
    let Some(dir) = support_dir(app) else { return };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let Ok(text) = serde_json::to_vec(&bounds) else {
        return;
    };
    let temp = dir.join(format!("{BOUNDS_FILE}.tmp"));
    if fs::write(&temp, text).is_ok() {
        let _ = fs::rename(&temp, dir.join(BOUNDS_FILE));
    }
}

/// Keeps a saved rectangle usable on the current displays: shrinks it to fit
/// the display it overlaps most, and moves it fully onto that display's work
/// area when too little of it would be visible (SPEC §18.8).
pub fn constrain(saved: Bounds, areas: &[Bounds]) -> Bounds {
    let overlap = |area: &Bounds| {
        let width = (saved.x + saved.width).min(area.x + area.width) - saved.x.max(area.x);
        let height = (saved.y + saved.height).min(area.y + area.height) - saved.y.max(area.y);
        (width.max(0.0), height.max(0.0))
    };
    let best = areas
        .iter()
        .max_by(|a, b| {
            let (aw, ah) = overlap(a);
            let (bw, bh) = overlap(b);
            (aw * ah).total_cmp(&(bw * bh))
        })
        .copied();
    let Some(area) = best else { return saved };
    let width = saved.width.min(area.width).max(1.0);
    let height = saved.height.min(area.height).max(1.0);
    let (visible_width, visible_height) = overlap(&area);
    if visible_width >= MIN_VISIBLE.min(width) && visible_height >= MIN_VISIBLE.min(height) {
        return Bounds {
            x: saved.x.clamp(
                area.x - width + MIN_VISIBLE,
                area.x + area.width - MIN_VISIBLE,
            ),
            y: saved.y.clamp(area.y, area.y + area.height - MIN_VISIBLE),
            width,
            height,
        };
    }
    Bounds {
        x: area.x + (area.width - width) / 2.0,
        y: area.y + (area.height - height) / 2.0,
        width,
        height,
    }
}

/// Appends one JSON line to the shell log, rotating once past 10 MiB.
pub fn log<R: Runtime>(app: &AppHandle<R>, event: &str, detail: serde_json::Value) {
    let Ok(dir) = app.path().app_log_dir() else {
        return;
    };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(LOG_FILE);
    if fs::metadata(&path).is_ok_and(|meta| meta.len() > LOG_MAX_BYTES) {
        let _ = fs::rename(&path, dir.join(format!("{LOG_FILE}.1")));
    }
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0);
    let line = json!({ "ms": ms, "event": event, "pid": std::process::id(), "detail": detail });
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Bounds = Bounds {
        x: 0.0,
        y: 25.0,
        width: 1728.0,
        height: 1092.0,
    };

    #[test]
    fn offscreen_bounds_move_onto_the_display() {
        let corrected = constrain(
            Bounds {
                x: -5000.0,
                y: 3000.0,
                width: 1280.0,
                height: 820.0,
            },
            &[SCREEN],
        );
        assert!(
            corrected.x >= SCREEN.x && corrected.x + corrected.width <= SCREEN.x + SCREEN.width
        );
        assert!(
            corrected.y >= SCREEN.y && corrected.y + corrected.height <= SCREEN.y + SCREEN.height
        );
    }

    #[test]
    fn oversized_bounds_shrink_and_visible_bounds_stay() {
        let kept = Bounds {
            x: 100.0,
            y: 100.0,
            width: 1280.0,
            height: 820.0,
        };
        assert_eq!(constrain(kept, &[SCREEN]), kept);
        let huge = constrain(
            Bounds {
                x: 0.0,
                y: 25.0,
                width: 9000.0,
                height: 9000.0,
            },
            &[SCREEN],
        );
        assert!(huge.width <= SCREEN.width && huge.height <= SCREEN.height);
    }
}
