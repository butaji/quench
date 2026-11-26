//! Repository-owned ECMAScript RegExp parser, IR, and interpreter.
//!
//! OXC supplies syntax facts; the VM owns the executable representation. A
//! guarded compiled backend is retained for supported patterns, with the
//! repository-owned matcher as the complete semantic fallback.

use std::{
    cell::OnceCell,
    ops::Range,
};

use oxc::regular_expression::{ast, LiteralParser, Options};

pub mod backtrack;
mod ascii_search;
mod emoji_zwj;
mod range_search;
pub use range_search::{LiteralMatches, RangeSearcher};
mod stack;

pub fn is_stack_exhaustion_message(message: &str) -> bool {
    message == quench_stack::STACK_EXHAUSTED_MESSAGE
}

pub fn is_ecma_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

pub fn validate_flags(flags: &str) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for flag in flags.chars() {
        if !matches!(flag, 'd' | 'g' | 'i' | 'm' | 's' | 'u' | 'v' | 'y') || !seen.insert(flag) {
            return Err("invalid regular expression flags".into());
        }
    }
    if seen.contains(&'u') && seen.contains(&'v') {
        return Err("invalid regular expression flags".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Default)]
pub struct Flags {
    pub ignore_case: bool,
    pub multiline: bool,
    pub dot_all: bool,
    pub unicode: bool,
    pub unicode_sets: bool,
}

impl From<&str> for Flags {
    fn from(flags: &str) -> Self {
        Self {
            ignore_case: flags.contains('i'),
            multiline: flags.contains('m'),
            dot_all: flags.contains('s'),
            unicode: flags.contains('u'),
            unicode_sets: flags.contains('v'),
        }
    }
}

impl Flags {
    /// The flags as one byte, which an ahead search's key holds (`backtrack::native::AheadKey`).
    pub fn bits(self) -> u8 {
        [self.ignore_case, self.multiline, self.dot_all, self.unicode, self.unicode_sets].iter().enumerate()
            .fold(0, |bits, (bit, set)| bits | u8::from(*set) << bit)
    }
}

#[derive(Clone)]
pub struct Match {
    pub range: Range<usize>,
    pub captures: Vec<Option<Range<usize>>>,
    named: Vec<String>,
}

impl Match {
    pub fn native(range: Range<usize>) -> Self {
        Self {
            range,
            captures: Vec::new(),
            named: Vec::new(),
        }
    }

    pub fn start(&self) -> usize {
        self.range.start
    }
    pub fn end(&self) -> usize {
        self.range.end
    }
    pub fn groups(&self) -> impl Iterator<Item = Option<Range<usize>>> + '_ {
        std::iter::once(Some(self.range.clone())).chain(self.captures.iter().cloned())
    }
    /// Drops this match, handing its captures vector to the next match found on this thread.
    pub fn recycle(self) {
        backtrack::recycle_captures(self.captures);
    }

    pub fn named_groups(&self) -> impl Iterator<Item = (&str, Option<Range<usize>>)> + '_ {
        self.named
            .iter()
            .enumerate()
            .filter(|(_, name)| !name.is_empty())
            .map(|(index, name)| (name.as_str(), self.captures.get(index).cloned().flatten()))
    }
}

pub struct Matches {
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
        /// `capture_indices(body)`: the captures each iteration resets.
        captures: Box<[usize]>,
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

pub use backtrack::Assertion;
// The projection seal, which the rows' stages name as `crate::projection` in every build that mounts them.
use quench_stack::projection;
use backtrack::{FOLDED_WORD_UNITS, LINE_TERMINATORS, WORD_UNITS};
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
        /// The ICU matcher `name`/`value` resolve to, looked up once at lowering.
        matcher: Option<PropertyMatcher>,
    },
    Nested(ClassExpr),
    String(Vec<u32>),
}

impl ClassItem {
    fn property(property: &ast::UnicodePropertyEscape) -> Self {
        let name = property.name.to_string();
        let value = property.value.as_ref().map(ToString::to_string);
        Self::Property {
            negative: property.negative,
            matcher: compile_property_matcher(&name, value.as_deref()),
            name,
            value,
        }
    }
}

struct Lowering<'a> {
    source: &'a str,
    names: Vec<String>,
    next_capture: usize,
}
#[derive(Clone, Copy)]
struct Unit {
    value: u32,
    offset_start: usize,
    offset_end: usize,
}
mod canonical;
use canonical::{ASCII_MAX, canonical};

pub struct Regex {
    /// The complete matcher's program, lowered when it first runs. A pattern the byte
    /// backend serves matches ASCII subjects without it, so it may never be needed.
    complete: OnceCell<CompleteMatcher>,
    /// The normalized source `complete` and its tree lower from.
    source: Box<str>,
    flags: Flags,
    capture_names: Vec<String>,
    has_named_groups: bool,
}

/// The repository matcher's form of a pattern.
struct CompleteMatcher {
    /// Literal units every match consumes contiguously (`required_literal`); empty if none.
    required_literal: Box<[u32]>,
    /// The most code points a match consumes before `required_literal`'s first unit (`None`
    /// where unbounded). The leftmost match holding a hit of the literal starts no earlier than
    /// the hit less this (`Regex::ascii_begin`).
    literal_reach: Option<usize>,
    /// The micro-VM's program (`backtrack::Program`): every search of the pattern runs it.
    backtracking: backtrack::Program,
    /// `backtracking` without its captures, for a search that reads only the match's bounds
    /// (`Regex::find_range_ascii`): `None` where the pattern has no captures, or has a
    /// backreference, whose match reads them.
    bounds: Option<backtrack::Program>,
    /// `required_literal` as bytes, when every unit is ASCII.
    required_bytes: Option<LiteralFinder>,
    /// `required_literal` as the native-endian bytes of its code units, which a UTF-16
    /// subject's search looks for (`contains_code_unit_bytes`); `None` when it is empty or a
    /// unit lies past UTF-16, where it occurs in no code-unit text. Built by the first such
    /// search (`CompleteMatcher::required_wide`): most programs search only ASCII subjects,
    /// and a finder is several hundred bytes (110 of V8-v7 RegExp's 237 patterns had one).
    required_wide: OnceCell<Option<LiteralFinder>>,
    /// The finder of a pattern that is its literal alone (`range_search::literal_run`).
    literal_run: range_search::LiteralRun,
    /// The fewest code points any match consumes (`min_length`). A code point is at least
    /// one unit of any subject, so a search with fewer units left has no match.
    min_length: usize,
    /// Whether some path through the pattern matches the empty string unconditionally: with
    /// no assertion, lookaround or backreference on it. A search from any position then
    /// succeeds at that position (`Regex::matches_at_every_start`).
    empty_everywhere: bool,
}

/// Whether `expr` holds a backreference, whose match reads the captures.
fn has_backreference(expr: &Expr) -> bool {
    match expr {
        Expr::Literal(_) | Expr::Dot | Expr::Class(_) | Expr::Assertion(_) => false,
        Expr::Sequence(parts) | Expr::Alternation(parts) => parts.iter().any(has_backreference),
        Expr::Capture { body, .. } | Expr::Mode { body, .. } | Expr::Repeat { body, .. } | Expr::Lookaround { body, .. } => has_backreference(body),
        Expr::Backreference(_) => true,
    }
}

/// `expr` with every capture group a plain group: the same matches, with no capture to
/// record or reset, for a pattern with no backreference.
fn without_captures(expr: &Expr) -> Expr {
    match expr {
        Expr::Sequence(parts) => Expr::Sequence(parts.iter().map(without_captures).collect()),
        Expr::Alternation(parts) => Expr::Alternation(parts.iter().map(without_captures).collect()),
        Expr::Capture { body, .. } => without_captures(body),
        Expr::Repeat { body, min, max, greedy, .. } => Expr::Repeat { body: Box::new(without_captures(body)), min: *min, max: *max, greedy: *greedy, captures: Box::new([]) },
        Expr::Lookaround { kind, body } => Expr::Lookaround { kind: *kind, body: Box::new(without_captures(body)) },
        Expr::Mode { body, flags } => Expr::Mode { body: Box::new(without_captures(body)), flags: *flags },
        other => other.clone(),
    }
}

