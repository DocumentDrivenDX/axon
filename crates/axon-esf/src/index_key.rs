//! Canonical, store-independent index-key encoding for ESF indexes.
//!
//! This module defines the **canonical specification** for turning a JSON
//! record plus an index declaration ([`IndexDef`] / [`CompoundIndexDef`]) into
//! an order-preserving byte key. The encoding is deliberately store-agnostic:
//! the same bytes can be used as a key in any ordered key-value store, a SQL
//! `BYTEA`/`BLOB` index column, or an in-memory `BTreeMap`, and lexicographic
//! comparison of the bytes (`memcmp`) reproduces the natural ordering of the
//! underlying typed values.
//!
//! Axon's storage layer (`axon-storage`) maintains its own index structures
//! (`IndexValue`, `OrderedFloat`, compound keys). This module is the canonical
//! reference those structures conform to; see the "Store SSOT" note at the
//! bottom of this file.
//!
//! # Encoding spec
//!
//! ## Per-type value encoding (byte-order == value-order)
//!
//! | [`IndexType`] | Encoding |
//! |---------------|----------|
//! | `String`   | Raw UTF-8 bytes. |
//! | `Integer`  | Big-endian `i64` with the sign bit flipped: `(v as u64) ^ (1 << 63)`. This maps `i64::MIN..=i64::MAX` onto `0..=u64::MAX` so unsigned big-endian byte order matches signed integer order (negatives sort before positives). |
//! | `Float`    | Order-preserving IEEE-754 `f64`: take the raw bits; if the sign bit is clear (non-negative) flip only the sign bit, otherwise flip all bits; then big-endian. This yields a total order where `-inf < negative < -0.0 < +0.0 < positive < +inf < NaN`. |
//! | `Datetime` | Canonicalized to epoch **nanoseconds** as an `i64`, then encoded exactly like `Integer`. |
//! | `Boolean`  | A single byte: `0x00` for `false`, `0x01` for `true` (so `false < true`). |
//!
//! ## Datetime input forms
//!
//! A datetime field value is accepted as **either**:
//! * an RFC 3339 / ISO 8601 string (e.g. `"2026-06-28T12:00:00Z"`,
//!   `"2026-06-28T12:00:00.123456789+02:00"`), parsed to epoch nanoseconds; or
//! * a JSON number interpreted as epoch nanoseconds (integer; a float with no
//!   fractional part is accepted, a float with a fractional part is rejected as
//!   unencodable since sub-nanosecond precision is not representable).
//!
//! Any string that does not parse as RFC 3339, or a number outside `i64`
//! range, is an [`IndexKeyError`] (never silently mis-sorted).
//!
//! The RFC 3339 parser here is a self-contained, dependency-free implementation
//! covering the grammar Axon emits and accepts. It supports fractional seconds
//! (1–9 digits) and `Z` / `±HH:MM` offsets, and applies proleptic-Gregorian
//! day counting with leap years. Leap seconds (`:60`) are clamped to `:59`.
//!
//! ## Null and missing semantics
//!
//! * **Missing** configured field (the dotted path does not resolve) =>
//!   `Ok(None)` (the record is *sparse* for this index; no key is produced).
//! * **JSON `null`** at the resolved path is treated identically to missing =>
//!   `Ok(None)`. Null is not a sortable indexed value in Axon.
//! * **Type mismatch** (the value is present and non-null but cannot be coerced
//!   to the declared [`IndexType`]) => `Err(IndexKeyError::TypeMismatch)`. This
//!   is a hard error so a wrongly-typed record can never silently sort in the
//!   wrong place.
//!
//! For a [`CompoundIndexDef`], *all* fields must produce a value: if any field
//! is missing/null the whole compound key is `Ok(None)` (sparse), and if any
//! field is a type mismatch the whole call is `Err`.
//!
//! ## Composite (length-prefixed) framing
//!
//! Both [`IndexDef::index_key`] (a single field) and
//! [`CompoundIndexDef::index_key`] produce a composite key by concatenating,
//! for each field **in declared order**:
//!
//! ```text
//! len: u32 big-endian  ++  encoded_value_bytes
//! ```
//!
//! Length-prefixing makes the encoding **leftmost-prefix friendly**: the full
//! key for fields `[a, b, c]` has the key for `[a]` (and `[a, b]`) as an exact
//! byte prefix, so a range scan on a prefix of the leading fields is a byte
//! range scan. Because `String` values are length-delimited by the `u32`
//! prefix rather than a terminator, no escaping is required and per-field
//! ordering is preserved within the composite.
//!
//! # Store SSOT
//!
//! This module is the **single source of truth** for Axon's canonical index-key
//! encoding. `axon-storage` currently maintains its own ordered structures
//! (`IndexValue`, `OrderedFloat`, `CompoundKey`) whose `Ord` implementations
//! conform to the ordering defined here (negatives-first integers, total-order
//! floats, byte-ordered strings, `false < true`). One behavioral divergence is
//! intentional and documented: the storage layer treats a type mismatch as
//! "not indexed" (`None`) per FEAT-013, whereas this canonical encoder returns
//! `Err(IndexKeyError::TypeMismatch)` so callers can choose to fail closed.
//! Migrating the store to call this encoder directly is tracked as a follow-up.

