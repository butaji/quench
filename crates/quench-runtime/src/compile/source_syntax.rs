//! OXC-owned call boundaries shared by compilation and error descriptions.
use oxc_ast::ast::{CallExpression, Expression};
use oxc_span::GetSpan;

pub(crate) fn call_offset<C: GetSpan>(call: &CallExpression<'_>, text: &str, comments: &[C]) -> u32 {
    if !call.optional {
        match &call.callee {
            Expression::Identifier(_) | Expression::Super(_) => return call.callee.span().start,
            Expression::StaticMemberExpression(member) => return member.property.span.start,
            _ => (),
        }
    }
    let after = call.type_arguments.as_ref().map_or_else(|| call.callee.span().end, |arguments| arguments.span.end);
    delimiter(text, comments, after, call.span.end, '(')
}

/// Find a delimiter between syntax boundaries, excluding OXC comment spans.
pub(crate) fn delimiter<C: GetSpan>(text: &str, comments: &[C], start: u32, end: u32, delimiter: char) -> u32 {
    let mut cursor = start as usize;
    let end = (end as usize).min(text.len());
    let mut comment = comments.partition_point(|comment| comment.span().end <= start);
    while cursor < end {
        if let Some(span) = comments.get(comment).map(GetSpan::span)
            && span.start as usize <= cursor && cursor < span.end as usize {
                cursor = span.end as usize;
                comment += 1;
                continue;
            }
        let Some(character) = text[cursor..end].chars().next() else { break };
        if character == delimiter { return cursor as u32; }
        cursor += character.len_utf8();
    }
    start
}
