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

#[cfg(test)]
mod tests {
    use super::builder;
    use oxc_allocator::Allocator;
    use oxc_span::SourceType;

    /// The hints size tables and decide nothing: the analysis of a source reports what the
    /// counted one does, for a source with more nodes than the hint and one with an error.
    #[test]
    fn hints_leave_the_early_errors_unchanged() {
        let dense = format!("var a = {};", "[1,".repeat(200) + &"]".repeat(200));
        for source in ["let a = 1; let a = 2;", "function f(x) { return x + 1; } f(1);", dense.as_str()] {
            let allocator = Allocator::default();
            let parsed = oxc_parser::Parser::new(&allocator, source, SourceType::script()).parse();
            let hinted = builder(&parsed.program).with_check_syntax_error(true).build(&parsed.program);
            let counted = oxc_semantic::SemanticBuilder::new().with_check_syntax_error(true).build(&parsed.program);
            assert_eq!(hinted.diagnostics.len(), counted.diagnostics.len(), "{source}");
        }
    }
}
