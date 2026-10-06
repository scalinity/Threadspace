//! Snapshot chunking shared by the companion (which decides whether a
//! complete view fits) and the desktop sender (which enforces the bound
//! before `SnapshotBegin`), so both apply exactly the same rule (SPEC §18.4).

use crate::limits::{FRAME_MAX_BYTES, SNAPSHOT_MAX_BYTES, SNAPSHOT_MAX_FRAMES};

/// Envelope reserve for a chunk frame's header and JSON framing.
pub const ENVELOPE_RESERVE: usize = 2048;

/// Escaped payload budget of one `SnapshotChunk` frame.
pub const CHUNK_BUDGET: usize = FRAME_MAX_BYTES - ENVELOPE_RESERVE;

/// Bytes a character occupies once JSON-escaped inside a string literal.
pub fn escaped_len(character: char) -> usize {
    match character {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{08}' | '\u{0c}' => 2,
        c if (c as u32) < 0x20 => 6,
        c => c.len_utf8(),
    }
}

/// Splits serialized JSON into UTF-8 slices whose escaped size fits `budget`.
pub fn chunk_for_frames(json: &str, budget: usize) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut size = 0;
    for (index, character) in json.char_indices() {
        let length = escaped_len(character);
        if size + length > budget && index > start {
            chunks.push(&json[start..index]);
            start = index;
            size = 0;
        }
        size += length;
    }
    if start < json.len() {
        chunks.push(&json[start..]);
    }
    chunks
}

/// Frames a snapshot needs: `SnapshotBegin`, its chunks and `SnapshotEnd`.
pub fn snapshot_frames(json: &str) -> usize {
    chunk_for_frames(json, CHUNK_BUDGET).len() + 2
}

/// Whether a serialized snapshot fits the complete-snapshot bound.
pub fn snapshot_fits(json: &str) -> bool {
    json.len() <= SNAPSHOT_MAX_BYTES && snapshot_frames(json) <= SNAPSHOT_MAX_FRAMES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_heavy_json_counts_escaped_bytes() {
        let json = "\"".repeat(CHUNK_BUDGET);
        assert_eq!(chunk_for_frames(&json, CHUNK_BUDGET).len(), 2);
        assert!(snapshot_fits(&json));
        assert!(!snapshot_fits(&"a".repeat(SNAPSHOT_MAX_BYTES + 1)));
    }
}
