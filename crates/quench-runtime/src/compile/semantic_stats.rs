//! Capacity hints for OXC's semantic analysis, taken from the source length.
//!
//! The builder sizes its node, scope, symbol and reference tables from a count of the tree,
//! a whole walk before the analysis itself; the hints are the sizes a program of that many
//! bytes has: a little under a source of mostly code (minified text has 0.21 to 0.37 nodes
//! a byte), and the tables grow past them.

use oxc_ast::ast::Program;
use oxc_semantic::{SemanticBuilder, Stats};

/// Source bytes to one AST node.
const BYTES_PER_NODE: u32 = 4;
/// Source bytes to one lexical scope.
const BYTES_PER_SCOPE: u32 = 128;
/// Source bytes to one symbol.
const BYTES_PER_SYMBOL: u32 = 100;
/// Source bytes to one identifier reference.
const BYTES_PER_REFERENCE: u32 = 20;

/// A semantic analysis of `program` sized from its source length.
pub(super) fn builder<'a>(program: &Program<'_>) -> SemanticBuilder<'a> {
    let bytes = program.span.end.saturating_sub(program.span.start);
    SemanticBuilder::new().with_stats(Stats::new(
        bytes / BYTES_PER_NODE,
        bytes / BYTES_PER_SCOPE,
        bytes / BYTES_PER_SYMBOL,
        bytes / BYTES_PER_REFERENCE,
    ))
}