use std::error::Error;
use std::fmt;

use serde_json::Value;

use crate::types::{CompoundIndexDef, IndexDef, IndexType};

/// Error produced when a record cannot be encoded into an index key.
///
/// This is distinct from "no key" (`Ok(None)`, i.e. a sparse record): an
/// error means the data is present but invalid for the declared index type and
/// must not be silently dropped or mis-sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexKeyError {
    /// A present, non-null value is not coercible to the declared index type.
    TypeMismatch {
        /// Dotted field path whose value failed to coerce.
        field: String,
        /// The declared index type the value was expected to satisfy.
        expected: IndexType,
        /// The JSON type name actually found (e.g. `"string"`, `"number"`).
        found: &'static str,
    },
    /// A value is of the right JSON kind but cannot be represented in the
    /// canonical domain (e.g. an unparseable RFC 3339 datetime, or a numeric
    /// datetime outside `i64` nanosecond range).
    Unencodable {
        /// Dotted field path whose value could not be encoded.
        field: String,
        /// Human-readable reason.
        reason: String,
    },
}

impl fmt::Display for IndexKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TypeMismatch {
                field,
                expected,
                found,
            } => write!(
                f,
                "index field '{field}': expected {expected} value, found JSON {found}"
            ),
            Self::Unencodable { field, reason } => {
                write!(f, "index field '{field}': {reason}")
            }
        }
    }
}

impl Error for IndexKeyError {}

/// JSON type name for error reporting.
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Navigate a dotted field path (`"a.b.c"`) in a JSON value.
///
/// Returns `None` if any segment is absent or a non-object is encountered
/// before the path is exhausted. An empty path returns the root value.
pub fn extract_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }
    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

/// Encode a present, non-null JSON value to its order-preserving byte form.
///
/// Returns `Err` on type mismatch / unencodable value. Callers handle
/// missing/null (sparse) before reaching this function.
fn encode_value(
    field: &str,
    value: &Value,
    index_type: &IndexType,
) -> Result<Vec<u8>, IndexKeyError> {
    match index_type {
        IndexType::String => match value.as_str() {
            Some(s) => Ok(s.as_bytes().to_vec()),
            None => Err(type_mismatch(field, index_type, value)),
        },
        IndexType::Integer => match value.as_i64() {
            Some(v) => Ok(encode_i64(v)),
            None => Err(type_mismatch(field, index_type, value)),
        },
        IndexType::Float => match value.as_f64() {
            // `as_f64` returns None for non-number JSON.
            Some(v) => Ok(encode_f64(v)),
            None => Err(type_mismatch(field, index_type, value)),
        },
        IndexType::Boolean => match value.as_bool() {
            Some(b) => Ok(vec![u8::from(b)]),
            None => Err(type_mismatch(field, index_type, value)),
        },
        IndexType::Datetime => encode_datetime(field, value),
    }
}

