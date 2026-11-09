//! Data scripts: a script whose first statement declares or assigns a JSON array or object,
//! as JSONP responses and generated data files are. Their literal's syntax tree takes
//! several times the text's size (the 7.3 MB SeaMonster inspector payload builds a tree of
//! about 80 MB), so OXC parses the script with the literal replaced by an empty one of the
//! same kind and length, and the code generator builds the value from the JSON text in the
//! program's source (`LiteralStep::Json`), as JSC's `LiteralParser` takes JSONP scripts.
//! The JSON grammar is a subset of the literal grammar with the same values, but for a
//! `__proto__` key, which a literal takes as its prototype (`json_literal_length`).

use oxc_ast::ast::{AssignmentOperator, Expression, Program, Statement};

/// A script's JSON literal, at byte `start` of its text, and the text OXC parses instead.
pub(crate) struct DataLiteral {
    pub(crate) start: u32,
    pub(crate) substituted: String,
}

/// The JSON literal `text` assigns first: after leading whitespace and comments, a target
/// of identifier characters, dots and whitespace (`let data`, `module.exports`), `=`, and a
/// JSON array or object. The target is only located here; OXC parses the statement, and
/// `is_top_level_value` admits the literal only where the parsed statement takes it.
pub(crate) fn locate(text: &str) -> Option<DataLiteral> {
    let bytes = text.as_bytes();
    let mut at = skip_trivia(bytes, 0);
    while bytes.get(at).is_some_and(|&byte| is_target_byte(byte)) {
        at += 1;
    }
    if bytes.get(at) != Some(&b'=') || bytes.get(at + 1) == Some(&b'=') {
        return None;
    }
    at += 1;
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    let length = crate::vm::json_literal_length(&text[at..])?;
    Some(DataLiteral {
        start: u32::try_from(at).ok()?,
        substituted: substitute(text, at..at + length),
    })
}

/// Whether the empty literal at `start` is the value a top-level declaration or plain
/// assignment of `program` takes: there the JSON text, which is a literal of the same kind,
/// is a whole expression with the same surroundings.
pub(crate) fn is_top_level_value(program: &Program<'_>, start: u32) -> bool {
    let placeholder = |value: &Expression<'_>| match value {
        Expression::ArrayExpression(array) => array.span.start == start && array.elements.is_empty(),
        Expression::ObjectExpression(object) => object.span.start == start && object.properties.is_empty(),
        _ => false,
    };
    program.body.iter().any(|statement| match statement {
        Statement::VariableDeclaration(declaration) => declaration
            .declarations
            .iter()
            .any(|declarator| declarator.init.as_ref().is_some_and(placeholder)),
        Statement::ExpressionStatement(statement) => matches!(
            &statement.expression,
            Expression::AssignmentExpression(assignment)
                if assignment.operator == AssignmentOperator::Assign && placeholder(&assignment.right)
        ),
        _ => false,
    })
}

/// `text` with the JSON text at `literal` replaced by the empty literal of its kind: its
/// brackets stay and the bytes between them are blank but for line terminators, so every
/// other byte and line keeps its place.
fn substitute(text: &str, literal: std::ops::Range<usize>) -> String {
    let inner = &text[literal.start + 1..literal.end - 1];
    let mut substituted = String::with_capacity(text.len());
    substituted.push_str(&text[..literal.start + 1]);
    for character in inner.chars() {
        if matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            substituted.push(character);
        } else {
            substituted.extend(std::iter::repeat_n(' ', character.len_utf8()));
        }
    }
    substituted.push_str(&text[literal.end - 1..]);
    debug_assert_eq!(substituted.len(), text.len());
    substituted
}

/// The index past the whitespace and comments from `at`.
fn skip_trivia(bytes: &[u8], mut at: usize) -> usize {
    loop {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let rest = &bytes[at..];
        if rest.starts_with(b"/*") {
            let Some(end) = rest.windows(2).position(|pair| pair == b"*/") else {
                return at;
            };
            at += end + 2;
        } else if rest.starts_with(b"//") {
            at += rest.iter().position(|&byte| byte == b'\n').unwrap_or(rest.len());
        } else {
            return at;
        }
    }
}

/// Whether `byte` may be part of a located target: identifier characters, member dots and
/// the whitespace between a declaration's keyword and its name.
fn is_target_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'.') || byte.is_ascii_whitespace()
}
