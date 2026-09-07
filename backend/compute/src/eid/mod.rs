//! Canonical EID module for EDMS.
//!
//! Provides formatting, parsing, validation, and centralized allocation
//! for endpoint identifiers in the format `E[0-9]{4,}-[A-Z]{3}` (e.g. `E0001-AAA`).

pub mod allocator;

pub use allocator::EidAllocator;

use std::fmt;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum EidError {
    InvalidFormat(String),
    DatabaseError(String),
    LockError(String),
    InvalidNumber(u64),
}

impl fmt::Display for EidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EidError::InvalidFormat(s) => write!(f, "Invalid EID format: {s}"),
            EidError::DatabaseError(s) => write!(f, "EID database error: {s}"),
            EidError::LockError(s) => write!(f, "EID lock error: {s}"),
            EidError::InvalidNumber(n) => write!(f, "Invalid EID number: {n}"),
        }
    }
}

impl std::error::Error for EidError {}

impl From<rusqlite::Error> for EidError {
    fn from(err: rusqlite::Error) -> Self {
        EidError::DatabaseError(err.to_string())
    }
}

/// Formats a sequential number with the default prefix `E` and default suffix `AAA`.
/// Numbers below 10,000 are padded to 4 digits (e.g. 1 -> `E0001-AAA`, 9999 -> `E9999-AAA`).
/// Numbers 10,000 and above format with full width (e.g. 10000 -> `E10000-AAA`).
pub fn format_eid(number: u64) -> String {
    format_eid_with_suffix(number, "AAA")
}

/// Formats a sequential number with a specific 3-letter uppercase suffix.
pub fn format_eid_with_suffix(number: u64, suffix: &str) -> String {
    format!("E{:04}-{}", number, suffix)
}

/// Formats a global sequential index with 10,000-block alphabetical rollover:
/// - 1..=9999 -> `E0001-AAA` .. `E9999-AAA`
/// - 10000..=19999 -> `E0000-AAB` .. `E9999-AAB`
/// (Capacity: 10,000 x 26^3 = 175,760,000 unique EIDs)
pub fn format_eid_rollover(index: u64) -> String {
    let num_part = index % 10000;
    let alpha_part = index / 10000;
    let suffix = index_to_suffix(alpha_part);
    format!("E{:04}-{}", num_part, suffix)
}

/// Converts a 0-based suffix index into a 3-letter uppercase code (0 -> "AAA", 1 -> "AAB", etc.).
pub fn index_to_suffix(idx: u64) -> String {
    let c2 = (idx % 26) as u8;
    let c1 = ((idx / 26) % 26) as u8;
    let c0 = ((idx / (26 * 26)) % 26) as u8;
    format!(
        "{}{}{}",
        (b'A' + c0) as char,
        (b'A' + c1) as char,
        (b'A' + c2) as char,
    )
}

/// Converts a 3-letter uppercase suffix code into a 0-based index ("AAA" -> 0, "AAB" -> 1, etc.).
pub fn suffix_to_index(suffix: &str) -> Result<u64, EidError> {
    if suffix.len() != 3 {
        return Err(EidError::InvalidFormat(format!(
            "Suffix must be exactly 3 characters: '{suffix}'"
        )));
    }
    let bytes = suffix.as_bytes();
    for &b in bytes {
        if !b.is_ascii_uppercase() {
            return Err(EidError::InvalidFormat(format!(
                "Suffix must be uppercase ASCII: '{suffix}'"
            )));
        }
    }
    let c0 = (bytes[0] - b'A') as u64;
    let c1 = (bytes[1] - b'A') as u64;
    let c2 = (bytes[2] - b'A') as u64;
    Ok(c0 * 26 * 26 + c1 * 26 + c2)
}

/// Validates whether a string conforms to the canonical EID specification:
/// Starts with 'E', followed by 4 or more digits, a hyphen, and 3 uppercase ASCII letters.
pub fn is_valid_eid(eid: &str) -> bool {
    let parts: Vec<&str> = eid.split('-').collect();
    if parts.len() != 2 {
        return false;
    }
    let num_part = parts[0];
    let suffix = parts[1];

    if !num_part.starts_with('E') || num_part.len() < 5 {
        return false;
    }
    if !num_part[1..].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if suffix.len() != 3 || !suffix.chars().all(|c| c.is_ascii_uppercase()) {
        return false;
    }
    true
}

/// Parses an EID into its numeric component and 3-letter suffix.
pub fn parse_eid(eid: &str) -> Result<(u64, String), EidError> {
    if !is_valid_eid(eid) {
        return Err(EidError::InvalidFormat(eid.to_string()));
    }
    let parts: Vec<&str> = eid.split('-').collect();
    let num_str = &parts[0][1..];
    let number = num_str.parse::<u64>().map_err(|_| {
        EidError::InvalidFormat(format!("Failed to parse numeric part of EID: '{eid}'"))
    })?;
    Ok((number, parts[1].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_eid_canonical() {
        assert_eq!(format_eid(1), "E0001-AAA");
        assert_eq!(format_eid(42), "E0042-AAA");
        assert_eq!(format_eid(9999), "E9999-AAA");
        assert_eq!(format_eid(10000), "E10000-AAA");
    }

    #[test]
    fn test_format_eid_with_suffix() {
        assert_eq!(format_eid_with_suffix(1234, "ABC"), "E1234-ABC");
    }

    #[test]
    fn test_format_eid_rollover() {
        assert_eq!(format_eid_rollover(1), "E0001-AAA");
        assert_eq!(format_eid_rollover(9999), "E9999-AAA");
        assert_eq!(format_eid_rollover(10000), "E0000-AAB");
        assert_eq!(format_eid_rollover(10001), "E0001-AAB");
    }

    #[test]
    fn test_validation() {
        assert!(is_valid_eid("E0001-AAA"));
        assert!(is_valid_eid("E1234-ABC"));
        assert!(is_valid_eid("E9999-ZZZ"));
        assert!(is_valid_eid("E10000-AAA"));

        assert!(!is_valid_eid(""));
        assert!(!is_valid_eid("0001-AAA"));
        assert!(!is_valid_eid("E001-AAA")); // only 3 digits
        assert!(!is_valid_eid("E0001-aa")); // lowercase / 2 chars
        assert!(!is_valid_eid("E0001-AAAA")); // 4 chars
        assert!(!is_valid_eid("uuid-something"));
    }

    #[test]
    fn test_parse_eid() {
        assert_eq!(parse_eid("E0001-AAA").unwrap(), (1, "AAA".to_string()));
        assert_eq!(parse_eid("E9999-XYZ").unwrap(), (9999, "XYZ".to_string()));
        assert_eq!(parse_eid("E10000-AAA").unwrap(), (10000, "AAA".to_string()));
        assert!(parse_eid("invalid").is_err());
    }
}
