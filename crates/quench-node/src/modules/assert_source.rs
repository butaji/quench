//! The source of a failed `assert.ok` call (`lib/assert.js` `getErrMessage`/`parseCode`): the
//! innermost call expression around the caller's position, on its first line.
use oxc_ast::ast::CallExpression;
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;

/// The innermost call expression whose span holds `offset`.
struct Innermost {
    offset: u32,
    found: Option<(u32, u32)>,
}

impl<'a> Visit<'a> for Innermost {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        let span = call.span();
        if span.start <= self.offset && self.offset <= span.end {
            self.found = Some((span.start, span.end));
        }
        walk::walk_call_expression(self, call);
    }
}

/// The byte offset of 1-based `line` and `column` (in UTF-16 units) in `source`.
fn offset(source: &str, line: u32, column: u32) -> Option<usize> {
    let start = if line <= 1 {
        0
    } else {
        source.match_indices('\n').nth(line as usize - 2).map(|(index, _)| index + 1)?
    };
    let mut units = 0;
    for (index, character) in source[start..].char_indices() {
        if units + 1 >= column as usize || character == '\n' {
            return Some(start + index);
        }
        units += character.len_utf16();
    }
    Some(source.len())
}

/// The call expression at `line`/`column` of `source`, as `getErrMessage` writes it.
pub(super) fn call_expression_at(source: &str, line: u32, column: u32) -> Option<String> {
    let position = offset(source, line, column)?;
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::cjs()).parse();
    let mut innermost = Innermost { offset: u32::try_from(position).ok()?, found: None };
    innermost.visit_program(&parsed.program);
    let (start, end) = innermost.found?;
    let text = &source[start as usize..end as usize];
    // Node reads the call's first line only.
    Some(text.split('\n').next()?.trim_end_matches('\r').to_owned())
}