/// Whether some path through `expr` matches empty whatever the subject and position: as
/// `nullable`, but an assertion, lookaround or backreference may fail, so it never counts.
fn empty_everywhere(expr: &Expr) -> bool {
    match expr {
        Expr::Literal(_) | Expr::Dot | Expr::Class(_) => false,
        Expr::Sequence(parts) => parts.iter().all(empty_everywhere),
        Expr::Alternation(alternatives) => alternatives.iter().any(empty_everywhere),
        Expr::Capture { body, .. } | Expr::Mode { body, .. } => empty_everywhere(body),
        Expr::Repeat { body, min, .. } => *min == 0 || empty_everywhere(body),
        Expr::Assertion(_) | Expr::Lookaround { .. } | Expr::Backreference(_) => false,
    }
}

/// A searcher for a literal byte run. The searcher is several hundred bytes, so it is boxed:
/// most patterns have no such literal, and theirs need not carry the space.
pub(crate) type LiteralFinder = Box<memchr::memmem::Finder<'static>>;

pub(crate) fn literal_finder(bytes: &[u8]) -> LiteralFinder {
    Box::new(memchr::memmem::Finder::new(bytes).into_owned())
}

impl CompleteMatcher {
    /// Whether the required-literal prefilter proves a search finds nothing: the search is not
    /// sticky (a sticky search tests one position, where the literal check costs more than the
    /// match), the pattern has a required literal, and `contains` (the subject's own form of the
    /// containment test) says the searched rest lacks it. The one prefilter rule of every subject form.
    fn prefilter_rejects(&self, sticky: bool, contains: impl FnOnce(&Self) -> bool) -> bool {
        !sticky && !self.required_literal.is_empty() && !contains(self)
    }

    /// `required_wide`, built on first use.
    fn required_wide(&self) -> Option<&LiteralFinder> {
        self.required_wide
            .get_or_init(|| code_unit_bytes(&self.required_literal).map(|bytes| literal_finder(&bytes)))
            .as_ref()
    }

    /// Lowers a parsed pattern whose capture names are `names`.
    fn lower(pattern: &ast::Pattern<'_>, source: &str, names: &[String], flags: Flags) -> Self {
        let program = lower_tree(pattern, source, names);
        let backtracking = backtrack::Program::compile(&program, flags, names.len()).expect("every pattern compiles to the VM");
        let bounds = (!names.is_empty() && !has_backreference(&program))
            .then(|| backtrack::Program::compile(&without_captures(&program), flags, 0).expect("every pattern compiles to the VM"));
        let RequiredLiteral { units: required_literal, reach: literal_reach } = required_literal(&program, flags);
        // A program that starts every ASCII search at its prefix finds the same literal
        // there, so the run's own search answers a subject without it; an anchored one tries
        // a single position, which costs less than scanning the subject for the literal.
        let required_bytes = (!required_literal.is_empty()
            && required_literal.iter().all(|unit| *unit <= u32::from(ASCII_MAX)))
        .then(|| required_literal.iter().map(|unit| *unit as u8).collect::<Vec<_>>())
        .filter(|bytes| !(backtracking.searches_prefix(bytes) || backtracking.anchored_at_start()))
        .map(|bytes| literal_finder(&bytes));
        // An ahead image's searches of the pattern, where it holds them (`backtrack::native::register_ahead`).
        backtracking.adopt_ahead((source.into(), flags.bits(), false));
        if let Some(bounds) = &bounds { bounds.adopt_ahead((source.into(), flags.bits(), true)); }
        let min_length = min_length(&program);
        let empty_everywhere = empty_everywhere(&program);
        let literal_run = range_search::LiteralRun::of(&program, &required_literal);
        Self {
            required_literal,
            literal_reach,
            backtracking,
            bounds,
            required_bytes,
            required_wide: OnceCell::new(),
            literal_run,
            min_length,
            empty_everywhere,
        }
    }
}

/// Lowers a parsed pattern whose capture names are `names` to the expression the VM compiles.
fn lower_tree(pattern: &ast::Pattern<'_>, source: &str, names: &[String]) -> Expr {
    let mut lowering = Lowering {
        source,
        names: names.to_vec(),
        next_capture: 0,
    };
    lower_disjunction(&pattern.body, &mut lowering)
}

impl std::fmt::Debug for Regex {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Regex")
            .field("capture_names", &self.capture_names)
            .field("has_named_groups", &self.has_named_groups)
            .finish_non_exhaustive()
    }
}

impl Regex {
    pub fn capture_count(&self) -> usize {
        self.capture_names.len()
    }

    pub fn with_flags(source: &str, flags: Flags) -> Result<Self, String> {
        let allocator = oxc::allocator::Allocator::default();
        let flags_text = flag_text(flags);
        let control_source = normalize_control_letter_escapes(source);
        let backslash_quantifier_source = normalize_escaped_backslash_quantifiers(&control_source);
        let normalized_source = normalize_new_unicode_scripts(&backslash_quantifier_source);
        let parsed = LiteralParser::new(
            &allocator,
            &normalized_source,
            Some(&flags_text),
            Options::default(),
        )
        .parse()
        .map_err(|error| error.to_string())?;
        stack::validate(&parsed)?;
        if contains_invalid_string_property(&parsed.body, flags) {
            return Err("invalid regular expression property of strings".into());
        }
        let mut names = Vec::new();
        collect_names(&parsed.body, &mut names);
        let has_named_groups = names.iter().any(|name| !name.is_empty());
        // The `regex` crate's automata were a second backend here; since the compiled
        // backtracking matcher (`backtrack::Program`) they cost more than they saved (V8-v7
        // RegExp, static x86_64, 2026-10-18: max RSS 9544 -> 8660 KiB and Score 869 -> 971
        // without them; up to 5 MiB more on the npm top 300), so they were removed.
        let complete = OnceCell::new();
        let _ = complete.set(CompleteMatcher::lower(&parsed, &normalized_source, &names, flags));
        Ok(Self {
            complete,
            source: normalized_source.as_ref().into(),
            flags,
            capture_names: names,
            has_named_groups,
        })
    }

    /// The native plans of the pattern's programs for an ahead image, by their key
    /// (`backtrack::native::AheadKey`): the search program's, and the capture-free bounds program's
    /// where it has one; a program whose code would name a live address has none (`Program::ahead_plan`).
    pub fn ahead_plans(&self) -> Vec<(backtrack::native::AheadKey, backtrack::native::NativePlan)> {
        let complete = self.complete();
        let key = |bounds| (self.source.clone(), self.flags.bits(), bounds);
        std::iter::once((false, &complete.backtracking)).chain(complete.bounds.as_ref().map(|bounds| (true, bounds)))
            .filter_map(|(bounds, program)| program.ahead_plan().map(|plan| (key(bounds), plan))).collect()
    }

    /// The repository matcher, lowered from the kept source on first use.
    /// Whether a search from `start` of a subject of `length` code units whose unit at
    /// `start` is `first()` (`None` past its end) finds no match, decided before the subject
    /// is prepared: too few units remain for the shortest match, or the pattern's start
    /// anchor rules the position out (`backtrack::Program::anchor_excludes`), or a `sticky`
    /// search's one position admits no match start (`backtrack::Program::start_excludes`).
    /// Code units are pattern units only outside Unicode mode, so the anchor applies only
    /// there. The
    /// complete matcher's facts are lowered once per pattern on first use, even for a
    /// pattern the automata backend matches.
    pub fn excludes_search(&self, length: usize, start: usize, sticky: bool, first: impl FnOnce() -> Option<u16>) -> bool {
        let complete = self.complete();
        if length.saturating_sub(start) < complete.min_length {
            return true;
        }
        let program = &complete.backtracking;
        let first = first().map(u32::from);
        (!(self.flags.unicode || self.flags.unicode_sets) && program.anchor_excludes(start, first))
            || (sticky && program.start_excludes(start, length, first))
    }