fn type_mismatch(field: &str, expected: &IndexType, value: &Value) -> IndexKeyError {
    IndexKeyError::TypeMismatch {
        field: field.to_string(),
        expected: expected.clone(),
        found: json_type_name(value),
    }
}

/// Order-preserving big-endian `i64`: flip the sign bit so negatives sort first.
fn encode_i64(v: i64) -> Vec<u8> {
    let ordered = (v as u64) ^ (1u64 << 63);
    ordered.to_be_bytes().to_vec()
}

/// Order-preserving IEEE-754 `f64`.
///
/// If the sign bit is clear (value is `+0.0`, positive, or a positive NaN),
/// flip only the sign bit. Otherwise (negative) flip all bits. Big-endian.
///
/// Ordering note: `-0.0` and `+0.0` have distinct bit patterns and therefore
/// distinct encodings (`-0.0` sorts immediately before `+0.0`); they are *not*
/// merged. All NaN bit patterns sort after `+inf`. Callers that require
/// `-0.0 == 0.0` or NaN exclusion must normalize before encoding.
fn encode_f64(v: f64) -> Vec<u8> {
    let bits = v.to_bits();
    let ordered = if bits & (1u64 << 63) == 0 {
        bits | (1u64 << 63)
    } else {
        !bits
    };
    ordered.to_be_bytes().to_vec()
}

/// Encode a datetime value (RFC 3339 string OR epoch-nanos number) as `Integer`.
fn encode_datetime(field: &str, value: &Value) -> Result<Vec<u8>, IndexKeyError> {
    match value {
        Value::String(s) => {
            let nanos = parse_rfc3339_nanos(s).ok_or_else(|| IndexKeyError::Unencodable {
                field: field.to_string(),
                reason: format!("'{s}' is not a valid RFC 3339 datetime"),
            })?;
            Ok(encode_i64(nanos))
        }
        Value::Number(n) => {
            if let Some(v) = n.as_i64() {
                Ok(encode_i64(v))
            } else if let Some(f) = n.as_f64() {
                // Accept an integral float; reject fractional (sub-ns) precision.
                // Bounds use 2^63 (the first f64 above i64::MAX) and -2^63
                // (== i64::MIN, exactly representable), so any `f` passing the
                // guard rounds to a valid i64.
                const I64_MIN_F: f64 = -9_223_372_036_854_775_808.0; // i64::MIN
                const TWO_POW_63: f64 = 9_223_372_036_854_775_808.0; // 2^63 > i64::MAX
                if f.fract() == 0.0 && (I64_MIN_F..TWO_POW_63).contains(&f) {
                    #[allow(clippy::cast_possible_truncation)]
                    Ok(encode_i64(f as i64))
                } else {
                    Err(IndexKeyError::Unencodable {
                        field: field.to_string(),
                        reason: format!(
                            "numeric datetime {f} is not an integer epoch-nanos in i64 range"
                        ),
                    })
                }
            } else {
                Err(IndexKeyError::Unencodable {
                    field: field.to_string(),
                    reason: "numeric datetime out of i64 range".to_string(),
                })
            }
        }
        other => Err(type_mismatch(field, &IndexType::Datetime, other)),
    }
}

/// Frame a single encoded value with a big-endian `u32` length prefix.
fn frame(encoded: &[u8], out: &mut Vec<u8>) {
    let len = encoded.len() as u32;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(encoded);
}

impl IndexDef {
    /// Compute the canonical, length-prefixed index key for this single-field
    /// index against `record`.
    ///
    /// * `Ok(Some(bytes))` — a key was produced.
    /// * `Ok(None)` — the field is missing or `null` (sparse; not indexed).
    /// * `Err(IndexKeyError)` — the field value is present but invalid for the
    ///   declared type.
    ///
    /// The returned bytes are `len(u32 BE) ++ encoded_value`. See the module
    /// docs for the per-type encoding and ordering guarantees.
    pub fn index_key(&self, record: &Value) -> Result<Option<Vec<u8>>, IndexKeyError> {
        let Some(value) = extract_path(record, &self.field) else {
            return Ok(None);
        };
        if value.is_null() {
            return Ok(None);
        }
        let encoded = encode_value(&self.field, value, &self.index_type)?;
        let mut out = Vec::with_capacity(4 + encoded.len());
        frame(&encoded, &mut out);
        Ok(Some(out))
    }
}

