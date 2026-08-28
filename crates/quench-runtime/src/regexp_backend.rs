//! Repository-owned ECMAScript RegExp parser, IR, and interpreter.
//!
//! OXC supplies syntax facts; the VM owns the executable representation and
//! matching algorithm. No third-party regular-expression engine is involved.

use std::ops::Range;

use oxc::regular_expression::{ast, LiteralParser, Options};

#[derive(Clone, Copy, Default)]
pub(crate) struct Flags {
    pub(crate) ignore_case: bool,
    pub(crate) multiline: bool,
    pub(crate) dot_all: bool,
    pub(crate) unicode: bool,
    pub(crate) unicode_sets: bool,
    reverse: bool,
}

impl From<&str> for Flags {
    fn from(flags: &str) -> Self {
        Self {
            ignore_case: flags.contains('i'),
            multiline: flags.contains('m'),
            dot_all: flags.contains('s'),
            unicode: flags.contains('u'),
            unicode_sets: flags.contains('v'),
            reverse: false,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Match {
    pub(crate) range: Range<usize>,
    pub(crate) captures: Vec<Option<Range<usize>>>,
    named: Vec<String>,
}

impl Match {
    pub(crate) fn start(&self) -> usize {
        self.range.start
    }
    pub(crate) fn end(&self) -> usize {
        self.range.end
    }
    pub(crate) fn groups(&self) -> impl Iterator<Item = Option<Range<usize>>> + '_ {
        std::iter::once(Some(self.range.clone())).chain(self.captures.iter().cloned())
    }
    pub(crate) fn named_groups(&self) -> impl Iterator<Item = (&str, Option<Range<usize>>)> + '_ {
        self.named
            .iter()
            .enumerate()
            .filter(|(_, name)| !name.is_empty())
            .map(|(index, name)| (name.as_str(), self.captures.get(index).cloned().flatten()))
    }
}

pub(crate) struct Matches {
    item: Option<Match>,
}
impl Iterator for Matches {
    type Item = Match;
    fn next(&mut self) -> Option<Self::Item> {
        self.item.take()
    }
}

#[derive(Clone)]
enum Expr {
    Sequence(Vec<Expr>),
    Alternation(Vec<Expr>),
    Literal(u32),
    Dot,
    Class(ClassExpr),
    Capture {
        index: usize,
        body: Box<Expr>,
    },
    Repeat {
        body: Box<Expr>,
        min: usize,
        max: Option<usize>,
        greedy: bool,
    },
    Assertion(Assertion),
    Lookaround {
        kind: Lookaround,
        body: Box<Expr>,
    },
    Backreference(Backreference),
    Mode {
        body: Box<Expr>,
        flags: ModeFlags,
    },
}

#[derive(Clone, Copy)]
enum Assertion {
    Start,
    End,
    Boundary,
    NonBoundary,
}
#[derive(Clone, Copy)]
enum Lookaround {
    Ahead,
    NegativeAhead,
    Behind,
    NegativeBehind,
}
#[derive(Clone)]
enum Backreference {
    Index(usize),
    Name(String),
}
#[derive(Clone, Copy, Default)]
struct ModeFlags {
    ignore_case: Option<bool>,
    multiline: Option<bool>,
    dot_all: Option<bool>,
}

#[derive(Clone)]
struct ClassExpr {
    negative: bool,
    kind: ClassKind,
    items: Vec<ClassItem>,
}
#[derive(Clone, Copy)]
enum ClassKind {
    Union,
    Intersection,
    Subtraction,
}
#[derive(Clone)]
enum ClassItem {
    Character(u32),
    Range(u32, u32),
    Escape(ast::CharacterClassEscapeKind),
    Property {
        negative: bool,
        name: String,
        value: Option<String>,
    },
    Nested(ClassExpr),
}

struct Lowering {
    names: Vec<String>,
}
#[derive(Clone, Copy)]
struct Unit {
    value: u32,
    offset_start: usize,
    offset_end: usize,
}
#[derive(Clone)]
struct State {
    position: usize,
    captures: Vec<Option<Range<usize>>>,
}

const MAX_BACKTRACK_STATES: usize = 4096;

pub(crate) struct Regex {
    program: Expr,
    flags: Flags,
    capture_names: Vec<String>,
}

impl Regex {
    pub(crate) fn with_flags(source: &str, flags: Flags) -> Result<Self, String> {
        let allocator = oxc::allocator::Allocator::default();
        crate::regexp::validate_pattern(source)?;
        let flag_text = flag_text(flags);
        let parsed = LiteralParser::new(&allocator, source, Some(&flag_text), Options::default())
            .parse()
            .map_err(|error| error.to_string())?;
        let mut lowering = Lowering { names: Vec::new() };
        let program = lower_disjunction(&parsed.body, &mut lowering);
        Ok(Self {
            program,
            flags,
            capture_names: lowering.names,
        })
    }