    /// Whether every search of a subject, ASCII (`ascii`) or not, reads its text as it is,
    /// never decoding it into the units a subject keeps once decoded (`Subject::decoded`):
    /// the backtracking program runs over ASCII bytes, and outside Unicode mode over code
    /// units too. A caller can then prepare such a subject for one search alone.
    pub fn searches_undecoded(&self, ascii: bool) -> bool {
        ascii || !(self.flags.unicode || self.flags.unicode_sets)
    }

    fn complete(&self) -> &CompleteMatcher {
        self.complete.get_or_init(|| {
            self.with_parsed_source(|parsed| {
                CompleteMatcher::lower(parsed, &self.source, &self.capture_names, self.flags)
            })
        })
    }

    fn with_parsed_source<R>(&self, lower: impl FnOnce(&ast::Pattern<'_>) -> R) -> R {
        let allocator = oxc::allocator::Allocator::default();
        let flags_text = flag_text(self.flags);
        let parsed = LiteralParser::new(&allocator, &self.source, Some(&flags_text), Options::default())
            .parse()
            .expect("a constructed pattern parses again");
        lower(&parsed)
    }

    #[inline]
    fn names_for_match(&self) -> Vec<String> {
        if self.has_named_groups {
            self.capture_names.clone()
        } else {
            Vec::new()
        }
    }

    pub fn find_from(&self, text: &str, start: usize) -> Matches {
        if start > text.len() {
            return Matches { item: None };
        }
        let units = units_from_str(text, self.flags.unicode || self.flags.unicode_sets);
        Matches { item: self.find_units(&units, first_unit_at(&units, start), false) }
    }

    /// Matches against a subject whose ASCII bytes were prepared once, so repeated matches
    /// over the same text do not rescan and recopy it.
    pub fn find_in_subject(&self, subject: &Subject, start: usize) -> Matches {
        Matches { item: self.subject_find(subject, start, false, true) }
    }

    /// The match that starts exactly at `start`, as a sticky (`y`) search requires, or in
    /// Unicode mode at the lead of the surrogate pair `start` lies inside
    /// (`first_unit_at`). Only that position is tried, so a failing sticky match costs no
    /// scan of the rest of the text.
    pub fn find_sticky_in_subject(&self, subject: &Subject, start: usize) -> Matches {
        Matches { item: self.subject_find(subject, start, true, true) }
    }

    /// The range of the next match in `subject` (only at `start` when `sticky`), for callers
    /// that observe no captures: the VM collects none.
    pub fn find_range_in_subject(&self, subject: &Subject, start: usize, sticky: bool) -> Option<Range<usize>> {
        self.subject_find(subject, start, sticky, false).map(|matched| matched.range)
    }

    /// The match the VM finds in `subject` from `start` (only at `start` when `sticky`),
    /// with its captures where `captures` asks for them.
    fn subject_find(&self, subject: &Subject, start: usize, sticky: bool, captures: bool) -> Option<Match> {
        if start > subject.len() {
            return None;
        }
        if let Some(bytes) = subject.ascii() {
            return self.find_ascii(bytes, start, sticky, captures).flatten();
        }
        let unicode = self.flags.unicode || self.flags.unicode_sets;
        let complete = self.complete();
        if subject.len().saturating_sub(start) < complete.min_length {
            return None;
        }
        if let (false, Some(units)) = (unicode, subject.wide()) {
            // Outside unicode mode a code unit is a pattern unit: no decoding, and
            // positions are offsets.
            if complete.prefilter_rejects(sticky, |complete| {
                complete.required_wide().is_some_and(|literal| contains_code_unit_bytes(&units[start..], literal.as_ref()))
            }) {
                return None;
            }
            return complete.backtracking.find(units, start, sticky, captures).map(|found| self.offset_match(found, captures));
        }
        let units = subject.decoded(unicode);
        self.find_units(units, first_unit_at(units, start), sticky)
    }

    /// The match `found` over a subject whose positions are its offsets, with its group
    /// names where its `captures` were collected.
    fn offset_match(&self, found: backtrack::Found, captures: bool) -> Match {
        let named = if captures { self.names_for_match() } else { Vec::new() };
        Match { range: found.start..found.end, captures: found.captures, named }
    }

    pub fn find_from_utf16(&self, input: &[u16], start: usize) -> Matches {
        if start > input.len() {
            return Matches { item: None };
        }
        let units = units_from_utf16(input, self.flags.unicode || self.flags.unicode_sets);
        Matches { item: self.find_units(&units, first_unit_at(&units, start), false) }
    }

    pub fn find_range_from_utf16(&self, input: &[u16], start: usize) -> Option<Range<usize>> {
        self.find_from_utf16(input, start).next().map(|matched| matched.range)
    }

    /// The match `golden_check` compares with the recorded answers.
    #[cfg(test)]
    fn golden_find(&self, input: &[Unit], first: usize) -> Option<Match> {
        self.find_units(input, first, false)
    }

    /// The VM's match over decoded units from unit `first` (only there when `sticky`), its
    /// positions and captures in UTF-16 offsets.
    fn find_units(&self, input: &[Unit], first: usize, sticky: bool) -> Option<Match> {
        let complete = self.complete();
        if complete.prefilter_rejects(sticky, |complete| contains_literal(&input[first.min(input.len())..], &complete.required_literal)) {
            return None;
        }
        let found = complete.backtracking.find(input, first, sticky, true)?;
        Some(self.unit_match(input, found.start, found.end, found.captures))
    }

    /// The match from unit `position` to unit `end` of `input`, with captures in units, in
    /// UTF-16 offsets.
    fn unit_match(
        &self,
        input: &[Unit],
        position: usize,
        end: usize,
        captures: Vec<Option<Range<usize>>>,
    ) -> Match {
        let start = input.get(position).map_or_else(
            || input.last().map_or(0, |unit| unit.offset_end),
            |unit| unit.offset_start,
        );
        let end = input.get(end).map_or_else(
            || input.last().map_or(start, |unit| unit.offset_end),
            |unit| {
                if end > 0 {
                    input[end - 1].offset_end
                } else {
                    unit.offset_start
                }
            },
        );
        let captures = captures
            .into_iter()
            .map(|capture| {
                capture.map(|range| {
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
        Match {
            range: start..end,
            captures,
            named: self.names_for_match(),
        }
    }
}

/// The most ASCII bytes a subject holds inline (`Subject::short_ascii`): the `Wide`
/// variant's four words, less the length byte.
pub const INLINE_SUBJECT_BYTES: usize = 4 * size_of::<usize>() - 1;

/// The text a subject matches: a string's own one-byte ASCII storage, a short ASCII copy,
/// or its UTF-16 units.
enum SubjectText {
    /// The bytes `range` of shared ASCII storage: a whole string, or a slice of one.
    Ascii {
        text: std::rc::Rc<str>,
        range: std::ops::Range<usize>,
    },
    Inline {
        length: u8,
        bytes: [u8; INLINE_SUBJECT_BYTES],
    },
    Wide {
        units: std::rc::Rc<[u16]>,
        /// One byte per unit, present only when every unit is ASCII.
        ascii: Option<Box<[u8]>>,
    },
}

/// Text prepared once for repeated matching. The shared storage is retained, so a caller
/// can recognize the same string again by allocation identity.
pub struct Subject {
    text: SubjectText,
    /// The units decoded for the backtracking matcher, by code unit and by code point
    /// (`unicode`), each decoded on first use.
    decoded: [std::cell::OnceCell<Vec<Unit>>; 2],
}

impl Subject {
    pub fn new(units: std::rc::Rc<[u16]>) -> Self {
        // One branch-free pass classifies the text (vectorizable); a second narrows ASCII
        // text to bytes. Early-exit scans would stop the compiler from vectorizing either.
        let all_bits = units.iter().fold(0, |bits, unit| bits | *unit);
        let ascii = (all_bits <= ASCII_MAX).then(|| units.iter().map(|unit| *unit as u8).collect());
        Self {
            text: SubjectText::Wide { units, ascii },
            decoded: Default::default(),
        }
    }

    /// A subject over UTF-16 text known to hold a non-ASCII unit, as a string's wide
    /// storage always does: the ASCII narrowing `new` tries cannot pay off, so preparing the
    /// subject touches no unit.
    pub fn from_non_ascii(units: std::rc::Rc<[u16]>) -> Self {
        debug_assert!(units.iter().any(|unit| *unit > ASCII_MAX));
        Self {
            text: SubjectText::Wide { units, ascii: None },
            decoded: Default::default(),
        }
    }

    /// A subject over the bytes `range` of ASCII text, matched in place without a copy.
    pub fn from_ascii(text: std::rc::Rc<str>, range: std::ops::Range<usize>) -> Self {
        debug_assert!(text.is_ascii() && range.end <= text.len());
        Self {
            text: SubjectText::Ascii { text, range },
            decoded: Default::default(),
        }
    }

    /// A subject over ASCII text of at most `INLINE_SUBJECT_BYTES`, copied inline, so a
    /// short string's subject needs no storage of its own.
    pub fn short_ascii(text: &str) -> Self {
        debug_assert!(text.is_ascii());
        let mut bytes = [0; INLINE_SUBJECT_BYTES];
        bytes[..text.len()].copy_from_slice(text.as_bytes());
        Self {
            text: SubjectText::Inline {
                length: u8::try_from(text.len()).expect("short text fits its length byte"),
                bytes,
            },
            decoded: Default::default(),
        }
    }


    fn len(&self) -> usize {
        match &self.text {
            SubjectText::Ascii { range, .. } => range.len(),
            SubjectText::Inline { length, .. } => usize::from(*length),
            SubjectText::Wide { units, .. } => units.len(),
        }
    }

    fn ascii(&self) -> Option<&[u8]> {
        match &self.text {
            SubjectText::Ascii { text, range } => Some(&text.as_bytes()[range.clone()]),
            SubjectText::Inline { length, bytes } => Some(&bytes[..usize::from(*length)]),
            SubjectText::Wide { ascii, .. } => ascii.as_deref(),
        }
    }

    fn decoded(&self, unicode: bool) -> &[Unit] {
        self.decoded[usize::from(unicode)].get_or_init(|| match &self.text {
            SubjectText::Ascii { .. } | SubjectText::Inline { .. } => {
                let ascii = self.ascii().expect("ASCII text has bytes");
                let units = ascii.iter().copied().map(u16::from).collect::<Vec<_>>();
                units_from_utf16(&units, unicode)
            }
            SubjectText::Wide { units, .. } => units_from_utf16(units, unicode),
        })
    }

    /// The UTF-16 units of a subject that is not all ASCII.
    fn wide(&self) -> Option<&[u16]> {
        match &self.text {
            SubjectText::Wide { units, ascii: None } => Some(units),
            _ => None,
        }
    }

    pub fn is_text_of(&self, units: &std::rc::Rc<[u16]>) -> bool {
        matches!(&self.text, SubjectText::Wide { units: own, .. } if std::rc::Rc::ptr_eq(own, units))
    }

    /// Whether this subject is the bytes `range` of the same ASCII storage as `text`.
    pub fn is_ascii_text_of(&self, text: &std::rc::Rc<str>, range: &std::ops::Range<usize>) -> bool {
        matches!(&self.text, SubjectText::Ascii { text: own, range: own_range }
            if std::rc::Rc::ptr_eq(own, text) && own_range == range)
    }
}

pub fn validate_property_escapes(source: &str, flags_text: &str) -> Result<(), String> {
    let flags = Flags::from(flags_text);
    let allocator = oxc::allocator::Allocator::default();
    let source = normalize_new_unicode_scripts(source);
    let parsed = LiteralParser::new(&allocator, &source, Some(flags_text), Options::default())
        .parse()
        .map_err(|error| error.to_string())?;
    stack::validate(&parsed)?;
    if contains_invalid_string_property(&parsed.body, flags) {
        return Err("invalid regular expression property of strings".into());
    }
    Ok(())
}

struct UnicodeScriptData {
    aliases: [&'static str; 2],
    ranges: &'static [(u32, u32)],
}

const NEW_UNICODE_SCRIPTS: &[UnicodeScriptData] = &[
    UnicodeScriptData {
        aliases: ["Beria_Erfe", "Berf"],
        ranges: &[(0x16EA0, 0x16EB8), (0x16EBB, 0x16ED3)],
    },
    UnicodeScriptData {
        aliases: ["Sidetic", "Sidt"],
        ranges: &[(0x10940, 0x10959)],
    },
    UnicodeScriptData {
        aliases: ["Tai_Yo", "Tayo"],
        ranges: &[(0x1E6C0, 0x1E6DE), (0x1E6E0, 0x1E6F5), (0x1E6FE, 0x1E6FF)],
    },
    UnicodeScriptData {
        aliases: ["Tolong_Siki", "Tols"],
        ranges: &[(0x11DB0, 0x11DDB), (0x11DE0, 0x11DE9)],
    },
];

fn normalize_escaped_backslash_quantifiers(source: &str) -> std::borrow::Cow<'_, str> {
    let bytes = source.as_bytes();
    let mut normalized = String::with_capacity(source.len());
    let mut copied_until = 0;
    let mut index = 0;
    let mut in_class = false;
    while index < bytes.len() {
        if bytes[index] == b'[' && (index == 0 || bytes[index - 1] != b'\\') {
            in_class = true;
        } else if bytes[index] == b']' && in_class && (index == 0 || bytes[index - 1] != b'\\') {
            in_class = false;
        }
        if !in_class && bytes.get(index..index + 3) == Some(b"\\\\{") {
            if let Some(end) = braced_quantifier_end(bytes, index + 3) {
                normalized.push_str(&source[copied_until..index]);
                // Grouping that atom avoids an OXC parser bug without changing meaning.
                normalized.push_str(r"(?:\\)");
                normalized.push_str(&source[index + 2..end]);
                copied_until = end;
                index = end;
                continue;
            }
        }
        if bytes[index] == b'\\' {
            index = (index + 2).min(bytes.len());
        } else {
            index += 1;
        }
    }
    if copied_until == 0 {
        std::borrow::Cow::Borrowed(source)
    } else {
        normalized.push_str(&source[copied_until..]);
        std::borrow::Cow::Owned(normalized)
    }
}

fn braced_quantifier_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    let minimum_start = index;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    if index == minimum_start {
        return None;
    }
    if bytes.get(index) == Some(&b',') {
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
    }
    (bytes.get(index) == Some(&b'}')).then_some(index + 1)
}

fn normalize_control_letter_escapes(source: &str) -> std::borrow::Cow<'_, str> {
    if !source.contains("\\c") {
        return std::borrow::Cow::Borrowed(source);
    }
    use std::fmt::Write;
    let mut normalized = String::with_capacity(source.len());
    let mut characters = source.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\' {
            if let Some(escaped) = characters.next() {
                if escaped == 'c' && characters.peek().is_some_and(char::is_ascii_alphabetic) {
                    let letter = characters.next().unwrap();
                    let value = u32::from(letter) & CONTROL_CODE_MASK;
                    // The pinned OXC parser splits class control escapes;
                    // this equivalent spelling also preserves range endpoints.
                    write!(&mut normalized, "\\x{value:02X}").unwrap();
                } else {
                    normalized.push(character);
                    normalized.push(escaped);
                }
                continue;
            }
        }
        normalized.push(character);
    }
    std::borrow::Cow::Owned(normalized)
}

fn normalize_new_unicode_scripts(pattern: &str) -> std::borrow::Cow<'_, str> {
    if !pattern.contains("\\p{") && !pattern.contains("\\P{") {
        return std::borrow::Cow::Borrowed(pattern);
    }