impl CompoundIndexDef {
    /// Compute the canonical, length-prefixed compound index key for this
    /// multi-field index against `record`.
    ///
    /// Fields are encoded **in declared order**, each as
    /// `len(u32 BE) ++ encoded_value`, and concatenated. Because of the
    /// length prefixes, the key of the leading field(s) is an exact byte
    /// prefix of the full key (leftmost-prefix friendly).
    ///
    /// * `Ok(Some(bytes))` — every field produced a value.
    /// * `Ok(None)` — at least one field is missing/`null` (sparse: a compound
    ///   index entry requires all fields).
    /// * `Err(IndexKeyError)` — at least one field value is present but invalid.
    pub fn index_key(&self, record: &Value) -> Result<Option<Vec<u8>>, IndexKeyError> {
        let mut out = Vec::new();
        for f in &self.fields {
            let Some(value) = extract_path(record, &f.field) else {
                return Ok(None);
            };
            if value.is_null() {
                return Ok(None);
            }
            let encoded = encode_value(&f.field, value, &f.index_type)?;
            frame(&encoded, &mut out);
        }
        Ok(Some(out))
    }
}

// ── Self-contained RFC 3339 parsing ─────────────────────────────────────────

/// Parse an RFC 3339 datetime into epoch nanoseconds (`i64`).
///
/// Returns `None` for any string that does not match the supported grammar:
/// `YYYY-MM-DDThh:mm:ss[.fraction][Z|±hh:mm]`. The date/time separator may be
/// `T` or a space. Fractional seconds of 1–9 digits are supported. A leap
/// second (`:60`) is clamped to `:59`. The result is the count of nanoseconds
/// since `1970-01-01T00:00:00Z`, which may be negative for earlier instants.
fn parse_rfc3339_nanos(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    // YYYY-MM-DD = 10 chars, then 'T'/' ', then at least hh:mm:ss = 8 chars.
    if bytes.len() < 19 {
        return None;
    }
    let year: i64 = parse_uint(&s[0..4])?;
    if bytes[4] != b'-' {
        return None;
    }
    let month: u32 = parse_uint(&s[5..7])?;
    if bytes[7] != b'-' {
        return None;
    }
    let day: u32 = parse_uint(&s[8..10])?;
    if bytes[10] != b'T' && bytes[10] != b't' && bytes[10] != b' ' {
        return None;
    }
    let hour: u32 = parse_uint(&s[11..13])?;
    if bytes[13] != b':' {
        return None;
    }
    let minute: u32 = parse_uint(&s[14..16])?;
    if bytes[16] != b':' {
        return None;
    }
    let second: u32 = parse_uint(&s[17..19])?;

    // Optional fractional seconds, then a timezone offset.
    let mut idx = 19;
    let frac_nanos: u32 = if idx < bytes.len() && bytes[idx] == b'.' {
        idx += 1;
        let start = idx;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
        }
        if idx == start || idx - start > 9 {
            return None;
        }
        let digits = &s[start..idx];
        let mut scaled: u32 = parse_uint(digits)?;
        // Scale to nanoseconds (pad to 9 digits).
        for _ in 0..(9 - (idx - start)) {
            scaled = scaled.checked_mul(10)?;
        }
        scaled
    } else {
        0
    };

    // Timezone offset: Z, z, or ±hh:mm.
    let offset_minutes: i64 = if idx >= bytes.len() {
        // No offset: treat as UTC (Axon emits explicit offsets; be lenient).
        0
    } else {
        match bytes[idx] {
            b'Z' | b'z' => {
                idx += 1;
                0
            }
            b'+' | b'-' => {
                let sign = if bytes[idx] == b'-' { -1 } else { 1 };
                idx += 1;
                if idx + 5 > bytes.len() {
                    return None;
                }
                let oh: i64 = parse_uint(&s[idx..idx + 2])?;
                if bytes[idx + 2] != b':' {
                    return None;
                }
                let om: i64 = parse_uint(&s[idx + 3..idx + 5])?;
                idx += 5;
                sign * (oh * 60 + om)
            }
            _ => return None,
        }
    };
    // Reject trailing garbage.
    if idx != bytes.len() {
        return None;
    }

    // Validate ranges. Leap second (:60) clamps to :59.
    if !(1..=12).contains(&month) || day < 1 || hour > 23 || minute > 59 {
        return None;
    }
    let second = if second == 60 { 59 } else { second };
    if second > 59 {
        return None;
    }
    if day > days_in_month(year, month) {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let secs_of_day = (hour as i64) * 3600 + (minute as i64) * 60 + (second as i64);
    let utc_secs = days
        .checked_mul(86_400)?
        .checked_add(secs_of_day)?
        .checked_sub(offset_minutes.checked_mul(60)?)?;
    let nanos = utc_secs
        .checked_mul(1_000_000_000)?
        .checked_add(frac_nanos as i64)?;
    Some(nanos)
}

