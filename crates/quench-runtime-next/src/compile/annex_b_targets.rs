//! OXC rejects Annex B call-expression assignment targets before building an
//! AST. Mark the call as a synthetic member target so the ordinary parser can
//! build the surrounding program; the compiler recognizes and lowers that
//! marker as the Annex B runtime error.

const MARKER_BASE: &str = "__quench_annex_b_call_target__";

/// Normalization only bridges Annex B ordinary call targets. Validate its
/// synthetic members against the OXC AST before they acquire member semantics.
pub(super) fn invalid_target(
    program: &oxc_ast::ast::Program<'_>,
    marker: &str,
) -> Option<oxc_span::Span> {
    use oxc_ast::ast::{AssignmentExpression, Expression, StaticMemberExpression};
    use oxc_ast_visit::{Visit, walk};
    use oxc_span::GetSpan;

    struct Targets<'m> {
        marker: &'m str,
        invalid: Option<oxc_span::Span>,
    }
    impl<'a> Visit<'a> for Targets<'_> {
        fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
            if member.property.name == self.marker
                && !matches!(member.object.without_parentheses(),
                    Expression::CallExpression(call) if !call.optional)
            {
                self.invalid.get_or_insert(member.span);
            }
            walk::walk_static_member_expression(self, member);
        }

        fn visit_assignment_expression(&mut self, assignment: &AssignmentExpression<'a>) {
            if assignment.operator.is_logical()
                && let Some(oxc_ast::ast::SimpleAssignmentTarget::StaticMemberExpression(member)) =
                    assignment.left.as_simple_assignment_target()
                && member.property.name == self.marker
            {
                self.invalid.get_or_insert(assignment.left.span());
            }
            walk::walk_assignment_expression(self, assignment);
        }
    }
    if marker.is_empty() {
        return None;
    }
    let mut targets = Targets {
        marker,
        invalid: None,
    };
    targets.visit_program(program);
    targets.invalid
}

pub(super) struct NormalizedTargets {
    pub(super) source: String,
    pub(super) marker: String,
}

pub(super) fn normalize(source: &str) -> Option<NormalizedTargets> {
    let insertions = call_target_insertions(source);
    if insertions.is_empty() {
        return None;
    }

    let mut marker = MARKER_BASE.to_string();
    while source.contains(&marker) {
        marker.push('_');
    }
    let suffix = format!(".{marker}");
    let mut normalized = String::with_capacity(source.len() + suffix.len() * insertions.len());
    let mut start = 0;
    for insertion in insertions {
        normalized.push_str(&source[start..insertion]);
        normalized.push_str(&suffix);
        start = insertion;
    }
    normalized.push_str(&source[start..]);
    Some(NormalizedTargets {
        source: normalized,
        marker,
    })
}