    let mut normalized = None;
    for script in NEW_UNICODE_SCRIPTS {
        if !script.aliases.iter().any(|alias| pattern.contains(alias)) {
            continue;
        }
        let ranges = script
            .ranges
            .iter()
            .map(|(start, end)| {
                if start == end {
                    format!(r"\u{{{start:X}}}")
                } else {
                    format!(r"\u{{{start:X}}}-\u{{{end:X}}}")
                }
            })
            .collect::<String>();
        let positive_class = format!("[{ranges}]");
        let negative_class = format!("[^{ranges}]");
        for value in script.aliases {
            for property in ["Script", "sc", "Script_Extensions", "scx"] {
                for (escape, class) in [("p", &positive_class), ("P", &negative_class)] {
                    let needle = format!(r"\{escape}{{{property}={value}}}");
                    let current = normalized.as_deref().unwrap_or(pattern);
                    if current.contains(&needle) {
                        let current = normalized.get_or_insert_with(|| pattern.to_owned());
                        *current = current.replace(&needle, class);
                    }
                }
            }
        }
    }
    normalized.map_or_else(
        || std::borrow::Cow::Borrowed(pattern),
        std::borrow::Cow::Owned,
    )
}

fn contains_invalid_string_property(disjunction: &ast::Disjunction<'_>, flags: Flags) -> bool {
    disjunction.body.iter().any(|alternative| {
        alternative
            .body
            .iter()
            .any(|term| term_contains_invalid_string_property(term, flags))
    })
}

