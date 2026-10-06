//! Journal cursors and revisions travel as canonical nonnegative decimal
//! strings within SQLite's signed 64-bit range (SPEC §5.4). Compare them only
//! after parsing; lexical order of decimal strings is wrong ("10" < "9").

/// Parses a canonical cursor string: ASCII digits, no sign, no leading zero
/// (except `"0"` itself), and within `0..=i64::MAX`.
pub fn parse_cursor(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 19 || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if bytes.len() > 1 && bytes[0] == b'0' {
        return None;
    }
    value.parse::<i64>().ok()
}

/// Formats a cursor value. Negative values never represent a journal position.
pub fn format_cursor(value: i64) -> String {
    debug_assert!(value >= 0, "journal cursors are nonnegative");
    value.max(0).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_values() {
        assert_eq!(parse_cursor("0"), Some(0));
        assert_eq!(parse_cursor("42"), Some(42));
        assert_eq!(parse_cursor("9223372036854775807"), Some(i64::MAX));
    }

    #[test]
    fn rejects_noncanonical_and_out_of_range_values() {
        for bad in ["", "-1", "+1", "01", "1.0", " 1", "9223372036854775808", "abc"] {
            assert_eq!(parse_cursor(bad), None, "{bad:?} must be rejected");
        }
    }

    #[test]
    fn numeric_not_lexical_order() {
        let nine = parse_cursor("9").expect("valid");
        let ten = parse_cursor("10").expect("valid");
        assert!(ten > nine);
        assert_eq!(format_cursor(ten), "10");
    }
}