fn call_target_insertions(source: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut opens = Vec::new();
    let mut insertions = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if let Some(end) = skipped_lexical_region(bytes, cursor) {
            cursor = end;
            continue;
        }
        match bytes[cursor] {
            b'(' => opens.push(cursor),
            b')' => {
                if let Some(open) = opens.pop()
                    && is_call_open(bytes, open)
                    && is_call_assignment_target(bytes, open, cursor, opens.last().copied())
                {
                    insertions.push(cursor + 1);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    insertions.sort_unstable();
    insertions.dedup();
    insertions
}

fn skipped_lexical_region(bytes: &[u8], start: usize) -> Option<usize> {
    match bytes.get(start..start + 2) {
        Some(b"//") => Some(line_end(bytes, start + 2)),
        Some(b"/*") => Some(block_end(bytes, start + 2)),
        _ => match bytes.get(start).copied() {
            Some(quote @ (b'\'' | b'"' | b'`')) => Some(quoted_end(bytes, start, quote)),
            _ => None,
        },
    }
}

fn line_end(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes
        .get(cursor)
        .is_some_and(|byte| !matches!(byte, b'\n' | b'\r'))
    {
        cursor += 1;
    }
    cursor
}

fn block_end(bytes: &[u8], mut cursor: usize) -> usize {
    while cursor + 1 < bytes.len() {
        if bytes[cursor..].starts_with(b"*/") {
            return cursor + 2;
        }
        cursor += 1;
    }
    bytes.len()
}

fn quoted_end(bytes: &[u8], mut cursor: usize, quote: u8) -> usize {
    cursor += 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor = (cursor + 2).min(bytes.len()),
            byte if byte == quote => return cursor + 1,
            _ => cursor += 1,
        }
    }
    bytes.len()
}

fn is_call_open(bytes: &[u8], open: usize) -> bool {
    let Some(previous) = previous_non_whitespace(bytes, open) else {
        return false;
    };
    if is_control_keyword_before(bytes, open, previous) {
        return false;
    }
    is_identifier_part(bytes[previous]) || matches!(bytes[previous], b')' | b']')
}

fn is_control_keyword_before(bytes: &[u8], open: usize, end: usize) -> bool {
    let mut start = end;
    while start > 0 && is_identifier_part(bytes[start - 1]) {
        start -= 1;
    }
    matches!(
        bytes.get(start..=end),
        Some(b"if" | b"while" | b"switch" | b"catch" | b"with" | b"for")
    ) && bytes
        .get(end + 1..open)
        .is_some_and(|between| between.iter().all(u8::is_ascii_whitespace))
}

fn is_call_assignment_target(
    bytes: &[u8],
    open: usize,
    close: usize,
    enclosing_open: Option<usize>,
) -> bool {
    let next = skip_whitespace(bytes, close + 1);
    let operators = [
        b"&&=".as_slice(),
        b"||=",
        b"??=",
        b">>>=",
        b"**=",
        b"<<=",
        b">>=",
        b"+=",
        b"-=",
        b"*=",
        b"/=",
        b"%=",
        b"&=",
        b"|=",
        b"^=",
        b"++",
        b"--",
    ];
    if starts_with_any(bytes, next, &operators) {
        return true;
    }
    if bytes.get(next) == Some(&b'=') && !matches!(bytes.get(next + 1), Some(b'=' | b'>')) {
        return true;
    }
    let is_loop_assignment = enclosing_open
        .is_some_and(|loop_open| is_for_keyword(bytes, loop_open))
        && [b"in".as_slice(), b"of"]
            .iter()
            .any(|word| starts_with_word(bytes, next, word));
    if is_loop_assignment {
        return true;
    }
    let Some(before_callee) = previous_non_whitespace(bytes, open) else {
        return false;
    };
    let mut callee_start = before_callee;
    while callee_start > 0 && is_identifier_part(bytes[callee_start - 1]) {
        callee_start -= 1;
    }
    previous_non_whitespace(bytes, callee_start)
        .is_some_and(|last| last > 0 && bytes[last] == b'+' && bytes[last - 1] == b'+')
}

fn is_for_keyword(bytes: &[u8], open: usize) -> bool {
    let end = previous_non_whitespace(bytes, open).map_or(open, |index| index + 1);
    let start = end.saturating_sub(b"for".len());
    bytes.get(start..end) == Some(b"for")
        && start
            .checked_sub(1)
            .is_none_or(|before| !is_identifier_part(bytes[before]))
}

fn starts_with_word(bytes: &[u8], start: usize, word: &[u8]) -> bool {
    bytes.get(start..start + word.len()) == Some(word)
        && bytes
            .get(start + word.len())
            .is_none_or(|byte| !is_identifier_part(*byte))
}

fn starts_with_any(bytes: &[u8], start: usize, choices: &[&[u8]]) -> bool {
    choices.iter().any(|choice| {
        bytes.get(start..start + choice.len()) == Some(*choice)
            && choice.last().is_some_and(|last| {
                !is_identifier_part(*last)
                    || bytes
                        .get(start + choice.len())
                        .is_none_or(|next| !is_identifier_part(*next))
            })
    })
}

fn skip_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    cursor
}

fn previous_non_whitespace(bytes: &[u8], mut cursor: usize) -> Option<usize> {
    while cursor > 0 {
        cursor -= 1;
        if !bytes[cursor].is_ascii_whitespace() {
            return Some(cursor);
        }
    }
    None
}

fn is_identifier_part(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}