fn term_contains_invalid_string_property(term: &ast::Term<'_>, flags: Flags) -> bool {
    match term {
        ast::Term::UnicodePropertyEscape(property) => {
            let is_string = property.strings || is_string_property(&property.name);
            is_string
                && (flags.unicode && !flags.unicode_sets || flags.unicode_sets && property.negative)
        }
        ast::Term::CharacterClass(class) => class_contains_invalid_string_property(class, flags),
        ast::Term::CapturingGroup(group) => contains_invalid_string_property(&group.body, flags),
        ast::Term::IgnoreGroup(group) => contains_invalid_string_property(&group.body, flags),
        ast::Term::LookAroundAssertion(assertion) => {
            contains_invalid_string_property(&assertion.body, flags)
        }
        ast::Term::Quantifier(quantifier) => {
            term_contains_invalid_string_property(&quantifier.body, flags)
        }
        _ => false,
    }
}

fn class_contains_invalid_string_property(class: &ast::CharacterClass<'_>, flags: Flags) -> bool {
    class.body.iter().any(|item| match item {
        ast::CharacterClassContents::UnicodePropertyEscape(property) => {
            let is_string = property.strings || is_string_property(&property.name);
            is_string
                && (flags.unicode && !flags.unicode_sets || flags.unicode_sets && property.negative)
        }
        ast::CharacterClassContents::NestedCharacterClass(nested) => {
            class_contains_invalid_string_property(nested, flags)
        }
        _ => false,
    })
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

/// The longest run of literal units that every match consumes contiguously: consecutive
/// literals of the top-level sequence, read through captures, which a match always enters.
/// Empty for case-insensitive patterns, whose literals match more than their own value.
/// The fewest code points a match of `expr` consumes. Assertions, lookarounds and
/// backreferences may consume none; a class consumes one code point unless it has string
/// alternatives (`v`-mode `\q{…}`), which may be empty.
fn min_length(expr: &Expr) -> usize {
    match expr {
        Expr::Sequence(parts) => parts
            .iter()
            .fold(0, |total, part| total.saturating_add(min_length(part))),
        Expr::Alternation(branches) => branches.iter().map(min_length).min().unwrap_or(0),
        Expr::Literal(_) | Expr::Dot => 1,
        Expr::Class(class) => usize::from(!class_has_strings(class)),
        Expr::Capture { body, .. } | Expr::Mode { body, .. } => min_length(body),
        Expr::Repeat { body, min, .. } => min.saturating_mul(min_length(body)),
        Expr::Assertion(_) | Expr::Lookaround { .. } | Expr::Backreference(_) => 0,
    }
}

/// Whether a class has string alternatives (`v`-mode `\q{…}`), whose length varies.
fn class_has_strings(class: &ClassExpr) -> bool {
    class.items.iter().any(|item| match item {
        ClassItem::String(_) => true,
        ClassItem::Nested(nested) => class_has_strings(nested),
        _ => false,
    })
}

/// The most code points any match of `expr` consumes, `None` where that is unbounded: an
/// unbounded repeat of a consuming body, a backreference, or a class with string alternatives.
fn max_length(expr: &Expr) -> Option<usize> {
    match expr {
        Expr::Sequence(parts) => parts.iter().try_fold(0, |total: usize, part| total.checked_add(max_length(part)?)),
        Expr::Alternation(branches) => branches.iter().try_fold(0, |most: usize, branch| Some(most.max(max_length(branch)?))),
        Expr::Literal(_) | Expr::Dot => Some(1),
        Expr::Class(class) => (!class_has_strings(class)).then_some(1),
        Expr::Capture { body, .. } | Expr::Mode { body, .. } => max_length(body),
        Expr::Repeat { body, max, .. } => {
            let width = max_length(body)?;
            match max {
                Some(max) => width.checked_mul(*max),
                None => (width == 0).then_some(0),
            }
        }
        Expr::Assertion(_) | Expr::Lookaround { .. } => Some(0),
        Expr::Backreference(_) => None,
    }
}

/// The literal `required_literal` finds, and the most code points a match consumes before it.
#[derive(Default)]
struct RequiredLiteral {
    units: Box<[u32]>,
    reach: Option<usize>,
}

/// The walk of a pattern's top-level literal runs: the run being read, the best run so far,
/// and the code points consumed so far (`consumed`, `None` once unbounded), with the count
/// at each run's first unit (`run_reach`, `best_reach`).
#[derive(Default)]
struct LiteralWalk {
    run: Vec<u32>,
    run_reach: Option<usize>,
    best: Vec<u32>,
    best_reach: Option<usize>,
    consumed: Option<usize>,
}

impl LiteralWalk {
    /// Reads `expr`: a literal extends the run, and any other part ends it and consumes what
    /// it may match. Sequences and captures are read through, as a match always enters them.
    fn visit(&mut self, expr: &Expr) {
        match expr {
            Expr::Sequence(parts) => parts.iter().for_each(|part| self.visit(part)),
            Expr::Capture { body, .. } => self.visit(body),
            Expr::Literal(value) => {
                if self.run.is_empty() {
                    self.run_reach = self.consumed;
                }
                self.run.push(*value);
                self.consume(max_length(expr));
            }
            _ => {
                self.end_run();
                self.consume(max_length(expr));
            }
        }
    }

    /// Counts `width` more code points, `None` as unbounded.
    fn consume(&mut self, width: Option<usize>) {
        self.consumed = self.consumed.zip(width).and_then(|(total, width)| total.checked_add(width));
    }

    /// Ends the run being read, keeping it as the best run where it is longer than the best.
    fn end_run(&mut self) {
        if self.run.len() > self.best.len() {
            std::mem::swap(&mut self.run, &mut self.best);
            self.best_reach = self.run_reach;
        }
        self.run.clear();
    }
}

/// The longest run of literal units every match consumes contiguously, with the reach of the
/// literal. Empty for case-insensitive patterns, whose literals match more than their own value.
fn required_literal(program: &Expr, flags: Flags) -> RequiredLiteral {
    if flags.ignore_case {
        return RequiredLiteral::default();
    }
    let mut walk = LiteralWalk { consumed: Some(0), ..LiteralWalk::default() };
    walk.visit(program);
    walk.end_run();
    RequiredLiteral { units: walk.best.into(), reach: walk.best_reach }
}

/// The native-endian bytes of `literal`'s code units, which a search of a UTF-16 text's bytes
/// looks for; `None` when it is empty or a unit lies past UTF-16.
fn code_unit_bytes(literal: &[u32]) -> Option<Vec<u8>> {
    if literal.is_empty() {
        return None;
    }
    literal
        .iter()
        .map(|unit| u16::try_from(*unit).ok().map(u16::to_ne_bytes))
        .collect::<Option<Vec<_>>>()
        .map(|units| units.concat())
}

/// Whether the code units `needle` searches for (`code_unit_bytes`) occur in `input`: at an
/// occurrence in its bytes that starts on a unit boundary.
fn contains_code_unit_bytes(input: &[u16], needle: &memchr::memmem::Finder<'_>) -> bool {
    // SAFETY: the bytes of initialized `u16` values, which any `u8` view may read.
    let haystack = unsafe {
        std::slice::from_raw_parts(input.as_ptr().cast::<u8>(), std::mem::size_of_val(input))
    };
    let unit = std::mem::size_of::<u16>();
    needle.find_iter(haystack).any(|offset| offset % unit == 0)
}

/// Whether `literal` occurs in `input`.
fn contains_literal(input: &[Unit], literal: &[u32]) -> bool {
    input.windows(literal.len()).any(|window| {
        window
            .iter()
            .zip(literal)
            .all(|(unit, value)| unit.value == *value)
    })
}

/// The index of the decoded unit holding offset `start` (`units.len()` at the end): a
/// Unicode-mode start inside a surrogate pair is the pair's lead, as RegExpBuiltinExec
/// reads `lastIndex` as the character holding that element.
fn first_unit_at(units: &[Unit], start: usize) -> usize {
    units.partition_point(|unit| unit.offset_end <= start)
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
            ast::BoundaryAssertionKind::StartBuffer => Assertion::StartBuffer,
            ast::BoundaryAssertionKind::EndBuffer => Assertion::EndBuffer,
            ast::BoundaryAssertionKind::EndBufferOptionalNewline => {
                Assertion::EndBufferOptionalNewline
            }
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
        ast::Term::Quantifier(quantifier) => {
            let body = lower_term(&quantifier.body, lowering);
            Expr::Repeat {
                captures: capture_indices(&body).into(),
                body: Box::new(body),
                min: quantifier.min.min(usize::MAX as u64) as usize,
                max: quantifier
                    .max
                    .map(|max| max.min(usize::MAX as u64) as usize),
                greedy: quantifier.greedy,
            }
        }
        ast::Term::Character(character) => lower_character(character, lowering.source),
        ast::Term::Dot(_) => Expr::Dot,
        ast::Term::CharacterClassEscape(escape) => Expr::Class(ClassExpr {
            negative: false,
            kind: ClassKind::Union,
            items: vec![ClassItem::Escape(escape.kind)],
        }),
        ast::Term::UnicodePropertyEscape(property) => Expr::Class(ClassExpr {
            negative: false,
            kind: ClassKind::Union,
            items: vec![ClassItem::property(property)],
        }),
        ast::Term::CharacterClass(class) => Expr::Class(lower_class(class, lowering.source)),
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
                .map_or_else(ModeFlags::default, |modifiers| ModeFlags {
                    ignore_case: modifier_mode(
                        modifiers.enabling,
                        modifiers.disabling,
                        ast::Modifier::I,
                    ),
                    multiline: modifier_mode(
                        modifiers.enabling,
                        modifiers.disabling,
                        ast::Modifier::M,
                    ),
                    dot_all: modifier_mode(
                        modifiers.enabling,
                        modifiers.disabling,
                        ast::Modifier::S,
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
                .filter_map(|(index, name)| {
                    decode_identifier_escapes(reference.name.as_str())
                        .is_some_and(|decoded| name == &decoded)
                        .then_some(index + 1)
                })
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
                            .and_then(|name| decode_identifier_escapes(name.as_str()))
                            .unwrap_or_default(),
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
                    .and_then(|name| decode_identifier_escapes(name.as_str()))
                    .unwrap_or_default(),
            );
            collect_names(&group.body, names);
        }
        ast::Term::IgnoreGroup(group) => collect_names(&group.body, names),
        ast::Term::LookAroundAssertion(assertion) => collect_names(&assertion.body, names),
        ast::Term::Quantifier(quantifier) => collect_term_names(&quantifier.body, names),
        _ => {}
    }
}

/// A group name with its `\uXXXX` and `\u{...}` escapes decoded; `None` when an escape is malformed.
pub fn decode_identifier_escapes(name: &str) -> Option<String> {
    let mut units = Vec::new();
    let mut chars = name.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            let mut encoded = [0; 2];
            units.extend(character.encode_utf16(&mut encoded).iter().copied());
            continue;
        }
        if chars.next()? != 'u' {
            return None;
        }
        let first = chars.next()?;
        let value = if first == '{' {
            let mut digits = String::new();
            loop {
                let digit = chars.next()?;
                if digit == '}' {
                    break;
                }
                if !digit.is_ascii_hexdigit() {
                    return None;
                }
                digits.push(digit);
            }
            u32::from_str_radix(&digits, 16).ok()?
        } else {
            let mut digits = String::from(first);
            digits.extend(chars.by_ref().take(3));
            if digits.len() != 4 || !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
                return None;
            }
            u32::from_str_radix(&digits, 16).ok()?
        };
        if value > u32::from(u16::MAX) {
            let mut encoded = [0; 2];
            units.extend(
                char::from_u32(value)?
                    .encode_utf16(&mut encoded)
                    .iter()
                    .copied(),
            );
        } else {
            units.push(u16::try_from(value).ok()?);
        }
    }
    String::from_utf16(&units).ok()
}

