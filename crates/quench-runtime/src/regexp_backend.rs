//! Repository-owned ECMAScript RegExp parser, IR, and interpreter.
//!
//! OXC supplies syntax facts; the VM owns the executable representation. A
//! guarded compiled backend is retained for supported patterns, with the
//! repository-owned matcher as the complete semantic fallback.

use std::{cell::RefCell, collections::VecDeque, ops::Range};

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
    pub(crate) fn native(range: Range<usize>) -> Self {
        Self {
            range,
            captures: Vec::new(),
            named: Vec::new(),
        }
    }

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
    Named(Vec<usize>),
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
    String(Vec<u32>),
}

struct Lowering {
    names: Vec<String>,
    next_capture: usize,
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

// The semantic matcher must not silently discard alternatives.  Earlier code
// capped this vector at 4096 states, which could change a valid ECMAScript
// match.  The compiled linear backend handles ordinary patterns; unsupported
// constructs use this complete fallback and are allowed to retain every
// branch. Memory is finite for a finite input/pattern (though the fallback can
// still be exponentially large); it is no longer silently result-changing.
const MAX_BACKTRACK_STATES: usize = usize::MAX;

pub(crate) struct Regex {
    program: Expr,
    flags: Flags,
    capture_names: Vec<String>,
    has_named_groups: bool,
    /// The automata backend is a guarded fast path for ASCII patterns. Regex
    /// syntax it cannot represent (backreferences/lookaround and other
    /// ECMAScript-only constructs) continues through our complete matcher.
    compiled: Option<regex::bytes::Regex>,
    /// Reusable capture workspace for the compiled backend. Creating
    /// `CaptureLocations` for every `exec`/`replace` call allocates a fresh
    /// vector on the fixture's hot path; the regex object is already cached
    /// and execution is single-threaded, so this disposable physical state
    /// can live beside it without changing semantic match data.
    compiled_locations: RefCell<Option<regex::bytes::CaptureLocations>>,
}

impl Regex {
    pub(crate) fn with_flags(source: &str, flags: Flags) -> Result<Self, String> {
        let allocator = oxc::allocator::Allocator::default();
        crate::regexp::validate_pattern(source)?;
        let flags_text = flag_text(flags);
        let parsed = LiteralParser::new(&allocator, source, Some(&flags_text), Options::default())
            .parse()
            .map_err(|error| error.to_string())?;
        let mut names = Vec::new();
        collect_names(&parsed.body, &mut names);
        let mut lowering = Lowering {
            names,
            next_capture: 0,
        };
        let program = lower_disjunction(&parsed.body, &mut lowering);
        let has_named_groups = lowering.names.iter().any(|name| !name.is_empty());
        let compiled = (!flags.unicode_sets
            && !flags.unicode
            && std::env::var_os("QUENCH_DISABLE_COMPILED_REGEXP").is_none())
        .then(|| {
            let mut builder = regex::bytes::RegexBuilder::new(source);
            builder
                .case_insensitive(flags.ignore_case)
                .multi_line(flags.multiline)
                .dot_matches_new_line(flags.dot_all)
                .unicode(false);
            builder.build().ok()
        })
        .flatten();
        let compiled_locations = compiled
            .as_ref()
            .map(regex::bytes::Regex::capture_locations);
        Ok(Self {
            program,
            flags,
            capture_names: lowering.names,
            has_named_groups,
            compiled,
            compiled_locations: RefCell::new(compiled_locations),
        })
    }

    #[inline]
    fn names_for_match(&self) -> Vec<String> {
        if self.has_named_groups {
            self.capture_names.clone()
        } else {
            Vec::new()
        }
    }

