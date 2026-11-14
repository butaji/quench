//! The opcode catalog as the lint reads it: the rows of the `opcodes!` table (`bytecode.rs`),
//! each opcode with the columns it declares (`class`, `staged`, `step`, ...). The census and
//! the native-form measure count from the same rows, so a question about the catalog has one
//! answer.

use std::collections::BTreeMap;

use proc_macro2::{TokenStream, TokenTree};
use syn::Item;

/// The macro whose invocation is the table.
const TABLE_MACRO: &str = "opcodes";
/// The column naming the decoder of an opcode's staged operation.
pub const STAGED: &str = "staged";
/// The column naming the generic step a lane or a body makes in place.
pub const STEP: &str = "step";
/// The column naming an opcode's operation family.
pub const CLASS: &str = "class";
/// The staged decoders (`staged` column) whose rows the lane steps by their definitions: the
/// register, window and capture rows (`lane_region/register_rows.rs` and kin).
pub const LANE_DECODERS: [&str; 3] = ["Flow", "Value", "Local"];
/// The column naming the JavaScript denotation a lowering consumes in place of a family name.
pub const JS: &str = "js";
/// The `step` value of a lane step whose result the lane writes back (a call-out row).
pub const RESULT_STEP: &str = "LaneResult";

/// One opcode of the table: the idents each column of its row declares.
pub struct Row {
    /// The opcode's name.
    pub name: String,
    columns: BTreeMap<String, Vec<String>>,
}

impl Row {
    /// Whether the row declares `column`.
    pub fn declares(&self, column: &str) -> bool {
        self.columns.contains_key(column)
    }

    /// The first value `column` declares.
    pub fn value(&self, column: &str) -> Option<&str> {
        self.columns.get(column)?.first().map(String::as_str)
    }
}

/// Whether `tree` is the punctuation `mark`.
pub fn is_punct(tree: Option<&TokenTree>, mark: char) -> bool {
    matches!(tree, Some(TokenTree::Punct(punct)) if punct.as_char() == mark)
}

/// Whether the tree at `at` opens a row: an opcode name before `=>`, after the table's start or
/// the previous row's end.
fn opens_row(trees: &[TokenTree], at: usize) -> bool {
    let after = at.checked_sub(1).and_then(|before| trees.get(before));
    let starts = at == 0 || is_punct(after, ',') || is_punct(after, ';');
    starts && matches!(trees[at], TokenTree::Ident(_)) && is_punct(trees.get(at + 1), '=')
}

/// The column a clause declares: its keyword and the idents up to the row's next comma.
fn column(clause: &[TokenTree]) -> Option<(String, Vec<String>)> {
    let Some(TokenTree::Ident(keyword)) = clause.first() else { return None };
    let values = clause[1..]
        .iter()
        .take_while(|tree| !is_punct(Some(*tree), ','))
        .filter_map(|tree| match tree {
            TokenTree::Ident(value) => Some(value.to_string()),
            _ => None,
        })
        .collect();
    Some((keyword.to_string(), values))
}

/// The row of `trees`, which start at its opcode name: the clauses after the effect, each
/// separated by `;`.
fn row(trees: &[TokenTree]) -> Row {
    let name = trees.first().map(ToString::to_string).unwrap_or_default();
    Row { name, columns: trees[1..].split(|tree| is_punct(Some(tree), ';')).skip(1).filter_map(column).collect() }
}

/// The rows of the `opcodes!` table `file` invokes, in table order.
pub fn table(file: &syn::File) -> Vec<Row> {
    let trees: Vec<TokenTree> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Macro(call) if call.mac.path.is_ident(TABLE_MACRO) => Some(call.mac.tokens.clone()),
            _ => None,
        })
        .flat_map(TokenStream::into_iter)
        .collect();
    let starts: Vec<usize> = (0..trees.len()).filter(|&at| opens_row(&trees, at)).collect();
    let ends = starts.iter().skip(1).copied().chain([trees.len()]);
    starts.iter().copied().zip(ends).map(|(from, to)| row(&trees[from..to])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(source: &str) -> Vec<Row> {
        table(&syn::parse_str(source).expect("test source parses"))
    }

    #[test]
    fn a_row_declares_the_columns_it_names() {
        let found = rows(
            "opcodes!($; Nop => Effect::PURE; layout Scalar; class Nop; step Lane, @ R, @ fields(A, B, C),
             Add => X::Y.union(Z::W); layout Scalar; staged Flow; step LaneResult; traits A B, @ R, @ fields(A, B, C));",
        );
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].value(CLASS), found[0].value(STEP)), (Some("Nop"), Some("Lane")));
        assert!(!found[0].declares(STAGED) && found[1].declares(STAGED));
        assert_eq!(found[1].value(STEP), Some(RESULT_STEP));
        assert_eq!(found[1].columns["traits"], ["A", "B"]);
    }

    #[test]
    fn other_macros_are_no_table() {
        assert!(rows("macro_rules! opcodes { () => {} } fn f() {}").is_empty());
    }
}
