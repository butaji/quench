//! OXC-owned call boundaries shared by compilation and error descriptions.
use oxc_ast::ast::{CallExpression, Expression};
use oxc_span::{GetSpan, Span};

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

/// The parser's spans become the runtime's `SourceSpan` here, once, at the front end: the
/// declared environment and the bytecode keep no parser span type.
impl From<Span> for crate::bytecode::SourceSpan {
    fn from(span: Span) -> Self {
        Self { start: span.start, end: span.end }
    }
}

/// The bytecode numbers operators as the parser does, and `crate::operator`'s enums carry the
/// same discriminants. A renumbered parser fails this build instead of mis-decoding bytecode.
const _: () = {
    use crate::operator::{BinaryOperator as RuntimeBinary, UnaryOperator as RuntimeUnary};
    use oxc_syntax::operator::{BinaryOperator as ParserBinary, UnaryOperator as ParserUnary};
    const BINARY: &[(ParserBinary, RuntimeBinary)] = &[
        (ParserBinary::Equality, RuntimeBinary::Equality),
        (ParserBinary::Inequality, RuntimeBinary::Inequality),
        (ParserBinary::StrictEquality, RuntimeBinary::StrictEquality),
        (ParserBinary::StrictInequality, RuntimeBinary::StrictInequality),
        (ParserBinary::LessThan, RuntimeBinary::LessThan),
        (ParserBinary::LessEqualThan, RuntimeBinary::LessEqualThan),
        (ParserBinary::GreaterThan, RuntimeBinary::GreaterThan),
        (ParserBinary::GreaterEqualThan, RuntimeBinary::GreaterEqualThan),
        (ParserBinary::Addition, RuntimeBinary::Addition),
        (ParserBinary::Subtraction, RuntimeBinary::Subtraction),
        (ParserBinary::Multiplication, RuntimeBinary::Multiplication),
        (ParserBinary::Division, RuntimeBinary::Division),
        (ParserBinary::Remainder, RuntimeBinary::Remainder),
        (ParserBinary::Exponential, RuntimeBinary::Exponential),
        (ParserBinary::ShiftLeft, RuntimeBinary::ShiftLeft),
        (ParserBinary::ShiftRight, RuntimeBinary::ShiftRight),
        (ParserBinary::ShiftRightZeroFill, RuntimeBinary::ShiftRightZeroFill),
        (ParserBinary::BitwiseOR, RuntimeBinary::BitwiseOR),
        (ParserBinary::BitwiseXOR, RuntimeBinary::BitwiseXOR),
        (ParserBinary::BitwiseAnd, RuntimeBinary::BitwiseAnd),
        (ParserBinary::In, RuntimeBinary::In),
        (ParserBinary::Instanceof, RuntimeBinary::Instanceof),
    ];
    const UNARY: &[(ParserUnary, RuntimeUnary)] = &[
        (ParserUnary::UnaryPlus, RuntimeUnary::UnaryPlus),
        (ParserUnary::UnaryNegation, RuntimeUnary::UnaryNegation),
        (ParserUnary::LogicalNot, RuntimeUnary::LogicalNot),
        (ParserUnary::BitwiseNot, RuntimeUnary::BitwiseNot),
        (ParserUnary::Typeof, RuntimeUnary::Typeof),
        (ParserUnary::Void, RuntimeUnary::Void),
        (ParserUnary::Delete, RuntimeUnary::Delete),
    ];
    let mut index = 0;
    while index < BINARY.len() {
        assert!(BINARY[index].0 as u32 == BINARY[index].1 as u32, "a binary operator's numbering diverged");
        index += 1;
    }
    let mut index = 0;
    while index < UNARY.len() {
        assert!(UNARY[index].0 as u32 == UNARY[index].1 as u32, "a unary operator's numbering diverged");
        index += 1;
    }
};
