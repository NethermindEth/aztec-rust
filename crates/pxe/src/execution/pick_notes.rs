//! Note filtering by select clauses — mirrors upstream `pick_notes.ts`.
//!
//! Both the private and utility oracles delegate to [`select_notes`] after
//! fetching the raw note set from the note store.

use crate::stores::note_store::StoredNote;
use aztec_core::types::Fr;

// ---------------------------------------------------------------------------
// Select clause
// ---------------------------------------------------------------------------

/// A single select filter parsed from the oracle arguments.
#[derive(Debug, Clone)]
pub struct SelectClause {
    /// Index of the field in the packed note content.
    pub index: usize,
    /// Byte offset within the field.
    pub offset: usize,
    /// Number of bytes to compare (0 means full field = 32 bytes).
    pub length: usize,
    /// Value to compare against.
    pub value: Fr,
    /// Comparator (1=EQ, 2=NEQ, 3=LT, 4=LTE, 5=GT, 6=GTE).
    pub comparator: u8,
}

// ---------------------------------------------------------------------------
// Comparator constants
// ---------------------------------------------------------------------------

const EQ: u8 = 1;
const NEQ: u8 = 2;
const LT: u8 = 3;
const LTE: u8 = 4;
const GT: u8 = 5;
const GTE: u8 = 6;

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse select clauses from the oracle `get_notes` arguments.
///
/// Layout:
/// - `args[3]`  → numSelects (single value)
/// - `args[4]`  → selectByIndexes (array)
/// - `args[5]`  → selectByOffsets (array)
/// - `args[6]`  → selectByLengths (array)
/// - `args[7]`  → selectValues (array)
/// - `args[8]`  → selectComparators (array)
pub fn parse_select_clauses(args: &[Vec<Fr>]) -> Vec<SelectClause> {
    let num_selects = args
        .get(3)
        .and_then(|v| v.first())
        .map(Fr::to_usize)
        .unwrap_or(0);

    if num_selects == 0 {
        return Vec::new();
    }

    let indexes = args.get(4).cloned().unwrap_or_default();
    let offsets = args.get(5).cloned().unwrap_or_default();
    let lengths = args.get(6).cloned().unwrap_or_default();
    let values = args.get(7).cloned().unwrap_or_default();
    let comparators = args.get(8).cloned().unwrap_or_default();

    (0..num_selects)
        .map(|i| SelectClause {
            index: indexes.get(i).map(Fr::to_usize).unwrap_or(0),
            offset: offsets.get(i).map(Fr::to_usize).unwrap_or(0),
            length: lengths.get(i).map(Fr::to_usize).unwrap_or(0),
            value: values.get(i).copied().unwrap_or(Fr::zero()),
            comparator: comparators.get(i).map(|f| f.to_usize() as u8).unwrap_or(0),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/// Extract a property value from a note's packed content, applying the
/// sub-field selector (offset/length). Returns an `Fr` suitable for
/// comparison.
fn extract_property(note_data: &[Fr], clause: &SelectClause) -> Option<Fr> {
    let field = note_data.get(clause.index)?;

    // Full-field comparison (most common case).
    if clause.offset == 0 && (clause.length == 0 || clause.length >= 32) {
        return Some(*field);
    }

    // Sub-field extraction: convert to big-endian bytes, slice, convert back.
    let bytes = field.to_be_bytes();
    let end = (clause.offset + clause.length).min(32);
    let mut buf = [0u8; 32];
    let slice = &bytes[clause.offset..end];
    // Right-align in 32-byte buffer for correct numeric value.
    buf[32 - slice.len()..].copy_from_slice(slice);
    Some(Fr::from(buf))
}

/// Compare two field elements using the given comparator.
fn compare(note_val: &Fr, select_val: &Fr, comparator: u8) -> bool {
    match comparator {
        EQ => note_val == select_val,
        NEQ => note_val != select_val,
        LT => note_val < select_val,
        LTE => note_val <= select_val,
        GT => note_val > select_val,
        GTE => note_val >= select_val,
        _ => true, // unknown comparator → don't filter
    }
}

/// Filter notes by all select clauses (AND semantics).
pub fn select_notes(notes: Vec<StoredNote>, clauses: &[SelectClause]) -> Vec<StoredNote> {
    if clauses.is_empty() {
        return notes;
    }
    notes
        .into_iter()
        .filter(|note| {
            clauses.iter().all(|clause| {
                extract_property(&note.note_data, clause)
                    .map_or(false, |val| compare(&val, &clause.value, clause.comparator))
            })
        })
        .collect()
}
