//! Bounded initial views and pages (SPEC §18.4). A complete view is sent when
//! it fits the snapshot bound; otherwise the initial view carries a bounded
//! prefix of each list in stable id order with exact totals and continuation
//! positions, and the rest is paged. Outstanding counts are never omitted.

use rusqlite::{Connection, params};
use threadspace_contracts::frames::snapshot_fits;
use threadspace_contracts::projection::{AttentionView, FleetSnapshot, SessionView};

use crate::{JournalError, projection};

/// Serialized budget of one list in a bounded initial view, before halving.
const INITIAL_LIST_BUDGET: usize = 192 * 1024;
/// Serialized budget of one page reply, under the 64 KiB query cap.
pub const PAGE_BUDGET: usize = 56 * 1024;

pub struct Page<T> {
    pub rows: Vec<T>,
    pub next_after: Option<String>,
    pub total: u32,
}

fn json_len<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
}

/// Takes rows in id order after `after` until `limit` rows or `budget`
/// serialized bytes; at least one row is returned when any remains.
fn page<T: serde::Serialize>(
    ids: Vec<String>,
    limit: u32,
    budget: usize,
    total: u32,
    load: impl Fn(&str) -> Result<T, JournalError>,
) -> Result<Page<T>, JournalError> {
    let limit = limit.max(1);
    let mut rows = Vec::new();
    let mut used = 0;
    let mut next_after = None;
    for (index, id) in ids.iter().enumerate() {
        if rows.len() as u32 >= limit {
            next_after = index.checked_sub(1).map(|last| ids[last].clone());
            break;
        }
        let row = load(id)?;
        let size = json_len(&row) + 1;
        if !rows.is_empty() && used + size > budget {
            next_after = Some(ids[index - 1].clone());
            break;
        }
        used += size;
        rows.push(row);
    }
    Ok(Page {
        rows,
        next_after,
        total,
    })
}

fn ids(conn: &Connection, sql: &str, after: Option<&str>) -> Result<Vec<String>, JournalError> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement
        .query_map(params![after], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn session_page(
    conn: &Connection,
    after: Option<&str>,
    limit: u32,
    budget: usize,
) -> Result<Page<SessionView>, JournalError> {
    let total: u32 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
    let ids = ids(
        conn,
        "SELECT id FROM sessions WHERE ?1 IS NULL OR id > ?1 ORDER BY id",
        after,
    )?;
    page(ids, limit, budget, total, |id| {
        projection::session(conn, id)
    })
}

pub fn attention_page(
    conn: &Connection,
    after: Option<&str>,
    limit: u32,
    budget: usize,
) -> Result<Page<AttentionView>, JournalError> {
    let total: u32 = conn.query_row(
        "SELECT COUNT(*) FROM attention_items WHERE resolved_at_ms IS NULL",
        [],
        |row| row.get(0),
    )?;
    let ids = ids(
        conn,
        "SELECT id FROM attention_items WHERE resolved_at_ms IS NULL AND (?1 IS NULL OR id > ?1)
          ORDER BY id",
        after,
    )?;
    page(ids, limit, budget, total, |id| {
        projection::attention(conn, id)
    })
}

/// The view a new subscription receives at `view_revision`.
pub fn initial_view(
    conn: &Connection,
    view_revision: String,
) -> Result<FleetSnapshot, JournalError> {
    let sessions = projection::sessions(conn)?;
    let attention = projection::open_attention(conn)?;
    let counts = projection::counts(conn)?;
    let complete = FleetSnapshot {
        view_revision: view_revision.clone(),
        total_sessions: sessions.len() as u32,
        total_attention: attention.len() as u32,
        sessions,
        attention,
        counts,
        complete: true,
        sessions_after: None,
        attention_after: None,
    };
    if serde_json::to_string(&complete).is_ok_and(|json| snapshot_fits(&json)) {
        return Ok(complete);
    }
    let mut budget = INITIAL_LIST_BUDGET;
    loop {
        let sessions = session_page(conn, None, u32::MAX, budget)?;
        let attention = attention_page(conn, None, u32::MAX, budget)?;
        let bounded = FleetSnapshot {
            view_revision: view_revision.clone(),
            sessions: sessions.rows,
            attention: attention.rows,
            counts,
            complete: false,
            total_sessions: sessions.total,
            total_attention: attention.total,
            sessions_after: sessions.next_after,
            attention_after: attention.next_after,
        };
        let fits = serde_json::to_string(&bounded).is_ok_and(|json| snapshot_fits(&json));
        if fits || budget <= 1024 {
            return Ok(bounded);
        }
        budget /= 2;
    }
}