fn parse_uint<T: std::str::FromStr>(s: &str) -> Option<T> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<T>().ok()
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days since 1970-01-01 for a civil date (proleptic Gregorian).
///
/// Algorithm from Howard Hinnant's `days_from_civil` (public domain).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = month as i64;
    let d = day as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::types::CompoundIndexField;
    use serde_json::json;

    fn def(field: &str, ty: IndexType) -> IndexDef {
        IndexDef {
            field: field.to_string(),
            index_type: ty,
            unique: false,
        }
    }

    /// Encode a bare scalar value through a single-field index, stripping the
    /// 4-byte length frame, for direct ordering comparisons.
    fn enc(ty: IndexType, value: Value) -> Vec<u8> {
        let key = def("v", ty)
            .index_key(&json!({ "v": value }))
            .unwrap()
            .unwrap();
        key[4..].to_vec()
    }

    fn assert_sorted_encodes_sorted(ty: IndexType, ascending: &[Value]) {
        let encoded: Vec<Vec<u8>> = ascending
            .iter()
            .map(|v| enc(ty.clone(), v.clone()))
            .collect();
        for win in encoded.windows(2) {
            assert!(
                win[0] < win[1],
                "expected {:?} < {:?} for type {ty}",
                win[0],
                win[1]
            );
        }
    }

    #[test]
    fn string_order_is_byte_order() {
        assert_sorted_encodes_sorted(
            IndexType::String,
            &[json!(""), json!("a"), json!("ab"), json!("b"), json!("z")],
        );
    }

    #[test]
    fn integer_order_including_negatives() {
        assert_sorted_encodes_sorted(
            IndexType::Integer,
            &[
                json!(i64::MIN),
                json!(-1000),
                json!(-1),
                json!(0),
                json!(1),
                json!(1000),
                json!(i64::MAX),
            ],
        );
    }

    #[test]
    fn float_order_negatives_before_positives() {
        // Finite values that serde_json can represent: negatives sort first.
        let ascending = [
            json!(-1e10),
            json!(-1.5),
            json!(-0.0),
            json!(0.0),
            json!(1.5),
            json!(1e10),
        ];
        assert_sorted_encodes_sorted(IndexType::Float, &ascending);

        // -0.0 and 0.0 have distinct encodings, -0.0 first.
        let neg_zero = enc(IndexType::Float, json!(-0.0));
        let pos_zero = enc(IndexType::Float, json!(0.0));
        assert!(neg_zero < pos_zero, "-0.0 should sort before +0.0");
    }

    #[test]
    fn float_special_values_total_order() {
        // serde_json's json! macro maps non-finite f64 to JSON null, so these
        // special values are exercised against the encoder directly. The
        // canonical total order is: -inf < negative < -0.0 < +0.0 < positive
        // < +inf < NaN.
        let ordered = [
            encode_f64(f64::NEG_INFINITY),
            encode_f64(-1.0),
            encode_f64(-0.0),
            encode_f64(0.0),
            encode_f64(1.0),
            encode_f64(f64::INFINITY),
            encode_f64(f64::NAN),
        ];
        for win in ordered.windows(2) {
            assert!(
                win[0] < win[1],
                "{:?} should sort before {:?}",
                win[0],
                win[1]
            );
        }
    }

    #[test]
    fn boolean_false_before_true() {
        assert_sorted_encodes_sorted(IndexType::Boolean, &[json!(false), json!(true)]);
        assert_eq!(enc(IndexType::Boolean, json!(false)), vec![0x00]);
        assert_eq!(enc(IndexType::Boolean, json!(true)), vec![0x01]);
    }

    #[test]
    fn datetime_rfc3339_string_and_numeric_agree_and_order() {
        // RFC3339 string and equivalent epoch-nanos number encode identically.
        let from_str = enc(IndexType::Datetime, json!("1970-01-01T00:00:01Z"));
        let from_num = enc(IndexType::Datetime, json!(1_000_000_000i64));
        assert_eq!(from_str, from_num, "string and numeric ns must agree");

        // Order preserved across the epoch (negative ns sort first).
        assert_sorted_encodes_sorted(
            IndexType::Datetime,
            &[
                json!("1969-12-31T23:59:59Z"),
                json!("1970-01-01T00:00:00Z"),
                json!("2026-06-28T12:00:00Z"),
                json!("2026-06-28T12:00:00.500Z"),
            ],
        );
    }

    #[test]
    fn datetime_offsets_normalize_to_utc() {
        // 12:00+02:00 == 10:00Z
        let with_offset = enc(IndexType::Datetime, json!("2026-06-28T12:00:00+02:00"));
        let utc = enc(IndexType::Datetime, json!("2026-06-28T10:00:00Z"));
        assert_eq!(with_offset, utc);
    }

    #[test]
    fn datetime_fractional_seconds_padded_to_nanos() {
        // .5 seconds == 500_000_000 ns after the whole second.
        let half = parse_rfc3339_nanos("1970-01-01T00:00:00.5Z").unwrap();
        assert_eq!(half, 500_000_000);
        let nine = parse_rfc3339_nanos("1970-01-01T00:00:00.123456789Z").unwrap();
        assert_eq!(nine, 123_456_789);
    }

    #[test]
    fn datetime_unparseable_is_error() {
        let err = def("v", IndexType::Datetime)
            .index_key(&json!({"v": "not-a-date"}))
            .unwrap_err();
        assert!(matches!(err, IndexKeyError::Unencodable { .. }));
    }

    #[test]
    fn datetime_fractional_number_is_error() {
        let err = def("v", IndexType::Datetime)
            .index_key(&json!({"v": 1.5}))
            .unwrap_err();
        assert!(matches!(err, IndexKeyError::Unencodable { .. }));
    }

    #[test]
    fn missing_field_is_sparse_none() {
        let key = def("absent", IndexType::String)
            .index_key(&json!({"present": "x"}))
            .unwrap();
        assert_eq!(key, None);
    }

    #[test]
    fn null_value_is_sparse_none() {
        let key = def("v", IndexType::String)
            .index_key(&json!({"v": null}))
            .unwrap();
        assert_eq!(key, None);
    }

    #[test]
    fn type_mismatch_is_error() {
        let err = def("v", IndexType::Integer)
            .index_key(&json!({"v": "not-an-int"}))
            .unwrap_err();
        assert!(matches!(
            err,
            IndexKeyError::TypeMismatch {
                expected: IndexType::Integer,
                found: "string",
                ..
            }
        ));
    }

    #[test]
    fn nested_path_resolves() {
        let record = json!({"a": {"b": {"c": "deep"}}});
        assert_eq!(extract_path(&record, "a.b.c"), Some(&json!("deep")));
        assert_eq!(extract_path(&record, "a.b.x"), None);
        let key = def("a.b.c", IndexType::String)
            .index_key(&record)
            .unwrap()
            .unwrap();
        assert_eq!(&key[4..], b"deep");
    }

    #[test]
    fn empty_path_returns_root() {
        let record = json!({"a": 1});
        assert_eq!(extract_path(&record, ""), Some(&record));
    }

    fn compound(fields: &[(&str, IndexType)]) -> CompoundIndexDef {
        CompoundIndexDef {
            fields: fields
                .iter()
                .map(|(f, t)| CompoundIndexField {
                    field: f.to_string(),
                    index_type: t.clone(),
                })
                .collect(),
            unique: false,
        }
    }

    #[test]
    fn compound_key_is_length_prefixed() {
        let idx = compound(&[
            ("status", IndexType::String),
            ("priority", IndexType::Integer),
        ]);
        let key = idx
            .index_key(&json!({"status": "open", "priority": 5}))
            .unwrap()
            .unwrap();
        // 4 (len) + 4 ("open") + 4 (len) + 8 (i64) = 20 bytes.
        assert_eq!(key.len(), 20);
        assert_eq!(&key[0..4], &4u32.to_be_bytes());
        assert_eq!(&key[4..8], b"open");
        assert_eq!(&key[8..12], &8u32.to_be_bytes());
    }

    #[test]
    fn leading_field_key_is_byte_prefix_of_compound() {
        let leading = def("status", IndexType::String);
        let full = compound(&[
            ("status", IndexType::String),
            ("priority", IndexType::Integer),
        ]);
        let record = json!({"status": "open", "priority": 5});

        let leading_key = leading.index_key(&record).unwrap().unwrap();
        let full_key = full.index_key(&record).unwrap().unwrap();

        assert!(
            full_key.starts_with(&leading_key),
            "leading field key must be a byte-prefix of the compound key"
        );
    }

    #[test]
    fn two_field_prefix_is_byte_prefix_of_three_field() {
        let two = compound(&[("a", IndexType::String), ("b", IndexType::Integer)]);
        let three = compound(&[
            ("a", IndexType::String),
            ("b", IndexType::Integer),
            ("c", IndexType::Boolean),
        ]);
        let record = json!({"a": "x", "b": 7, "c": true});
        let two_key = two.index_key(&record).unwrap().unwrap();
        let three_key = three.index_key(&record).unwrap().unwrap();
        assert!(three_key.starts_with(&two_key));
    }

    #[test]
    fn compound_sparse_when_any_field_missing_or_null() {
        let idx = compound(&[("a", IndexType::String), ("b", IndexType::Integer)]);
        assert_eq!(idx.index_key(&json!({"a": "x"})).unwrap(), None);
        assert_eq!(idx.index_key(&json!({"a": "x", "b": null})).unwrap(), None);
    }

    #[test]
    fn compound_errors_when_any_field_type_mismatches() {
        let idx = compound(&[("a", IndexType::String), ("b", IndexType::Integer)]);
        let err = idx.index_key(&json!({"a": "x", "b": "nope"})).unwrap_err();
        assert!(matches!(err, IndexKeyError::TypeMismatch { .. }));
    }

    #[test]
    fn compound_order_respects_field_order() {
        // Same leading field, differing second field -> order by second.
        let idx = compound(&[("a", IndexType::String), ("b", IndexType::Integer)]);
        let lo = idx.index_key(&json!({"a": "x", "b": 1})).unwrap().unwrap();
        let hi = idx.index_key(&json!({"a": "x", "b": 2})).unwrap().unwrap();
        assert!(lo < hi);

        // Differing leading field dominates regardless of second field.
        let a_big_b_small = idx.index_key(&json!({"a": "y", "b": 0})).unwrap().unwrap();
        assert!(hi < a_big_b_small, "leading field must dominate ordering");
    }
}