    pub(crate) fn find_from(&self, text: &str, start: usize) -> Matches {
        let units = units_from_str(text, self.flags.unicode || self.flags.unicode_sets);
        let first = units
            .iter()
            .position(|unit| unit.offset_start >= start)
            .unwrap_or(units.len());
        Matches {
            item: self.find_units(&units, first),
        }
    }

    pub(crate) fn find_from_utf16(&self, input: &[u16], start: usize) -> Matches {
        let units = input
            .iter()
            .enumerate()
            .map(|(index, value)| Unit {
                value: u32::from(*value),
                offset_start: index,
                offset_end: index + 1,
            })
            .collect::<Vec<_>>();
        Matches {
            item: self.find_units(&units, start),
        }
    }

    fn find_units(&self, input: &[Unit], first: usize) -> Option<Match> {
        for position in first..=input.len() {
            let state = State {
                position,
                captures: vec![None; self.capture_names.len()],
            };
            if let Some(mut found) = match_expr(&self.program, input, state, self.flags) {
                let start = input.get(position).map_or_else(
                    || input.last().map_or(0, |unit| unit.offset_end),
                    |unit| unit.offset_start,
                );
                let end = input.get(found.position).map_or_else(
                    || input.last().map_or(start, |unit| unit.offset_end),
                    |unit| unit.offset_start,
                );
                let captures = found
                    .captures
                    .iter_mut()
                    .map(|capture| {
                        capture.take().map(|range| {
                            let capture_start =
                                input.get(range.start).map_or(end, |unit| unit.offset_start);
                            let capture_end =
                                input.get(range.end).map_or(end, |unit| unit.offset_start);
                            capture_start..capture_end
                        })
                    })
                    .collect();
                return Some(Match {
                    range: start..end,
                    captures,
                    named: self.capture_names.clone(),
                });
            }
        }
        None
    }
}

fn flag_text(flags: Flags) -> String {
    let mut text = String::new();
    if flags.ignore_case {
        text.push('i');
    }
    if flags.multiline {
        text.push('m');
    }
    if flags.dot_all {
        text.push('s');
    }
    if flags.unicode {
        text.push('u');
    }
    if flags.unicode_sets {
        text.push('v');
    }
    text
}

fn units_from_str(text: &str, unicode: bool) -> Vec<Unit> {
    if unicode {
        return text
            .char_indices()
            .map(|(offset_start, character)| Unit {
                value: u32::from(character),
                offset_start,
                offset_end: offset_start + character.len_utf8(),
            })
            .collect();
    }
    text.char_indices()
        .flat_map(|(offset_start, character)| {
            let offset_end = offset_start + character.len_utf8();
            let mut buffer = [0; 2];
            character
                .encode_utf16(&mut buffer)
                .iter()
                .map(move |value| Unit {
                    value: u32::from(*value),
                    offset_start,
                    offset_end,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn lower_disjunction(disjunction: &ast::Disjunction<'_>, lowering: &mut Lowering) -> Expr {
    let alternatives = disjunction
        .body
        .iter()
        .map(|alternative| {
            Expr::Sequence(
                alternative
                    .body
                    .iter()
                    .map(|term| lower_term(term, lowering))
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    if alternatives.len() == 1 {
        alternatives
            .into_iter()
            .next()
            .unwrap_or(Expr::Sequence(Vec::new()))
    } else {
        Expr::Alternation(alternatives)
    }
}

fn lower_term(term: &ast::Term<'_>, lowering: &mut Lowering) -> Expr {
    match term {
        ast::Term::BoundaryAssertion(assertion) => Expr::Assertion(match assertion.kind {
            ast::BoundaryAssertionKind::Start => Assertion::Start,
            ast::BoundaryAssertionKind::End => Assertion::End,
            ast::BoundaryAssertionKind::Boundary => Assertion::Boundary,
            ast::BoundaryAssertionKind::NegativeBoundary => Assertion::NonBoundary,
        }),
        ast::Term::LookAroundAssertion(assertion) => Expr::Lookaround {
            kind: match assertion.kind {
                ast::LookAroundAssertionKind::Lookahead => Lookaround::Ahead,
                ast::LookAroundAssertionKind::NegativeLookahead => Lookaround::NegativeAhead,
                ast::LookAroundAssertionKind::Lookbehind => Lookaround::Behind,
                ast::LookAroundAssertionKind::NegativeLookbehind => Lookaround::NegativeBehind,
            },
            body: Box::new(lower_disjunction(&assertion.body, lowering)),
        },
        ast::Term::Quantifier(quantifier) => Expr::Repeat {
            body: Box::new(lower_term(&quantifier.body, lowering)),
            min: quantifier.min.min(usize::MAX as u64) as usize,
            max: quantifier
                .max
                .map(|max| max.min(usize::MAX as u64) as usize),
            greedy: quantifier.greedy,
        },
        ast::Term::Character(character) => Expr::Literal(character.value),
        ast::Term::Dot(_) => Expr::Dot,
        ast::Term::CharacterClassEscape(escape) => Expr::Class(ClassExpr {
            negative: false,
            kind: ClassKind::Union,
            items: vec![ClassItem::Escape(escape.kind)],
        }),
        ast::Term::UnicodePropertyEscape(property) => Expr::Class(ClassExpr {
            negative: false,
            kind: ClassKind::Union,
            items: vec![ClassItem::Property {
                negative: property.negative,
                name: property.name.to_string(),
                value: property.value.as_ref().map(ToString::to_string),
            }],
        }),
        ast::Term::CharacterClass(class) => Expr::Class(lower_class(class, lowering)),
        ast::Term::CapturingGroup(group) => {
            let index = lowering.names.len();
            lowering.names.push(
                group
                    .name
                    .as_ref()
                    .map_or_else(String::new, |name| name.to_string()),
            );
            Expr::Capture {
                index,
                body: Box::new(lower_disjunction(&group.body, lowering)),
            }
        }
        ast::Term::IgnoreGroup(group) => {
            let body = lower_disjunction(&group.body, lowering);
            let flags = group
                .modifiers
                .as_ref()
                .map_or(ModeFlags::default(), |modifiers| ModeFlags {
                    ignore_case: modifier_mode(
                        modifiers.enabling.as_ref(),
                        modifiers.disabling.as_ref(),
                        |modifier| modifier.ignore_case,
                    ),
                    multiline: modifier_mode(
                        modifiers.enabling.as_ref(),
                        modifiers.disabling.as_ref(),
                        |modifier| modifier.multiline,
                    ),
                    dot_all: modifier_mode(
                        modifiers.enabling.as_ref(),
                        modifiers.disabling.as_ref(),
                        |modifier| modifier.sticky,
                    ),
                });
            Expr::Mode {
                body: Box::new(body),
                flags,
            }
        }
        ast::Term::IndexedReference(reference) => {
            Expr::Backreference(Backreference::Index(reference.index as usize))
        }
        ast::Term::NamedReference(reference) => {
            let index = lowering
                .names
                .iter()
                .position(|name| name == reference.name.as_str())
                .map_or(0, |index| index + 1);
            Expr::Backreference(Backreference::Index(index))
        }
    }
}

fn modifier_mode(
    enabling: Option<&ast::Modifier>,
    disabling: Option<&ast::Modifier>,
    enabled: impl Fn(&ast::Modifier) -> bool,
) -> Option<bool> {
    if enabling.is_some_and(|modifier| enabled(modifier)) {
        Some(true)
    } else if disabling.is_some_and(|modifier| enabled(modifier)) {
        Some(false)
    } else {
        None
    }
}

fn lower_class(class: &ast::CharacterClass<'_>, lowering: &mut Lowering) -> ClassExpr {
    let items = class
        .body
        .iter()
        .flat_map(|item| match item {
            ast::CharacterClassContents::CharacterClassRange(range) => {
                vec![ClassItem::Range(range.min.value, range.max.value)]
            }
            ast::CharacterClassContents::CharacterClassEscape(escape) => {
                vec![ClassItem::Escape(escape.kind)]
            }
            ast::CharacterClassContents::UnicodePropertyEscape(property) => {
                vec![ClassItem::Property {
                    negative: property.negative,
                    name: property.name.to_string(),
                    value: property.value.as_ref().map(ToString::to_string),
                }]
            }
            ast::CharacterClassContents::Character(character) => {
                vec![ClassItem::Character(character.value)]
            }
            ast::CharacterClassContents::NestedCharacterClass(nested) => {
                vec![ClassItem::Nested(lower_class(nested, lowering))]
            }
            ast::CharacterClassContents::ClassStringDisjunction(strings) => strings
                .body
                .iter()
                .flat_map(|string| {
                    string
                        .body
                        .iter()
                        .map(|character| ClassItem::Character(character.value))
                })
                .collect(),
        })
        .collect();
    ClassExpr {
        negative: class.negative,
        kind: match class.kind {
            ast::CharacterClassContentsKind::Union => ClassKind::Union,
            ast::CharacterClassContentsKind::Intersection => ClassKind::Intersection,
            ast::CharacterClassContentsKind::Subtraction => ClassKind::Subtraction,
        },
        items,
    }
}

fn match_expr(expr: &Expr, input: &[Unit], state: State, flags: Flags) -> Option<State> {
    match expr {
        Expr::Sequence(parts) => match_sequence(parts, input, state, flags),
        Expr::Alternation(alternatives) => alternatives
            .iter()
            .find_map(|alternative| match_expr(alternative, input, state.clone(), flags)),
        Expr::Literal(value) => input
            .get(state.position)
            .filter(|unit| equal(*value, unit.value, flags.ignore_case))
            .map(|_| State {
                position: state.position + 1,
                ..state
            }),
        Expr::Dot => input
            .get(state.position)
            .filter(|unit| flags.dot_all || !is_line_terminator(unit.value))
            .map(|_| State {
                position: state.position + 1,
                ..state
            }),
        Expr::Class(class) => input
            .get(state.position)
            .filter(|unit| class_matches(class, unit.value, flags.ignore_case))
            .map(|_| State {
                position: state.position + 1,
                ..state
            }),
        Expr::Capture { index, body } => {
            let start = state.position;
            let mut result = match_expr(body, input, state, flags)?;
            if let Some(capture) = result.captures.get_mut(*index) {
                if !flags.reverse || capture.is_none() {
                    *capture = Some(start..result.position);
                }
            }
            Some(result)
        }
        Expr::Repeat {
            body,
            min,
            max,
            greedy,
        } => repeat_options(body, input, state, flags, *min, *max, *greedy)
            .into_iter()
            .next(),
        Expr::Assertion(assertion) => {
            assertion_matches(*assertion, input, state.position, flags).then_some(state)
        }
        Expr::Lookaround { kind, body } => lookaround_match(*kind, body, input, state, flags),
        Expr::Backreference(reference) => {
            let index = match reference {
                Backreference::Index(index) => index.saturating_sub(1),
                Backreference::Name(_) => return None,
            };
            let range = state.captures.get(index).and_then(Option::as_ref)?;
            let width = range.end.saturating_sub(range.start);
            let matches = input
                .get(state.position..state.position + width)
                .is_some_and(|actual| {
                    input[range.clone()]
                        .iter()
                        .zip(actual)
                        .all(|(expected, actual)| {
                            equal(expected.value, actual.value, flags.ignore_case)
                        })
                });
            matches.then_some(State {
                position: state.position + width,
                ..state
            })
        }
        Expr::Mode { body, flags: local } => {
            match_expr(body, input, state, merge_flags(flags, *local))
        }
    }
}

fn match_sequence(parts: &[Expr], input: &[Unit], state: State, flags: Flags) -> Option<State> {
    sequence_options(parts, input, state, flags)
        .into_iter()
        .next()
}

fn sequence_options(parts: &[Expr], input: &[Unit], state: State, flags: Flags) -> Vec<State> {
    fn visit(
        parts: &[Expr],
        index: usize,
        input: &[Unit],
        state: State,
        flags: Flags,
        output: &mut Vec<State>,
    ) {
        if output.len() >= MAX_BACKTRACK_STATES {
            return;
        }
        let Some(part) = parts.get(index) else {
            output.push(state);
            return;
        };
        for candidate in match_options(part, input, state.clone(), flags) {
            visit(parts, index + 1, input, candidate, flags, output);
            if output.len() >= MAX_BACKTRACK_STATES {
                return;
            }
        }
    }
    let mut output = Vec::new();
    visit(parts, 0, input, state, flags, &mut output);
    output
}

fn match_options(expr: &Expr, input: &[Unit], state: State, flags: Flags) -> Vec<State> {
    match expr {
        Expr::Sequence(parts) => sequence_options(parts, input, state, flags),
        Expr::Repeat {
            body,
            min,
            max,
            greedy,
        } => repeat_options(body, input, state, flags, *min, *max, *greedy),
        Expr::Alternation(alternatives) => alternatives
            .iter()
            .filter_map(|alternative| match_expr(alternative, input, state.clone(), flags))
            .take(MAX_BACKTRACK_STATES)
            .collect(),
        Expr::Capture { index, body } => {
            let start = state.position;
            match_options(body, input, state, flags)
                .into_iter()
                .map(|mut result| {
                    if let Some(capture) = result.captures.get_mut(*index) {
                        if !flags.reverse || capture.is_none() {
                            *capture = Some(start..result.position);
                        }
                    }
                    result
                })
                .take(MAX_BACKTRACK_STATES)
                .collect()
        }
        _ => match_expr(expr, input, state, flags).into_iter().collect(),
    }
}

fn repeat_options(
    body: &Expr,
    input: &[Unit],
    state: State,
    flags: Flags,
    min: usize,
    max: Option<usize>,
    greedy: bool,
) -> Vec<State> {
    let limit = max.unwrap_or(input.len().saturating_add(1));
    fn visit(
        body: &Expr,
        input: &[Unit],
        state: State,
        flags: Flags,
        count: usize,
        min: usize,
        limit: usize,
        greedy: bool,
    ) -> Vec<State> {
        let mut result = Vec::new();
        let continuations = if count >= limit {
            Vec::new()
        } else {
            match_options(body, input, state.clone(), flags)
                .into_iter()
                .filter(|next| next.position != state.position || count < min)
                .flat_map(|next| visit(body, input, next, flags, count + 1, min, limit, greedy))
                .collect()
        };
        if greedy {
            result.extend(continuations);
            if count >= min {
                result.push(state);
            }
        } else {
            if count >= min {
                result.push(state.clone());
            }
            result.extend(continuations);
        }
        result
    }
    visit(body, input, state, flags, 0, min, limit, greedy)
        .into_iter()
        .take(MAX_BACKTRACK_STATES)
        .collect()
}

fn assertion_matches(assertion: Assertion, input: &[Unit], position: usize, flags: Flags) -> bool {
    match assertion {
        Assertion::Start => {
            position == 0
                || (flags.multiline
                    && position > 0
                    && is_line_terminator(input[position - 1].value))
        }
        Assertion::End => {
            position == input.len()
                || (flags.multiline
                    && input
                        .get(position)
                        .is_some_and(|unit| is_line_terminator(unit.value)))
        }
        Assertion::Boundary | Assertion::NonBoundary => {
            let left = position
                .checked_sub(1)
                .and_then(|index| input.get(index))
                .is_some_and(|unit| is_word_mode(unit.value, flags.ignore_case && flags.unicode));
            let right = input
                .get(position)
                .is_some_and(|unit| is_word_mode(unit.value, flags.ignore_case && flags.unicode));
            let boundary = left != right;
            matches!(assertion, Assertion::Boundary) == boundary
        }
    }
}

fn lookaround_match(
    kind: Lookaround,
    body: &Expr,
    input: &[Unit],
    state: State,
    flags: Flags,
) -> Option<State> {
    let behind = matches!(kind, Lookaround::Behind | Lookaround::NegativeBehind);
    let negative = matches!(kind, Lookaround::NegativeAhead | Lookaround::NegativeBehind);
    let found = if behind {
        (0..=state.position).find_map(|start| {
            let candidate = State {
                position: start,
                captures: state.captures.clone(),
            };
            match_expr(
                body,
                input,
                candidate,
                Flags {
                    reverse: true,
                    ..flags
                },
            )
            .filter(|result| result.position == state.position)
        })
    } else {
        match_expr(body, input, state.clone(), flags)
    };
    if negative {
        found.is_none().then_some(state)
    } else {
        found
    }
}

fn merge_flags(flags: Flags, local: ModeFlags) -> Flags {
    Flags {
        ignore_case: local.ignore_case.unwrap_or(flags.ignore_case),
        multiline: local.multiline.unwrap_or(flags.multiline),
        dot_all: local.dot_all.unwrap_or(flags.dot_all),
        ..flags
    }
}

fn class_matches(class: &ClassExpr, value: u32, ignore_case: bool) -> bool {
    let item_matches = |item: &ClassItem| match item {
        ClassItem::Character(character) => equal(*character, value, ignore_case),
        ClassItem::Range(min, max) => {
            let value = if ignore_case { lower(value) } else { value };
            let min = if ignore_case { lower(*min) } else { *min };
            let max = if ignore_case { lower(*max) } else { *max };
            min <= value && value <= max
        }
        ClassItem::Escape(escape) => escape_matches(*escape, value, ignore_case),
        ClassItem::Property {
            negative,
            name,
            value: property,
        } => property_matches(name, property.as_deref(), value) != *negative,
        ClassItem::Nested(nested) => class_matches(nested, value, ignore_case),
    };
    let matches = match class.kind {
        ClassKind::Union => class.items.iter().any(item_matches),
        ClassKind::Intersection => class.items.iter().all(item_matches),
        ClassKind::Subtraction => {
            class.items.first().is_some_and(item_matches)
                && !class.items.iter().skip(1).any(item_matches)
        }
    };
    if class.negative {
        !matches
    } else {
        matches
    }
}

fn escape_matches(escape: ast::CharacterClassEscapeKind, value: u32, ignore_case: bool) -> bool {
    match escape {
        ast::CharacterClassEscapeKind::D => value <= 0x7f && (value as u8).is_ascii_digit(),
        ast::CharacterClassEscapeKind::NegativeD => {
            !(value <= 0x7f && (value as u8).is_ascii_digit())
        }
        ast::CharacterClassEscapeKind::S => {
            char::from_u32(value).is_some_and(crate::regexp::is_ecma_whitespace)
        }
        ast::CharacterClassEscapeKind::NegativeS => {
            !char::from_u32(value).is_some_and(crate::regexp::is_ecma_whitespace)
        }
        ast::CharacterClassEscapeKind::W => is_word_mode(value, ignore_case),
        ast::CharacterClassEscapeKind::NegativeW => !is_word_mode(value, ignore_case),
    }
}

fn property_matches(name: &str, value: Option<&str>, character: u32) -> bool {
    let Some(character) = char::from_u32(character) else {
        return false;
    };
    match (name, value) {
        ("ASCII", _) => character.is_ascii(),
        ("Any", _) => true,
        ("Assigned", _) => {
            character.is_alphabetic()
                || character.is_numeric()
                || character.is_ascii_punctuation()
                || character.is_whitespace()
        }
        ("Letter", _) | ("Alphabetic", _) | ("L", _) => character.is_alphabetic(),
        ("Number", _) | ("N", _) | ("Decimal_Number", _) | ("Nd", _) => character.is_numeric(),
        ("Lowercase_Letter", _)
        | ("Ll", _)
        | ("General_Category", Some("Lowercase_Letter"))
        | ("General_Category", Some("Ll")) => character.is_lowercase(),
        ("Uppercase_Letter", _)
        | ("Lu", _)
        | ("General_Category", Some("Uppercase_Letter"))
        | ("General_Category", Some("Lu")) => character.is_uppercase(),
        ("General_Category", Some("Letter")) => character.is_alphabetic(),
        ("White_Space", _) | ("space", _) => character.is_whitespace(),
        ("ASCII_Hex_Digit", _) => character.is_ascii_hexdigit(),
        ("Script", Some("Latin")) | ("Script_Extensions", Some("Latin")) => {
            character.is_ascii_alphabetic()
        }
        _ => false,
    }
}

fn equal(left: u32, right: u32, ignore_case: bool) -> bool {
    if ignore_case {
        lower(left) == lower(right)
    } else {
        left == right
    }
}
fn lower(value: u32) -> u32 {
    char::from_u32(value)
        .and_then(|character| character.to_lowercase().next())
        .map_or(value, u32::from)
}
fn is_word(value: u32) -> bool {
    char::from_u32(value)
        .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
}
fn is_word_mode(value: u32, ignore_case: bool) -> bool {
    is_word(value) || (ignore_case && matches!(char::from_u32(value), Some('ſ' | 'K')))
}
fn is_line_terminator(value: u32) -> bool {
    matches!(value, 0x000A | 0x000D | 0x2028 | 0x2029)
}

#[cfg(test)]
mod tests {
    use super::{Flags, Regex};

    #[test]
    fn captures_partition_a_greedy_run() {
        let regex = Regex::with_flags("(b+)(b+)(b+)", Flags::default()).unwrap();
        let matched = regex.find_from("abbbbbbbc", 0).next().unwrap();
        assert_eq!(matched.range, 1..8);
        assert_eq!(matched.captures, vec![Some(1..6), Some(6..7), Some(7..8)]);
    }
}