    pub(crate) fn find_from(&self, text: &str, start: usize) -> Matches {
        if text.is_ascii() {
            if let Some(compiled) = &self.compiled {
                // Most replace/test patterns have no captures. Use the
                // allocation-free search API for those, avoiding both a
                // capture workspace probe and construction of an empty range
                // vector on every match.
                if compiled.captures_len() == 1 {
                    let item = compiled
                        .find_at(text.as_bytes(), start)
                        .map(|matched| Match::native(matched.range()));
                    return Matches { item };
                }
                let mut workspace = self.compiled_locations.borrow_mut();
                let locations = workspace
                    .as_mut()
                    .expect("compiled regexp has capture workspace");
                let item = compiled
                    .captures_read_at(locations, text.as_bytes(), start)
                    .map(|matched| Match {
                        range: matched.range(),
                        captures: (1..locations.len())
                            .map(|index| locations.get(index).map(|(start, end)| start..end))
                            .collect(),
                        named: self.names_for_match(),
                    });
                return Matches { item };
            }
        }
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
        let units = units_from_utf16(input, self.flags.unicode || self.flags.unicode_sets);
        let first = units
            .iter()
            .position(|unit| unit.offset_start >= start)
            .unwrap_or(units.len());
        Matches {
            item: self.find_units(&units, first),
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
                    |unit| {
                        if found.position > 0 {
                            input[found.position - 1].offset_end
                        } else {
                            unit.offset_start
                        }
                    },
                );
                let captures = found
                    .captures
                    .iter_mut()
                    .map(|capture| {
                        capture.take().map(|range| {
                            let capture_start = input.get(range.start).map_or_else(
                                || input.last().map_or(end, |unit| unit.offset_end),
                                |unit| unit.offset_start,
                            );
                            let capture_end = if range.end == 0 {
                                capture_start
                            } else {
                                input.get(range.end - 1).map_or_else(
                                    || input.last().map_or(capture_start, |unit| unit.offset_end),
                                    |unit| unit.offset_end,
                                )
                            };
                            capture_start..capture_end
                        })
                    })
                    .collect();
                return Some(Match {
                    range: start..end,
                    captures,
                    named: self.names_for_match(),
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

fn units_from_utf16(input: &[u16], unicode: bool) -> Vec<Unit> {
    let mut units = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        let value = input[index];
        if unicode
            && (0xD800..=0xDBFF).contains(&value)
            && input
                .get(index + 1)
                .is_some_and(|next| (0xDC00..=0xDFFF).contains(next))
        {
            let low = input[index + 1];
            units.push(Unit {
                value: 0x1_0000 + ((u32::from(value) - 0xD800) << 10) + u32::from(low) - 0xDC00,
                offset_start: index,
                offset_end: index + 2,
            });
            index += 2;
        } else {
            units.push(Unit {
                value: u32::from(value),
                offset_start: index,
                offset_end: index + 1,
            });
            index += 1;
        }
    }
    units
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
            let index = lowering.next_capture;
            lowering.next_capture += 1;
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
            let indices = lowering
                .names
                .iter()
                .enumerate()
                .filter_map(|(index, name)| (name == reference.name.as_str()).then_some(index + 1))
                .collect();
            Expr::Backreference(Backreference::Named(indices))
        }
    }
}

fn collect_names(disjunction: &ast::Disjunction<'_>, names: &mut Vec<String>) {
    for alternative in &disjunction.body {
        for term in &alternative.body {
            match term {
                ast::Term::CapturingGroup(group) => {
                    names.push(
                        group
                            .name
                            .as_ref()
                            .map_or_else(String::new, |name| name.to_string()),
                    );
                    collect_names(&group.body, names);
                }
                ast::Term::LookAroundAssertion(assertion) => collect_names(&assertion.body, names),
                ast::Term::Quantifier(quantifier) => collect_term_names(&quantifier.body, names),
                ast::Term::IgnoreGroup(group) => collect_names(&group.body, names),
                _ => {}
            }
        }
    }
}

fn collect_term_names(term: &ast::Term<'_>, names: &mut Vec<String>) {
    match term {
        ast::Term::CapturingGroup(group) => {
            names.push(
                group
                    .name
                    .as_ref()
                    .map_or_else(String::new, |name| name.to_string()),
            );
            collect_names(&group.body, names);
        }
        ast::Term::IgnoreGroup(group) => collect_names(&group.body, names),
        ast::Term::LookAroundAssertion(assertion) => collect_names(&assertion.body, names),
        ast::Term::Quantifier(quantifier) => collect_term_names(&quantifier.body, names),
        _ => {}
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
            ast::CharacterClassContents::ClassStringDisjunction(strings) => {
                vec![ClassItem::Nested(ClassExpr {
                    negative: false,
                    kind: ClassKind::Union,
                    items: strings
                        .body
                        .iter()
                        .map(|string| {
                            ClassItem::String(
                                string
                                    .body
                                    .iter()
                                    .map(|character| character.value)
                                    .collect(),
                            )
                        })
                        .collect(),
                })]
            }
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
            .filter(|unit| equal(*value, unit.value, flags.ignore_case, flags.unicode))
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
        Expr::Class(class) => class_match_widths(
            class,
            input,
            state.position,
            flags.ignore_case,
            flags.unicode,
        )
        .into_iter()
        .next()
        .map(|width| State {
            position: state.position + width,
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
            let range = match reference {
                Backreference::Index(index) => state
                    .captures
                    .get(index.saturating_sub(1))
                    .and_then(Option::as_ref),
                Backreference::Named(indices) => indices.iter().find_map(|index| {
                    state
                        .captures
                        .get(index.saturating_sub(1))
                        .and_then(Option::as_ref)
                }),
            };
            let Some(range) = range else {
                if flags.reverse {
                    return None;
                }
                return Some(state);
            };
            let width = range.end.saturating_sub(range.start);
            let matches = input
                .get(state.position..state.position + width)
                .is_some_and(|actual| {
                    input[range.clone()]
                        .iter()
                        .zip(actual)
                        .all(|(expected, actual)| {
                            equal(
                                expected.value,
                                actual.value,
                                flags.ignore_case,
                                flags.unicode,
                            )
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
            .flat_map(|alternative| match_options(alternative, input, state.clone(), flags))
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
        Expr::Mode { body, flags: local } => {
            match_options(body, input, state, merge_flags(flags, *local))
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
    if matches!(body, Expr::Literal(_) | Expr::Dot | Expr::Class(_)) {
        return repeat_simple_options(body, input, state, flags, min, max, greedy);
    }
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
        output: &mut Vec<State>,
    ) {
        if output.len() >= MAX_BACKTRACK_STATES {
            return;
        }
        if !greedy && count >= min {
            output.push(state.clone());
            if output.len() >= MAX_BACKTRACK_STATES {
                return;
            }
        }
        if count < limit {
            let mut iteration_state = state.clone();
            if !flags.reverse {
                for index in capture_indices(body) {
                    if let Some(capture) = iteration_state.captures.get_mut(index) {
                        *capture = None;
                    }
                }
            }
            for next in match_options(body, input, iteration_state, flags) {
                if next.position != state.position || count < min {
                    visit(
                        body,
                        input,
                        next,
                        flags,
                        count + 1,
                        min,
                        limit,
                        greedy,
                        output,
                    );
                    if output.len() >= MAX_BACKTRACK_STATES {
                        return;
                    }
                }
            }
        }
        if greedy && count >= min && output.len() < MAX_BACKTRACK_STATES {
            output.push(state);
        }
    }
    let mut output = Vec::new();
    visit(
        body,
        input,
        state,
        flags,
        0,
        min,
        limit,
        greedy,
        &mut output,
    );
    output
}

fn repeat_simple_options(
    body: &Expr,
    input: &[Unit],
    state: State,
    flags: Flags,
    min: usize,
    max: Option<usize>,
    greedy: bool,
) -> Vec<State> {
    let limit = max.unwrap_or(input.len().saturating_add(1));
    let mut states = VecDeque::new();
    let mut current = state.clone();
    let mut count = 0;
    while count < limit {
        let Some(next) = match_expr(body, input, current.clone(), flags) else {
            break;
        };
        if next.position == current.position {
            break;
        }
        count += 1;
        current = next;
        if count >= min {
            if states.len() == MAX_BACKTRACK_STATES {
                states.pop_front();
            }
            states.push_back(current.clone());
        }
    }
    if !greedy && min == 0 {
        states.push_front(state.clone());
        states.truncate(MAX_BACKTRACK_STATES);
    }
    let mut result: Vec<State> = states.into_iter().collect();
    if greedy {
        result.reverse();
        if min == 0 {
            result.push(state);
        }
    }
    result
}

fn capture_indices(expr: &Expr) -> Vec<usize> {
    let mut indices = Vec::new();
    fn visit(expr: &Expr, indices: &mut Vec<usize>) {
        match expr {
            Expr::Capture { index, body } => {
                indices.push(*index);
                visit(body, indices);
            }
            Expr::Sequence(parts) | Expr::Alternation(parts) => {
                parts.iter().for_each(|part| visit(part, indices))
            }
            Expr::Repeat { body, .. } | Expr::Lookaround { body, .. } | Expr::Mode { body, .. } => {
                visit(body, indices)
            }
            _ => {}
        }
    }
    visit(expr, &mut indices);
    indices.sort_unstable();
    indices.dedup();
    indices
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
        reverse_options(body, input, state.clone(), flags)
            .into_iter()
            .next()
    } else {
        match_expr(body, input, state.clone(), flags)
    };
    if negative {
        found.is_none().then_some(state)
    } else {
        found.map(|found| State {
            position: state.position,
            captures: found.captures,
        })
    }
}

fn reverse_options(expr: &Expr, input: &[Unit], state: State, flags: Flags) -> Vec<State> {
    match expr {
        Expr::Sequence(parts) => reverse_sequence_options(parts, input, state, flags),
        Expr::Alternation(alternatives) => alternatives
            .iter()
            .flat_map(|alternative| reverse_options(alternative, input, state.clone(), flags))
            .take(MAX_BACKTRACK_STATES)
            .collect(),
        Expr::Literal(value) => {
            let Some(previous) = state
                .position
                .checked_sub(1)
                .and_then(|index| input.get(index))
            else {
                return Vec::new();
            };
            equal(*value, previous.value, flags.ignore_case, flags.unicode)
                .then_some(State {
                    position: state.position - 1,
                    ..state
                })
                .into_iter()
                .collect()
        }
        Expr::Dot => {
            let Some(previous) = state
                .position
                .checked_sub(1)
                .and_then(|index| input.get(index))
            else {
                return Vec::new();
            };
            (flags.dot_all || !is_line_terminator(previous.value))
                .then_some(State {
                    position: state.position - 1,
                    ..state
                })
                .into_iter()
                .collect()
        }
        Expr::Class(class) => {
            reverse_class_options(class, input, state, flags.ignore_case, flags.unicode)
        }
        Expr::Capture { index, body } => {
            let end = state.position;
            reverse_options(body, input, state, flags)
                .into_iter()
                .map(|mut result| {
                    if let Some(capture) = result.captures.get_mut(*index) {
                        *capture = Some(result.position..end);
                    }
                    result
                })
                .take(MAX_BACKTRACK_STATES)
                .collect()
        }
        Expr::Repeat {
            body,
            min,
            max,
            greedy,
        } => reverse_repeat_options(body, input, state, flags, *min, *max, *greedy),
        Expr::Assertion(assertion) => assertion_matches(*assertion, input, state.position, flags)
            .then_some(state)
            .into_iter()
            .collect(),
        Expr::Lookaround { kind, body } => lookaround_match(*kind, body, input, state, flags)
            .into_iter()
            .collect(),
        Expr::Backreference(reference) => {
            let range = match reference {
                Backreference::Index(index) => state
                    .captures
                    .get(index.saturating_sub(1))
                    .and_then(Option::as_ref),
                Backreference::Named(indices) => indices.iter().find_map(|index| {
                    state
                        .captures
                        .get(index.saturating_sub(1))
                        .and_then(Option::as_ref)
                }),
            };
            let Some(range) = range else {
                return vec![state];
            };
            let width = range.end.saturating_sub(range.start);
            if width > state.position {
                return Vec::new();
            }
            let expected = &input[range.clone()];
            let actual = &input[state.position - width..state.position];
            expected
                .iter()
                .zip(actual)
                .all(|(left, right)| {
                    equal(left.value, right.value, flags.ignore_case, flags.unicode)
                })
                .then_some(State {
                    position: state.position - width,
                    ..state
                })
                .into_iter()
                .collect()
        }
        Expr::Mode { body, flags: local } => {
            reverse_options(body, input, state, merge_flags(flags, *local))
        }
    }
}

fn reverse_sequence_options(
    parts: &[Expr],
    input: &[Unit],
    state: State,
    flags: Flags,
) -> Vec<State> {
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
        if index == 0 {
            output.push(state);
            return;
        }
        for candidate in reverse_options(&parts[index - 1], input, state.clone(), flags) {
            visit(parts, index - 1, input, candidate, flags, output);
        }
    }
    let mut output = Vec::new();
    visit(parts, parts.len(), input, state, flags, &mut output);
    output
}

fn reverse_class_options(
    class: &ClassExpr,
    input: &[Unit],
    state: State,
    ignore_case: bool,
    unicode: bool,
) -> Vec<State> {
    let mut output = Vec::new();
    for width in 1..=state.position.min(32) {
        let start = state.position - width;
        if class_match_widths(class, input, start, ignore_case, unicode).contains(&width) {
            output.push(State {
                position: start,
                ..state.clone()
            });
        }
    }
    output.reverse();
    output
}

fn reverse_repeat_options(
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
        output: &mut Vec<State>,
    ) {
        if output.len() >= MAX_BACKTRACK_STATES {
            return;
        }
        if !greedy && count >= min {
            output.push(state.clone());
            if output.len() >= MAX_BACKTRACK_STATES {
                return;
            }
        }
        if count < limit {
            for next in reverse_options(body, input, state.clone(), flags) {
                if next.position != state.position || count < min {
                    visit(
                        body,
                        input,
                        next,
                        flags,
                        count + 1,
                        min,
                        limit,
                        greedy,
                        output,
                    );
                    if output.len() >= MAX_BACKTRACK_STATES {
                        return;
                    }
                }
            }
        }
        if greedy && count >= min && output.len() < MAX_BACKTRACK_STATES {
            output.push(state);
        }
    }
    let mut output = Vec::new();
    visit(
        body,
        input,
        state,
        flags,
        0,
        min,
        limit,
        greedy,
        &mut output,
    );
    output
}

fn merge_flags(flags: Flags, local: ModeFlags) -> Flags {
    Flags {
        ignore_case: local.ignore_case.unwrap_or(flags.ignore_case),
        multiline: local.multiline.unwrap_or(flags.multiline),
        dot_all: local.dot_all.unwrap_or(flags.dot_all),
        ..flags
    }
}

fn class_item_matches(item: &ClassItem, value: u32, ignore_case: bool, unicode: bool) -> bool {
    match item {
        ClassItem::Character(character) => equal(*character, value, ignore_case, unicode),
        ClassItem::Range(min, max) => {
            let value = if ignore_case {
                canonical(value, unicode)
            } else {
                value
            };
            let min = if ignore_case {
                canonical(*min, unicode)
            } else {
                *min
            };
            let max = if ignore_case {
                canonical(*max, unicode)
            } else {
                *max
            };
            min <= value && value <= max
        }
        ClassItem::Escape(escape) => escape_matches(*escape, value, ignore_case, unicode),
        ClassItem::Property {
            negative,
            name,
            value: property,
        } => {
            if ignore_case
                && *negative
                && (matches!(
                    name.as_str(),
                    "Lu" | "Ll" | "Uppercase_Letter" | "Lowercase_Letter"
                ) || matches!(
                    property.as_deref(),
                    Some("Lu" | "Ll" | "Uppercase_Letter" | "Lowercase_Letter")
                ))
            {
                return true;
            }
            let matches = property_matches(name, property.as_deref(), value)
                || (ignore_case
                    && char::from_u32(value).is_some_and(|character| {
                        character
                            .to_uppercase()
                            .chain(character.to_lowercase())
                            .map(u32::from)
                            .any(|variant| property_matches(name, property.as_deref(), variant))
                    }));
            matches != *negative
        }
        ClassItem::Nested(nested) => class_matches(nested, value, ignore_case, unicode),
        ClassItem::String(_) => false,
    }
}

fn class_item_widths(
    item: &ClassItem,
    input: &[Unit],
    position: usize,
    ignore_case: bool,
    unicode: bool,
) -> Vec<usize> {
    match item {
        ClassItem::String(expected) => input
            .get(position..position.saturating_add(expected.len()))
            .filter(|actual| {
                actual.len() == expected.len()
                    && actual.iter().zip(expected).all(|(actual, expected)| {
                        equal(*expected, actual.value, ignore_case, unicode)
                    })
            })
            .map_or_else(Vec::new, |_| vec![expected.len()]),
        ClassItem::Property { name, .. } if name == "Emoji_Keycap_Sequence" => {
            let mut sequences = vec![[b'#' as u32, 0xFE0F, 0x20E3], [b'*' as u32, 0xFE0F, 0x20E3]];
            sequences.extend((b'0'..=b'9').map(|digit| [u32::from(digit), 0xFE0F, 0x20E3]));
            sequences
                .into_iter()
                .filter(|expected| {
                    input
                        .get(position..position + expected.len())
                        .is_some_and(|actual| {
                            actual
                                .iter()
                                .zip(expected)
                                .all(|(actual, expected)| actual.value == *expected)
                        })
                })
                .map(|expected| expected.len())
                .collect()
        }
        ClassItem::Property { name, .. } if is_string_property(name) => {
            string_property_widths(name, input, position)
        }
        ClassItem::Nested(nested) => {
            class_match_widths(nested, input, position, ignore_case, unicode)
        }
        _ => input
            .get(position)
            .filter(|unit| class_item_matches(item, unit.value, ignore_case, unicode))
            .map_or_else(Vec::new, |_| vec![1]),
    }
}

fn is_string_property(name: &str) -> bool {
    matches!(
        name,
        "Basic_Emoji"
            | "RGI_Emoji"
            | "RGI_Emoji_Flag_Sequence"
            | "RGI_Emoji_Modifier_Sequence"
            | "RGI_Emoji_Tag_Sequence"
            | "RGI_Emoji_ZWJ_Sequence"
    )
}

fn string_property_widths(name: &str, input: &[Unit], position: usize) -> Vec<usize> {
    let mut widths = Vec::new();
    let limit = match name {
        "Basic_Emoji" => 2,
        "RGI_Emoji_Flag_Sequence" | "RGI_Emoji_Modifier_Sequence" => 3,
        "RGI_Emoji_Tag_Sequence" => 8,
        _ => 16,
    };
    for width in 1..=input.len().saturating_sub(position).min(limit) {
        if string_property_matches_units(name, &input[position..position + width]) {
            widths.push(width);
        }
    }
    widths
}

fn string_property_matches_units(name: &str, units: &[Unit]) -> bool {
    match name {
        "Basic_Emoji" => is_basic_emoji_units(units),
        "RGI_Emoji_Flag_Sequence" => is_flag_units(units),
        "RGI_Emoji_Modifier_Sequence" => is_modifier_units(units),
        "RGI_Emoji_Tag_Sequence" => is_tag_units(units),
        "RGI_Emoji_ZWJ_Sequence" => is_zwj_units(units),
        "RGI_Emoji" => {
            is_basic_emoji_units(units)
                || is_keycap_units(units)
                || is_flag_units(units)
                || is_modifier_units(units)
                || is_tag_units(units)
                || is_zwj_units(units)
        }
        _ => false,
    }
}

fn is_keycap_units(units: &[Unit]) -> bool {
    matches!(
        units,
        [a, b, c] if (a.value == 35 || a.value == 42 || (0x30..=0x39).contains(&a.value))
            && b.value == 0xFE0F && c.value == 0x20E3
    )
}

fn is_flag_units(units: &[Unit]) -> bool {
    units.len() == 2
        && units
            .iter()
            .all(|unit| (0x1F1E6..=0x1F1FF).contains(&unit.value))
}

fn is_modifier_units(units: &[Unit]) -> bool {
    if !units
        .last()
        .is_some_and(|unit| (0x1F3FB..=0x1F3FF).contains(&unit.value))
    {
        return false;
    }
    let mut base = &units[..units.len().saturating_sub(1)];
    if base.last().is_some_and(|unit| unit.value == 0xFE0F) {
        base = &base[..base.len() - 1];
    }
    base.len() == 1 && is_basic_emoji_code_point(base[0].value)
}

fn is_tag_units(units: &[Unit]) -> bool {
    units.len() >= 3
        && units.first().is_some_and(|unit| unit.value == 0x1F3F4)
        && units.last().is_some_and(|unit| unit.value == 0xE007F)
        && units[1..units.len() - 1]
            .iter()
            .all(|unit| (0xE0020..=0xE007E).contains(&unit.value))
}

fn is_zwj_units(units: &[Unit]) -> bool {
    units.iter().any(|unit| unit.value == 0x200D)
        && units.split(|unit| unit.value == 0x200D).all(|part| {
            !part.is_empty()
                && part.iter().all(|unit| {
                    unit.value == 0xFE0F
                        || (0x1F3FB..=0x1F3FF).contains(&unit.value)
                        || is_basic_emoji_code_point(unit.value)
                })
        })
}

fn is_basic_emoji_units(units: &[Unit]) -> bool {
    match units {
        [unit] => is_basic_emoji_code_point(unit.value),
        [base, variation] => variation.value == 0xFE0F && is_basic_emoji_code_point(base.value),
        _ => false,
    }
}

fn is_basic_emoji_code_point(value: u32) -> bool {
    property_matches("Emoji", None, value)
}

fn class_match_widths(
    class: &ClassExpr,
    input: &[Unit],
    position: usize,
    ignore_case: bool,
    unicode: bool,
) -> Vec<usize> {
    let mut widths: Vec<usize> = match class.kind {
        ClassKind::Union => class
            .items
            .iter()
            .flat_map(|item| class_item_widths(item, input, position, ignore_case, unicode))
            .collect(),
        ClassKind::Intersection => {
            let candidate = class.items.first().map_or_else(Vec::new, |item| {
                class_item_widths(item, input, position, ignore_case, unicode)
            });
            candidate
                .into_iter()
                .filter(|width| {
                    class.items.iter().skip(1).all(|item| {
                        class_item_widths(item, input, position, ignore_case, unicode)
                            .contains(width)
                    })
                })
                .collect()
        }
        ClassKind::Subtraction => {
            let candidate = class.items.first().map_or_else(Vec::new, |item| {
                class_item_widths(item, input, position, ignore_case, unicode)
            });
            candidate
                .into_iter()
                .filter(|width| {
                    !class.items.iter().skip(1).any(|item| {
                        class_item_widths(item, input, position, ignore_case, unicode)
                            .contains(width)
                    })
                })
                .collect()
        }
    };
    widths.sort_unstable_by(|left, right| right.cmp(left));
    widths.dedup();
    if class.negative {
        if widths.is_empty() && input.get(position).is_some() {
            widths.push(1);
        } else {
            widths.clear();
        }
    }
    widths
}

fn class_matches(class: &ClassExpr, value: u32, ignore_case: bool, unicode: bool) -> bool {
    let item_matches = |item: &ClassItem| class_item_matches(item, value, ignore_case, unicode);
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

fn escape_matches(
    escape: ast::CharacterClassEscapeKind,
    value: u32,
    ignore_case: bool,
    unicode: bool,
) -> bool {
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
        ast::CharacterClassEscapeKind::W => is_word_mode(value, ignore_case && unicode),
        ast::CharacterClassEscapeKind::NegativeW => !is_word_mode(value, ignore_case && unicode),
    }
}

pub(crate) fn property_matches(name: &str, value: Option<&str>, character: u32) -> bool {
    let Some(character) = char::from_u32(character) else {
        return false;
    };
    if let Some(result) = icu_property_matches(name, value, character) {
        return result;
    }
    match (name, value) {
        ("ASCII", _) => character.is_ascii(),
        ("Any", _) => true,
        ("Assigned", _) => {
            character.is_alphabetic()
                || character.is_numeric()
                || character.is_ascii_punctuation()
                || character.is_whitespace()
        }
        ("Letter", _) | ("Alphabetic", _) | ("Alpha", _) | ("L", _) => character.is_alphabetic(),
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
        ("ASCII_Hex_Digit", _) | ("AHex", _) | ("Hex_Digit", _) => character.is_ascii_hexdigit(),
        ("Script", Some("Latin")) | ("Script_Extensions", Some("Latin")) => {
            character.is_ascii_alphabetic()
        }
        ("Script", Some("Han")) | ("Script_Extensions", Some("Han")) => {
            matches!(character as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0x20000..=0x2FA1F)
        }
        _ => false,
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PropertyMatcher {
    kind: PropertyMatcherKind,
}

#[derive(Clone, Copy)]
enum PropertyMatcherKind {
    Any,
    Assigned,
    Script(icu_properties::props::Script),
    ScriptExtensions(icu_properties::props::Script),
    GeneralCategory(icu_properties::props::GeneralCategory),
    GeneralCategoryGroup(icu_properties::props::GeneralCategoryGroup),
    Binary(fn(char) -> bool),
}

impl PropertyMatcher {
    pub(crate) fn matches(self, character: char) -> bool {
        use icu_properties::{props, CodePointMapData};
        match self.kind {
            PropertyMatcherKind::Any => true,
            PropertyMatcherKind::Assigned => {
                CodePointMapData::<props::GeneralCategory>::new().get(character)
                    != props::GeneralCategory::Unassigned
            }
            PropertyMatcherKind::Script(target) => {
                CodePointMapData::<props::Script>::new().get(character) == target
            }
            PropertyMatcherKind::ScriptExtensions(target) => {
                icu_properties::script::ScriptWithExtensions::new().has_script(character, target)
            }
            PropertyMatcherKind::GeneralCategory(target) => {
                CodePointMapData::<props::GeneralCategory>::new().get(character) == target
            }
            PropertyMatcherKind::GeneralCategoryGroup(target) => {
                target.contains(CodePointMapData::<props::GeneralCategory>::new().get(character))
            }
            PropertyMatcherKind::Binary(matcher) => matcher(character),
        }
    }
}

fn binary_property_matches<P: icu_properties::props::BinaryProperty>(character: char) -> bool {
    icu_properties::CodePointSetData::new::<P>().contains(character)
}

pub(crate) fn compile_property_matcher(name: &str, value: Option<&str>) -> Option<PropertyMatcher> {
    use icu_properties::{props, PropertyParser};
    let kind = if name == "Any" {
        PropertyMatcherKind::Any
    } else if name == "Assigned" {
        PropertyMatcherKind::Assigned
    } else if matches!(name, "Script" | "sc") {
        PropertyMatcherKind::Script(PropertyParser::<props::Script>::new().get_loose(value?)?)
    } else if matches!(name, "Script_Extensions" | "scx") {
        PropertyMatcherKind::ScriptExtensions(
            PropertyParser::<props::Script>::new().get_loose(value?)?,
        )
    } else if matches!(name, "General_Category" | "gc") {
        if let Some(target) = PropertyParser::<props::GeneralCategory>::new().get_loose(value?) {
            PropertyMatcherKind::GeneralCategory(target)
        } else {
            PropertyMatcherKind::GeneralCategoryGroup(
                PropertyParser::<props::GeneralCategoryGroup>::new().get_loose(value?)?,
            )
        }
    } else {
        macro_rules! binary_property {
            ($( $($property:literal)|+ => $kind:ident),* $(,)?) => {
                match name {
                    $( $( $property => PropertyMatcherKind::Binary(binary_property_matches::<props::$kind>), )+ )*
                    _ => return None,
                }
            };
        }
        binary_property!(
        "ASCII_Hex_Digit" => AsciiHexDigit,
        "AHex" => AsciiHexDigit,
        "Alphabetic" => Alphabetic,
        "Alpha" => Alphabetic,
        "Bidi_Control" => BidiControl,
        "Bidi_C" => BidiControl,
        "Bidi_Mirrored" => BidiMirrored,
        "Bidi_M" => BidiMirrored,
        "Case_Ignorable" => CaseIgnorable,
        "CI" => CaseIgnorable,
        "Cased" => Cased,
        "Changes_When_Casefolded" | "CWCF" => ChangesWhenCasefolded,
        "Changes_When_Casemapped" | "CWCM" => ChangesWhenCasemapped,
        "Changes_When_Lowercased" | "CWL" => ChangesWhenLowercased,
        "Changes_When_NFKC_Casefolded" | "CWKCF" => ChangesWhenNfkcCasefolded,
        "Changes_When_Titlecased" | "CWT" => ChangesWhenTitlecased,
        "Changes_When_Uppercased" | "CWU" => ChangesWhenUppercased,
        "Dash" => Dash,
        "Deprecated" | "Dep" => Deprecated,
        "Default_Ignorable_Code_Point" | "DI" => DefaultIgnorableCodePoint,
        "Diacritic" | "Dia" => Diacritic,
        "Emoji" => Emoji,
        "Emoji_Component" | "EComp" => EmojiComponent,
        "Emoji_Modifier" | "EMod" => EmojiModifier,
        "Emoji_Modifier_Base" | "EBase" => EmojiModifierBase,
        "Emoji_Presentation" | "EPres" => EmojiPresentation,
        "Extended_Pictographic" | "ExtPict" => ExtendedPictographic,
        "Extender" | "Ext" => Extender,
        "Grapheme_Base" | "Gr_Base" => GraphemeBase,
        "Grapheme_Extend" | "Gr_Ext" => GraphemeExtend,
        "Hex_Digit" | "Hex" => HexDigit,
        "ID_Continue" | "IDC" => IdContinue,
        "ID_Start" | "IDS" => IdStart,
        "Ideographic" | "Ideo" => Ideographic,
        "IDS_Binary_Operator" | "IDSB" => IdsBinaryOperator,
        "IDS_Trinary_Operator" | "IDST" => IdsTrinaryOperator,
        "Join_Control" | "Join_C" => JoinControl,
        "Logical_Order_Exception" | "LOE" => LogicalOrderException,
        "Lowercase" | "Lower" => Lowercase,
        "Math" => Math,
        "Noncharacter_Code_Point" | "NChar" => NoncharacterCodePoint,
        "Pattern_Syntax" | "Pat_Syn" => PatternSyntax,
        "Pattern_White_Space" | "Pat_WS" => PatternWhiteSpace,
        "Quotation_Mark" | "QMark" => QuotationMark,
        "Radical" => Radical,
        "Regional_Indicator" | "RI" => RegionalIndicator,
        "Sentence_Terminal" | "STerm" => SentenceTerminal,
        "Soft_Dotted" | "SD" => SoftDotted,
        "Terminal_Punctuation" | "Term" => TerminalPunctuation,
        "Unified_Ideograph" | "UIdeo" => UnifiedIdeograph,
        "Uppercase" | "Upper" => Uppercase,
        "Variation_Selector" | "VS" => VariationSelector,
        "White_Space" | "space" | "WSpace" => WhiteSpace,
        "XID_Continue" | "XIDC" => XidContinue,
        "XID_Start" | "XIDS" => XidStart,
        )
    };
    Some(PropertyMatcher { kind })
}

fn icu_property_matches(name: &str, value: Option<&str>, character: char) -> Option<bool> {
    Some(compile_property_matcher(name, value)?.matches(character))
}

fn equal(left: u32, right: u32, ignore_case: bool, unicode: bool) -> bool {
    if ignore_case {
        canonical(left, unicode) == canonical(right, unicode)
    } else {
        left == right
    }
}
fn canonical(value: u32, unicode: bool) -> u32 {
    if unicode {
        lower(value)
    } else if (u32::from(b'A')..=u32::from(b'Z')).contains(&value) {
        value + 0x20
    } else {
        value
    }
}
fn lower(value: u32) -> u32 {
    char::from_u32(value)
        .map(|character| icu_casemap::CaseMapper::new().simple_fold(character))
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

    #[test]
    fn automata_backend_preserves_ascii_capture_ranges() {
        let regex = Regex::with_flags("(ab+)", Flags::default()).unwrap();
        let matched = regex.find_from("zzabbb", 0).next().unwrap();
        assert_eq!(matched.range, 2..6);
        assert_eq!(matched.captures, vec![Some(2..6)]);
    }

    #[test]
    fn unicode_string_class_matches() {
        for (pattern, input) in [("[[0-9]&&\\q{0|2|4|9️⃣}]", "0"), ("[\\q{0|2|4|9️⃣}]", "9️⃣")]
        {
            let regex = Regex::with_flags(pattern, Flags::from("v")).unwrap();
            assert!(
                regex.find_from(input, 0).next().is_some(),
                "{pattern} {input}"
            );
        }
    }
    #[test]
    fn lookbehind_sticky_prefix() {
        let regex = Regex::with_flags("(?<=^(\\w+))def", Flags::from("g")).unwrap();
        let input = "abcdefdef".encode_utf16().collect::<Vec<_>>();
        assert!(regex.find_from_utf16(&input, 0).next().is_some());
    }
    #[test]
    fn lookahead_capture_preserves_extent() {
        let regex = Regex::with_flags("(?:(?=(abc)))a", Flags::default()).unwrap();
        let matched = regex.find_from("abc", 0).next().unwrap();
        assert_eq!(matched.range, 0..1);
        assert_eq!(matched.captures, vec![Some(0..3)]);
    }
    #[test]
    fn duplicate_group_properties_pattern_matches() {
        let regex = Regex::with_flags(
            "(?:(?<x>a)|(?<y>a)(?<x>b))(?:(?<z>c)|(?<z>d))",
            Flags::default(),
        )
        .unwrap();
        assert!(regex.find_from("abc", 0).next().is_some());
    }
}