fn modifier_mode(
    enabling: ast::Modifier,
    disabling: ast::Modifier,
    flag: ast::Modifier,
) -> Option<bool> {
    if enabling.contains(flag) {
        Some(true)
    } else if disabling.contains(flag) {
        Some(false)
    } else {
        None
    }
}

const CONTROL_CODE_MASK: u32 = 0x1F;
const CONTROL_ESCAPE_PARTS: usize = 3;

fn lower_character(character: &ast::Character, source: &str) -> Expr {
    let legacy_octal_tail = (character.kind == ast::CharacterKind::Octal3)
        .then(|| source.get(character.span.start as usize..character.span.end as usize))
        .flatten()
        .and_then(|escape| {
            let mut digits = escape.chars().skip(1);
            let first = digits.next()?;
            let second = digits.next()?;
            let third = digits.next()?;
            (matches!(first, '4'..='7')
                && second.is_ascii_digit()
                && second <= '7'
                && third.is_ascii_digit()
                && third <= '7')
                .then_some(u32::from(third))
        });
    if let Some(tail) = legacy_octal_tail {
        Expr::Sequence(vec![Expr::Literal(character.value), Expr::Literal(tail)])
    } else {
        Expr::Literal(character.value)
    }
}

fn lower_class(class: &ast::CharacterClass<'_>, source: &str) -> ClassExpr {
    let mut items = Vec::new();
    let mut index = 0;
    while let Some(item) = class.body.get(index) {
        if let Some(control) = legacy_class_control(&class.body[index..], source) {
            items.push(ClassItem::Character(control));
            index += CONTROL_ESCAPE_PARTS;
            continue;
        }
        items.extend(match item {
            ast::CharacterClassContents::CharacterClassRange(range) => {
                vec![ClassItem::Range(range.min.value, range.max.value)]
            }
            ast::CharacterClassContents::CharacterClassEscape(escape) => {
                vec![ClassItem::Escape(escape.kind)]
            }
            ast::CharacterClassContents::UnicodePropertyEscape(property) => {
                vec![ClassItem::property(property)]
            }
            ast::CharacterClassContents::Character(character) => {
                let value = if character.kind == ast::CharacterKind::ControlLetter {
                    character.value & CONTROL_CODE_MASK
                } else {
                    character.value
                };
                vec![ClassItem::Character(value)]
            }
            ast::CharacterClassContents::NestedCharacterClass(nested) => {
                vec![ClassItem::Nested(lower_class(nested, source))]
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
        });
        index += 1;
    }
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

fn legacy_class_control(contents: &[ast::CharacterClassContents<'_>], source: &str) -> Option<u32> {
    let [ast::CharacterClassContents::Character(backslash), ast::CharacterClassContents::Character(control), ast::CharacterClassContents::Character(letter), ..] =
        contents
    else {
        return None;
    };
    let spelling = |character: &ast::Character| {
        source.get(character.span.start as usize..character.span.end as usize)
    };
    if spelling(backslash) != Some("\\")
        || spelling(control) != Some("c")
        || !char::from_u32(letter.value).is_some_and(|value| value.is_ascii_digit() || value == '_')
    {
        return None;
    }
    Some(letter.value & CONTROL_CODE_MASK)
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
            let in_range = |member: u32| *min <= member && member <= *max;
            if !ignore_case {
                return in_range(value);
            }
            // The range matches when one of its members has `value`'s canonical form.
            let key = canonical(value, unicode);
            (canonical(key, unicode) == key && in_range(key))
                || canonical_preimages_of(key, unicode).any(in_range)
        }
        ClassItem::Escape(escape) => escape_matches(*escape, value, ignore_case, unicode),
        ClassItem::Property {
            negative,
            name,
            value: property,
            matcher,
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
            let property_matches = |character| {
                resolved_property_matches(*matcher, name, property.as_deref(), character)
            };
            let matches = property_matches(value)
                || (ignore_case
                    && char::from_u32(value).is_some_and(|character| {
                        character
                            .to_uppercase()
                            .chain(character.to_lowercase())
                            .map(u32::from)
                            .any(property_matches)
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
            let mut sequences: Vec<[u32; KEYCAP_SEQUENCE_UNITS]> = vec![[b'#' as u32, 0xFE0F, 0x20E3], [b'*' as u32, 0xFE0F, 0x20E3]];
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

/// The most units a property of strings matches (`RGI_Emoji` and `RGI_Emoji_ZWJ_Sequence`).
const STRING_PROPERTY_MAX_UNITS: usize = 16;
/// The units of an `Emoji_Keycap_Sequence`: a key, U+FE0F, U+20E3.
const KEYCAP_SEQUENCE_UNITS: usize = 3;

fn string_property_widths(name: &str, input: &[Unit], position: usize) -> Vec<usize> {
    let mut widths = Vec::new();
    let limit = match name {
        "Basic_Emoji" => 2,
        "RGI_Emoji_Flag_Sequence" | "RGI_Emoji_Modifier_Sequence" => 3,
        "RGI_Emoji_Tag_Sequence" => 8,
        _ => STRING_PROPERTY_MAX_UNITS,
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

/// Whether `units` (code points) are one RGI emoji ZWJ sequence: a lookup in Unicode's list
/// (`emoji_zwj`), not any ZWJ-joined run of emoji, which would admit sequences Unicode never
/// recommended.
fn is_zwj_units(units: &[Unit]) -> bool {
    units.iter().any(|unit| unit.value == 0x200D)
        && emoji_zwj::is_rgi_zwj_sequence(units.iter().map(|unit| unit.value))
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

/// Whether every item of `class` matches exactly one unit, so its widths at any position are
/// `[1]` or none.
/// The most units one match of `class` takes: its longest string, a property of strings'
/// bound, one for a class of single units.
fn class_max_width(class: &ClassExpr) -> usize {
    class
        .items
        .iter()
        .map(|item| match item {
            ClassItem::String(units) => units.len(),
            ClassItem::Property { name, .. } if name == "Emoji_Keycap_Sequence" => KEYCAP_SEQUENCE_UNITS,
            ClassItem::Property { name, .. } if is_string_property(name) => STRING_PROPERTY_MAX_UNITS,
            ClassItem::Nested(nested) => class_max_width(nested),
            _ => 1,
        })
        .max()
        .unwrap_or(1)
        .max(1)
}

fn class_is_single_unit(class: &ClassExpr) -> bool {
    class.items.iter().all(|item| match item {
        ClassItem::String(_) => false,
        ClassItem::Property { name, .. } => {
            name != "Emoji_Keycap_Sequence" && !is_string_property(name)
        }
        ClassItem::Nested(nested) => class_is_single_unit(nested),
        _ => true,
    })
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
        ast::CharacterClassEscapeKind::S => char::from_u32(value).is_some_and(is_ecma_whitespace),
        ast::CharacterClassEscapeKind::NegativeS => {
            !char::from_u32(value).is_some_and(is_ecma_whitespace)
        }
        ast::CharacterClassEscapeKind::W => is_word_mode(value, ignore_case && unicode),
        ast::CharacterClassEscapeKind::NegativeW => !is_word_mode(value, ignore_case && unicode),
    }
}

pub fn property_matches(name: &str, value: Option<&str>, character: u32) -> bool {
    resolved_property_matches(
        compile_property_matcher(name, value),
        name,
        value,
        character,
    )
}

/// `property_matches` with the ICU matcher for `name`/`value` already resolved.
fn resolved_property_matches(
    matcher: Option<PropertyMatcher>,
    name: &str,
    value: Option<&str>,
    character: u32,
) -> bool {
    if (0xD800..=0xDFFF).contains(&character) {
        return surrogate_property_matches(name, value);
    }
    let Some(character) = char::from_u32(character) else {
        return false;
    };
    if let Some(matcher) = matcher {
        return matcher.matches(character);
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

fn surrogate_property_matches(name: &str, value: Option<&str>) -> bool {
    matches!(
        (name, value),
        ("Any", _)
            | ("Assigned", _)
            | ("C" | "Other", None)
            | (
                "General_Category" | "gc",
                Some("C" | "Cs" | "Other" | "Surrogate")
            )
            | (
                "Script" | "sc" | "Script_Extensions" | "scx",
                Some("Unknown" | "Zzzz")
            )
    )
}

#[derive(Clone, Copy)]
pub struct PropertyMatcher {
    kind: PropertyMatcherKind,
}

#[derive(Clone, Copy)]
enum PropertyMatcherKind {
    Any,
    Assigned,
    Script(icu_properties::props::Script),
    ScriptExtensions(icu_properties::props::Script),
    ScriptRanges(&'static [(u32, u32)]),
    GeneralCategory(icu_properties::props::GeneralCategory),
    GeneralCategoryGroup(icu_properties::props::GeneralCategoryGroup),
    Binary(fn(char) -> bool),
}

impl PropertyMatcher {
    pub fn matches(self, character: char) -> bool {
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
            PropertyMatcherKind::ScriptRanges(ranges) => ranges
                .iter()
                .any(|(start, end)| (*start..=*end).contains(&u32::from(character))),
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

pub fn compile_property_matcher(name: &str, value: Option<&str>) -> Option<PropertyMatcher> {
    use icu_properties::{props, PropertyParser};
    let new_script_ranges = value.and_then(|value| {
        NEW_UNICODE_SCRIPTS
            .iter()
            .find(|script| script.aliases.contains(&value))
            .map(|script| script.ranges)
    });
    let kind = if name == "Any" {
        PropertyMatcherKind::Any
    } else if name == "Assigned" {
        PropertyMatcherKind::Assigned
    } else if matches!(name, "Script" | "sc") {
        if let Some(ranges) = new_script_ranges {
            PropertyMatcherKind::ScriptRanges(ranges)
        } else {
            PropertyMatcherKind::Script(PropertyParser::<props::Script>::new().get_loose(value?)?)
        }
    } else if matches!(name, "Script_Extensions" | "scx") {
        if let Some(ranges) = new_script_ranges {
            PropertyMatcherKind::ScriptRanges(ranges)
        } else {
            PropertyMatcherKind::ScriptExtensions(
                PropertyParser::<props::Script>::new().get_loose(value?)?,
            )
        }
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

fn equal(left: u32, right: u32, ignore_case: bool, unicode: bool) -> bool {
    if ignore_case {
        canonical(left, unicode) == canonical(right, unicode)
    } else {
        left == right
    }
}
/// For each canonicalization mode, the code points whose canonical form is another code point,
/// as `(form, member)` pairs ordered by form: the inverse of `canonical`, which
/// case-insensitive class ranges need. The build computes both from `canonical` itself
/// (`build.rs`): scanning every code point had cost each process about 14M instructions at
/// its first case-insensitive class.
fn canonical_preimages(unicode: bool) -> &'static [(u32, u32)] {
    if unicode {
        &CANONICAL_PREIMAGES_UNICODE
    } else {
        &CANONICAL_PREIMAGES_BMP
    }
}

include!(concat!(env!("OUT_DIR"), "/canonical_preimages.rs"));

/// The code points other than `key` itself whose canonical form is `key`.
fn canonical_preimages_of(key: u32, unicode: bool) -> impl Iterator<Item = u32> {
    let table = canonical_preimages(unicode);
    let first = table.partition_point(|(form, _)| *form < key);
    table[first..]
        .iter()
        .take_while(move |(form, _)| *form == key)
        .map(|(_, member)| *member)
}

/// The units `\w` matches, as inclusive ranges.
/// The units `\w` also matches under unicode case folding: long s and the Kelvin sign.
fn in_units(value: u32, ranges: &[(u32, u32)]) -> bool {
    ranges.iter().any(|&(first, last)| first <= value && value <= last)
}
fn is_word(value: u32) -> bool {
    in_units(value, &WORD_UNITS)
}
fn is_word_mode(value: u32, ignore_case: bool) -> bool {
    is_word(value) || (ignore_case && in_units(value, &FOLDED_WORD_UNITS))
}
fn is_line_terminator(value: u32) -> bool {
    in_units(value, &LINE_TERMINATORS)
}

#[cfg(test)]
mod tests {
    use super::{
        code_unit_bytes, contains_code_unit_bytes, literal_finder, normalize_new_unicode_scripts,
        Flags, Regex, Subject, NEW_UNICODE_SCRIPTS,
    };

    /// The wide search's required-literal check: an empty literal occurs everywhere, one with a
    /// unit past UTF-16 nowhere.
    fn contains_code_units(input: &[u16], literal: &[u32]) -> bool {
        literal.is_empty()
            || code_unit_bytes(literal)
                .is_some_and(|needle| contains_code_unit_bytes(input, &literal_finder(&needle)))
    }

    #[test]
    fn code_unit_literals_match_whole_units() {
        let text = |text: &str| text.encode_utf16().collect::<Vec<_>>();
        let literal = |text: &str| text.encode_utf16().map(u32::from).collect::<Vec<_>>();
        assert!(contains_code_units(&text("aé=bc"), &literal("=b")));
        assert!(contains_code_units(&text("aé=bc"), &literal("")));
        assert!(!contains_code_units(&text("aé=bc"), &literal("cb")));
        // Little-endian bytes 01 02 03 00 hold the bytes of 0x0302 across a unit boundary.
        assert!(!contains_code_units(&[0x0201, 0x0003], &[0x0302]));
        assert!(contains_code_units(&[0x0201, 0x0302], &[0x0302]));
        assert!(!contains_code_units(&text("abc"), &[0x1_F600]));
    }

    #[test]
    fn multiline_end_anchor_accepts_crlf_and_other_ecmascript_terminators() {
        let regex = Regex::with_flags(r"^Connection: close\r\n$", Flags::from("m")).unwrap();
        for subject in [
            "A\r\nConnection: close\r\n\r\n",
            "A\nConnection: close\r\n\n",
            "A\u{2028}Connection: close\r\n\u{2028}",
            "A\u{2029}Connection: close\r\n\u{2029}",
        ] {
            assert!(regex.find_from(subject, 0).next().is_some(), "{subject:?}");
        }
    }

    #[test]
    fn new_unicode_script_normalization_borrows_unrelated_patterns() {
        for pattern in [r"\s+", r"\p{Letter}", r"\p{Script=Latin}"] {
            assert!(matches!(
                normalize_new_unicode_scripts(pattern),
                std::borrow::Cow::Borrowed(_)
            ));
        }
    }

    #[test]
    fn new_unicode_script_aliases_keep_their_ranges() {
        for script in NEW_UNICODE_SCRIPTS {
            let character = char::from_u32(script.ranges[0].0).unwrap().to_string();
            for alias in script.aliases {
                let positive =
                    Regex::with_flags(&format!(r"\p{{Script={alias}}}"), Flags::from("u")).unwrap();
                assert!(
                    positive.find_from(&character, 0).next().is_some(),
                    "positive property did not match {alias}"
                );

                let negative =
                    Regex::with_flags(&format!(r"\P{{Script={alias}}}"), Flags::from("u")).unwrap();
                assert!(
                    negative.find_from(&character, 0).next().is_none(),
                    "negative property matched {alias}"
                );
            }
        }
    }

    #[test]
    fn case_insensitive_ranges_match_members_not_folded_endpoints() {
        // U+03CE folds to U+038F, below the range's start: folding the endpoints empties
        // the range, while its members (all Greek lowercase letters here) still match.
        for flags in ["i", "iu"] {
            let regex = Regex::with_flags(r"^[\u0391-\u03ce]+$", Flags::from(flags)).unwrap();
            assert!(
                regex
                    .find_from("αβγδεζηθικλμνξοπρςστυφχψω", 0)
                    .next()
                    .is_some(),
                "{flags}"
            );
            let ascii = Regex::with_flags(r"^[a-z]+$", Flags::from(flags)).unwrap();
            assert!(ascii.find_from("QUENCH", 0).next().is_some(), "{flags}");
        }
    }

    #[test]
    fn sticky_matches_only_at_the_start_position() {
        let regex = Regex::with_flags(r"y|\d+", Flags::default()).unwrap();
        let subject = super::Subject::from_ascii("ab12y".into(), 0.."ab12y".len());
        assert!(regex.find_sticky_in_subject(&subject, 0).next().is_none());
        let digits = regex.find_sticky_in_subject(&subject, 2).next().unwrap();
        assert_eq!(digits.range, 2..4);
        assert_eq!(
            regex
                .find_sticky_in_subject(&subject, 4)
                .next()
                .unwrap()
                .range,
            4..5
        );
        assert!(regex.find_sticky_in_subject(&subject, 6).next().is_none());
    }

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
    fn empty_match_search_past_input_end_terminates() {
        let regex = Regex::with_flags("(?:)", Flags::default()).unwrap();
        let input = "A😀B".encode_utf16().collect::<Vec<_>>();
        assert!(regex
            .find_from_utf16(&input, input.len() + 1)
            .next()
            .is_none());
        assert!(regex.find_from("A😀B", "A😀B".len() + 1).next().is_none());
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

    #[test]
    fn subjects_shorter_than_every_match_fail_and_longer_ones_still_match() {
        // The minimum length gates every search path: ASCII and wide subjects, sticky and
        // ranged searches, with case folding, optional tails, and empty-matching parts.
        let find = |pattern: &str, flags: &str, text: &str, start: usize| {
            let regex = Regex::with_flags(pattern, Flags::from(flags)).unwrap();
            let units: std::rc::Rc<[u16]> = text.encode_utf16().collect::<Vec<_>>().into();
            let subject = Subject::new(units);
            let found = regex.find_in_subject(&subject, start).next().map(|m| m.range);
            let sticky = regex.find_sticky_in_subject(&subject, start).next().map(|m| m.range);
            let range = regex.find_range_in_subject(&subject, start, false);
            assert_eq!(found, range, "{pattern} on {text:?}");
            (found, sticky)
        };
        let route = r"^(?:\/users\/profile)(?:\/$)?$";
        assert_eq!(find(route, "i", "/", 0), (None, None));
        assert_eq!(find(route, "i", "/USERS/profile", 0), (Some(0..14), Some(0..14)));
        assert_eq!(find(route, "i", "/users/profile/", 0), (Some(0..15), Some(0..15)));
        assert_eq!(find("a{3}", "", "xaa", 0), (None, None));
        assert_eq!(find("a{3}", "", "xaaa", 1), (Some(1..4), Some(1..4)));
        assert_eq!(find("ab|c", "", "xc", 1), (Some(1..2), Some(1..2)));
        // Zero-width parts consume nothing, so an empty remainder still matches.
        assert_eq!(find("(?=)", "", "ab", 2), (Some(2..2), Some(2..2)));
        assert_eq!(find(r"(a?)\1$", "", "", 0), (Some(0..0), Some(0..0)));
        assert_eq!(find(r"[\q{}]", "v", "", 0), (Some(0..0), Some(0..0)));
        // One astral code point is two units of the subject.
        assert_eq!(find("\u{1F600}", "u", "x\u{1F600}", 1), (Some(1..3), Some(1..3)));
        assert_eq!(find(".", "u", "\u{1F600}", 0), (Some(0..2), Some(0..2)));
    }
}

#[cfg(test)]
mod canonical_preimage_tests {
    #[test]
    fn built_tables_are_the_inverse_of_canonical() {
        for unicode in [false, true] {
            assert_eq!(super::canonical_preimages(unicode), &*super::canonical::preimages(unicode));
        }
    }
}
