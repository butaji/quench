//! `base[key]` of a string key that names a property, as `o[k]` in a for-in body reads, by atom.
//! It is one of the reads `lane_index_slot_element` tries in its generic order; no site is
//! requickened to a specialized handler, so every `GetIndex` runs the generated `ELEMENT_READ` row.
use super::*;

// merge: theirs requickens a GetIndex site to a name-key-first or string-first handler
// (`lane_get_index_named`, `lane_get_index_string`, `named_key_read`). Ours runs every
// GetIndex through the generated ELEMENT_READ row and sets no requickening, so those
// handlers are not ported. Their results are the same reads, only the per-site order differs.
