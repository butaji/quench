//! Repository-owned ECMAScript RegExp parser, IR, and interpreter.
//!
//! OXC supplies syntax facts; the VM owns the executable representation. A
//! guarded compiled backend is retained for supported patterns, with the
//! repository-owned matcher as the complete semantic fallback.

use std::{
    cell::{OnceCell, RefCell},
    ops::Range,
};

use oxc::regular_expression::{ast, LiteralParser, Options};

mod backtrack;
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

#[derive(Clone, Copy)]
enum Assertion {
    Start,
    End,
    StartBuffer,
    EndBuffer,
    EndBufferOptionalNewline,
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
/// A backtracking position. Captures are shared copy-on-write: choice points clone states
/// far more often than captures change.
#[derive(Clone)]
struct State {
    position: usize,
    captures: std::rc::Rc<Vec<Option<Range<usize>>>>,
}

impl State {
    /// Records capture `index`, copying the captures only while another state shares them.
    fn set_capture(&mut self, index: usize, capture: Option<Range<usize>>) {
        if let Some(slot) = std::rc::Rc::make_mut(&mut self.captures).get_mut(index) {
            *slot = capture;
        }
    }
}

// The semantic matcher must not silently discard alternatives.  Earlier code
// capped this vector at 4096 states, which could change a valid ECMAScript
// match.  The compiled linear backend handles ordinary patterns; unsupported
// constructs use this complete fallback and are allowed to retain every
// branch. Memory is finite for a finite input/pattern (though the fallback can
// still be exponentially large); it is no longer silently result-changing.
const MAX_BACKTRACK_STATES: usize = usize::MAX;
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
    compiled_dot_requires_cr_guard: bool,
    /// The automata backend, a guarded fast path for ASCII patterns, when it serves this
    /// pattern; `None` for every pattern unless an embedder asks for it
    /// (`QUENCH_COMPILED_REGEXP`). Boxed, so a pattern without it does not carry its space.
    byte_backend: Option<Box<ByteBackend>>,
}

/// The `regex` crate's form of a pattern (`Regex::byte_backend`). Regex syntax it cannot
/// represent (backreferences/lookaround and other ECMAScript-only constructs) continues
/// through the complete matcher.
struct ByteBackend {
    /// Whether the automaton's match results, captures included, are ECMAScript's.
    serves_matches: bool,
    /// Test-only matching may use regular syntax with captures whose reported values differ
    /// from ECMAScript; the boolean and end offset remain exact.
    serves_ranges: bool,
    /// The automaton, built from `source` when a match first needs it: building one can take
    /// megabytes (a long case-insensitive pattern such as a URI grammar), and most patterns a
    /// program constructs at startup never run. `None` inside is a pattern the backend
    /// refused, which the complete matcher serves.
    automaton: OnceCell<Option<regex::bytes::Regex>>,
    source: Box<str>,
    /// Reusable capture workspace. Creating `CaptureLocations` for every `exec`/`replace`
    /// call allocates a fresh vector on the hot path; the regex object is already cached and
    /// execution is single-threaded, so this disposable physical state can live beside it
    /// without changing semantic match data.
    locations: RefCell<Option<regex::bytes::CaptureLocations>>,
}

/// The repository matcher's form of a pattern.
struct CompleteMatcher {
    /// The lowered tree `match_expr` walks. Kept when no backtracking program compiles;
    /// otherwise the program serves every search, and the tree is lowered again only for
    /// a path that walks it.
    tree: OnceCell<Expr>,
    /// Literal units every match consumes contiguously (`required_literal`); empty if none.
    required_literal: Box<[u32]>,
    /// The compiled backtracking matcher (`backtrack::Program`), for every pattern without
    /// lookbehind or multi-unit classes.
    backtracking: Option<backtrack::Program>,
    /// `required_literal` as bytes, when every unit is ASCII.
    required_bytes: Option<LiteralFinder>,
    /// `required_literal` as the native-endian bytes of its code units, which a UTF-16
    /// subject's search looks for (`contains_code_unit_bytes`); `None` when it is empty or a
    /// unit lies past UTF-16, where it occurs in no code-unit text. Built by the first such
    /// search (`CompleteMatcher::required_wide`): most programs search only ASCII subjects,
    /// and a finder is several hundred bytes (110 of V8-v7 RegExp's 237 patterns had one).
    required_wide: OnceCell<Option<LiteralFinder>>,
    /// The fewest code points any match consumes (`min_length`). A code point is at least
    /// one unit of any subject, so a search with fewer units left has no match.
    min_length: usize,
}

/// A searcher for a literal byte run. The searcher is several hundred bytes, so it is boxed:
/// most patterns have no such literal, and theirs need not carry the space.
pub(crate) type LiteralFinder = Box<memchr::memmem::Finder<'static>>;

pub(crate) fn literal_finder(bytes: &[u8]) -> LiteralFinder {
    Box::new(memchr::memmem::Finder::new(bytes).into_owned())
}

impl CompleteMatcher {
    /// `required_wide`, built on first use.
    fn required_wide(&self) -> Option<&LiteralFinder> {
        self.required_wide
            .get_or_init(|| code_unit_bytes(&self.required_literal).map(|bytes| literal_finder(&bytes)))
            .as_ref()
    }

    /// Lowers a parsed pattern whose capture names are `names`.
    fn lower(pattern: &ast::Pattern<'_>, source: &str, names: &[String], flags: Flags) -> Self {
        let program = lower_tree(pattern, source, names);
        let backtracking = backtrack::Program::compile(&program, flags, names.len());
        let required_literal = required_literal(&program, flags);
        // A program that starts every ASCII search at its prefix finds the same literal
        // there, so the run's own search answers a subject without it; an anchored one tries
        // a single position, which costs less than scanning the subject for the literal.
        let required_bytes = (!required_literal.is_empty()
            && required_literal.iter().all(|unit| *unit <= u32::from(ASCII_MAX)))
        .then(|| required_literal.iter().map(|unit| *unit as u8).collect::<Vec<_>>())
        .filter(|bytes| {
            !backtracking
                .as_ref()
                .is_some_and(|program| program.searches_prefix(bytes) || program.anchored_at_start())
        })
        .map(|bytes| literal_finder(&bytes));
        let min_length = min_length(&program);
        let tree = OnceCell::new();
        if backtracking.is_none() {
            let _ = tree.set(program);
        }
        Self {
            tree,
            required_literal,
            backtracking,
            required_bytes,
            required_wide: OnceCell::new(),
            min_length,
        }
    }
}

/// Lowers a parsed pattern whose capture names are `names` to the tree `match_expr` walks.
fn lower_tree(pattern: &ast::Pattern<'_>, source: &str, names: &[String]) -> Expr {
    let mut lowering = Lowering {
        source,
        names: names.to_vec(),
        next_capture: 0,
    };
    lower_disjunction(&pattern.body, &mut lowering)
}

/// The byte matcher that answers range-only queries (`Regex::range_matcher`).
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
        // The `regex` crate is a useful literal fast path, but its capture
        // and character semantics are not ECMAScript-complete (notably
        // identity escapes, nullable quantified captures, dotAll and inline
        // mode modifiers).  Keep compilation behind a source-shape guard so
        // those patterns use the repository matcher instead of silently
        // changing observable match results.
        // Since the compiled backtracking matcher (`backtrack::Program`), the automata cost
        // more than they save: on V8-v7 RegExp (static x86_64, 2026-10-18) leaving them out
        // takes max RSS from 9544 to 8660 KiB and Score from 869 to 971, and on the npm top
        // 300 they add up to 5 MiB (semver 20.5 -> 25.4). They stay available, as a
        // linear-time bound on backtracking, for embedders that ask for them.
        let backend_enabled = !flags.unicode_sets
            && !flags.unicode
            && std::env::var_os("QUENCH_COMPILED_REGEXP").is_some();
        let compiled_serves_matches = backend_enabled
            && compiled_source_syntax_is_safe(source, NonCapturingGroups::Allowed)
            && compiled_structure_is_safe(&parsed.body);
        let compiled_serves_ranges = backend_enabled && compiled_test_source_is_safe(source);
        let byte_backend = (compiled_serves_matches || compiled_serves_ranges).then(|| {
            Box::new(ByteBackend {
                serves_matches: compiled_serves_matches,
                serves_ranges: compiled_serves_ranges,
                automaton: OnceCell::new(),
                source: source.into(),
                locations: RefCell::new(None),
            })
        });
        let complete = OnceCell::new();
        // The source stays kept either way: `tree` lowers it again when the backtracking
        // program let construction drop the tree.
        if !compiled_serves_matches {
            let _ = complete.set(CompleteMatcher::lower(
                &parsed,
                &normalized_source,
                &names,
                flags,
            ));
        }
        Ok(Self {
            complete,
            source: normalized_source.as_ref().into(),
            flags,
            capture_names: names,
            has_named_groups,
            compiled_dot_requires_cr_guard: !flags.dot_all && has_bare_dot(source),
            byte_backend,
        })
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
        let Some(program) = complete.backtracking.as_ref() else {
            return false;
        };
        let first = first().map(u32::from);
        (!(self.flags.unicode || self.flags.unicode_sets) && program.anchor_excludes(start, first))
            || (sticky && program.start_excludes(start, length, first))
    }

    /// Whether every search of a subject, ASCII (`ascii`) or not, reads its text as it is,
    /// never decoding it into the units a subject keeps once decoded (`Subject::decoded`):
    /// the backtracking program runs over ASCII bytes, and outside Unicode mode over code
    /// units too. A caller can then prepare such a subject for one search alone.
    pub fn searches_undecoded(&self, ascii: bool) -> bool {
        self.complete().backtracking.is_some() && (ascii || !(self.flags.unicode || self.flags.unicode_sets))
    }

    fn complete(&self) -> &CompleteMatcher {
        self.complete.get_or_init(|| {
            self.with_parsed_source(|parsed| {
                CompleteMatcher::lower(parsed, &self.source, &self.capture_names, self.flags)
            })
        })
    }

    /// The tree `match_expr` walks, lowered again from the kept source when the backtracking
    /// program let construction drop it.
    fn tree(&self) -> &Expr {
        self.complete().tree.get_or_init(|| {
            self.with_parsed_source(|parsed| lower_tree(parsed, &self.source, &self.capture_names))
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

    /// The byte backend's automaton when `serves` holds of it, built on first use.
    fn byte_automaton(&self, serves: impl FnOnce(&ByteBackend) -> bool) -> Option<&regex::bytes::Regex> {
        let backend = self.byte_backend.as_deref().filter(|backend| serves(backend))?;
        backend.automaton.get_or_init(|| compile_byte_regex(&backend.source, self.flags)).as_ref()
    }

    /// The byte matcher sound for match results, if any.
    fn compiled(&self) -> Option<&regex::bytes::Regex> {
        self.byte_automaton(|backend| backend.serves_matches)
    }

    /// The byte matcher sound for match ranges, if any.
    fn range_matcher(&self) -> Option<&regex::bytes::Regex> {
        self.byte_automaton(|backend| backend.serves_ranges)
    }

    /// The byte backend's capture workspace; only a pattern it serves reaches it.
    fn byte_locations(&self) -> std::cell::RefMut<'_, Option<regex::bytes::CaptureLocations>> {
        self.byte_backend.as_ref().expect("a pattern the byte backend serves").locations.borrow_mut()
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
        if text.is_ascii()
            && (!self.compiled_dot_requires_cr_guard || !text.as_bytes().contains(&b'\r'))
        {
            if let Some(compiled) = self.compiled() {
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
                let mut workspace = self.byte_locations();
                let locations =
                    workspace.get_or_insert_with(|| compiled.capture_locations());
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
        Matches {
            item: self.find_units(&units, first_unit_at(&units, start)),
        }
    }

    fn find_compiled(&self, compiled: &regex::bytes::Regex, bytes: &[u8], start: usize) -> Matches {
        let mut workspace = self.byte_locations();
        let locations = workspace.get_or_insert_with(|| compiled.capture_locations());
        let item = compiled
            .captures_read_at(locations, bytes, start)
            .map(|matched| Match {
                range: matched.range(),
                captures: (1..locations.len())
                    .map(|index| locations.get(index).map(|(start, end)| start..end))
                    .collect(),
                named: self.names_for_match(),
            });
        Matches { item }
    }

    /// Matches against a subject whose ASCII bytes were prepared once, so repeated matches
    /// over the same text do not rescan and recopy it.
    pub fn find_in_subject(&self, subject: &Subject, start: usize) -> Matches {
        if start > subject.len() {
            return Matches { item: None };
        }
        if let Some(found) = self.backtracking_find(subject, start, false, true) {
            return Matches { item: found };
        }
        if let (Some(bytes), Some(compiled)) = (subject.ascii(), self.compiled()) {
            if !self.compiled_dot_requires_cr_guard || !subject.has_carriage_return() {
                return self.find_compiled(compiled, bytes, start);
            }
        }
        let units = subject.decoded(self.flags.unicode || self.flags.unicode_sets);
        Matches {
            item: self.find_units(units, first_unit_at(units, start)),
        }
    }

    /// A searcher of `subject`'s match ranges, as `find_range_in_subject` finds them, with
    /// the backend chosen once for every search of a loop over the subject's matches.
    pub fn range_searcher<'a>(&'a self, subject: &'a Subject) -> RangeSearcher<'a> {
        let cr_admitted =
            || !self.compiled_dot_requires_cr_guard || !subject.has_carriage_return();
        let backend = match subject.ascii() {
            Some(bytes) => match self.range_matcher().or(self.compiled()) {
                Some(automata) if cr_admitted() => RangeBackend::Automata { automata, bytes },
                _ => match self.complete().backtracking.as_ref() {
                    Some(program) => RangeBackend::Backtracking {
                        program,
                        bytes,
                        linear: self.compiled().is_some() && cr_admitted(),
                    },
                    None => RangeBackend::General,
                },
            },
            None => RangeBackend::General,
        };
        RangeSearcher { regex: self, subject, backend }
    }

    /// The match that starts exactly at `start`, as a sticky (`y`) search requires. Only that
    /// position is tried, so a failing sticky match costs no scan of the rest of the text.
    pub fn find_sticky_in_subject(&self, subject: &Subject, start: usize) -> Matches {
        if start > subject.len() {
            return Matches { item: None };
        }
        if let Some(found) = self.backtracking_find(subject, start, true, true) {
            return Matches { item: found };
        }
        let units = subject.decoded(self.flags.unicode || self.flags.unicode_sets);
        let first = first_unit_at(units, start);
        Matches {
            item: self.match_at_positions(units, first..=first),
        }
    }

    /// The range of the next match in `subject`, for callers that observe no captures.
    /// A byte matcher that is sound for match ranges answers without capture bookkeeping.
    pub fn find_range_in_subject(&self, subject: &Subject, start: usize) -> Option<Range<usize>> {
        if start > subject.len() {
            return None;
        }
        if let Some(found) = self.backtracking_find(subject, start, false, false) {
            return found.map(|matched| matched.range);
        }
        if let (Some(bytes), Some(compiled)) = (
            subject.ascii(),
            self.range_matcher().or(self.compiled()),
        ) {
            if !self.compiled_dot_requires_cr_guard || !subject.has_carriage_return() {
                return compiled
                    .find_at(bytes, start)
                    .map(|matched| matched.range());
            }
        }
        self.find_in_subject(subject, start)
            .next()
            .map(|matched| matched.range)
    }

    /// The match the compiled backtracking matcher finds from `start` (only at `start` when
    /// `sticky`), or `None` when it does not answer: the pattern did not compile, the
    /// search ran past the budget it has while the linear-time backend can answer instead,
    /// or that backend answers a search for no captures, which its automata make without
    /// backtracking.
    fn backtracking_find(
        &self,
        subject: &Subject,
        start: usize,
        sticky: bool,
        captures: bool,
    ) -> Option<Option<Match>> {
        if let Some(bytes) = subject.ascii() {
            // A sticky search has no automata path: the repository matcher answers it.
            let automata = !sticky
                && if captures {
                    self.compiled().is_some()
                } else {
                    self.range_matcher().or(self.compiled()).is_some()
                };
            if automata
                && (!captures || self.capture_names.is_empty())
                && (!self.compiled_dot_requires_cr_guard || !subject.has_carriage_return())
            {
                return None;
            }
            let complete = self.complete();
            let program = complete.backtracking.as_ref()?;
            if bytes.len().saturating_sub(start) < complete.min_length {
                return Some(None);
            }
            // Every match consumes the required literal, so a subject without it has none.
            // A sticky search tries the one start, whose start-set check costs less than
            // scanning the whole remainder for the literal at every split position.
            if let Some(literal) = complete.required_bytes.as_ref().filter(|_| !sticky) {
                if literal.find(&bytes[start..]).is_none() {
                    return Some(None);
                }
            }
            let linear = self.compiled().is_some()
                && (!self.compiled_dot_requires_cr_guard || !subject.has_carriage_return());
            let budget = linear.then(|| backtrack::Program::budget(bytes.len()));
            let found = program.find(bytes, start, sticky, budget).ok()?;
            return Some(found.map(|found| Match {
                range: found.start..found.end,
                captures: found.captures,
                named: self.names_for_match(),
            }));
        }
        let unicode = self.flags.unicode || self.flags.unicode_sets;
        let complete = self.complete();
        let program = complete.backtracking.as_ref()?;
        if subject.len().saturating_sub(start) < complete.min_length {
            return Some(None);
        }
        if let (false, Some(units)) = (unicode, subject.wide()) {
            // Outside unicode mode a code unit is a pattern unit: no decoding, and
            // positions are offsets.
            if !sticky
                && !complete.required_literal.is_empty()
                && !complete
                    .required_wide()
                    .is_some_and(|literal| contains_code_unit_bytes(&units[start..], literal.as_ref()))
            {
                return Some(None);
            }
            let found = program.find(units, start, sticky, None).ok()?;
            return Some(found.map(|found| Match {
                range: found.start..found.end,
                captures: found.captures,
                named: self.names_for_match(),
            }));
        }
        let units = subject.decoded(unicode);
        let first = first_unit_at(units, start);
        if !sticky
            && !complete.required_literal.is_empty()
            && !contains_literal(&units[first..], &complete.required_literal)
        {
            return Some(None);
        }
        let found = program.find(units, first, sticky, None).ok()?;
        Some(found.and_then(|found| {
            // A sticky match must start at `start` itself, not inside a surrogate pair.
            if sticky && units.get(found.start).map_or(subject.len(), |unit| unit.offset_start) != start {
                return None;
            }
            Some(self.unit_match(units, found.start, found.end, found.captures))
        }))
    }

    pub fn find_from_utf16(&self, input: &[u16], start: usize) -> Matches {
        if start > input.len() {
            return Matches { item: None };
        }
        if input.iter().all(|unit| *unit <= ASCII_MAX)
            && (!self.compiled_dot_requires_cr_guard || !input.contains(&u16::from(b'\r')))
        {
            if let Some(compiled) = self.compiled() {
                let bytes = input.iter().map(|unit| *unit as u8).collect::<Vec<_>>();
                return self.find_compiled(compiled, &bytes, start);
            }
        }
        let units = units_from_utf16(input, self.flags.unicode || self.flags.unicode_sets);
        Matches {
            item: self.find_units(&units, first_unit_at(&units, start)),
        }
    }

    pub fn find_range_from_utf16(&self, input: &[u16], start: usize) -> Option<Range<usize>> {
        if input.iter().all(|unit| *unit <= ASCII_MAX)
            && (!self.compiled_dot_requires_cr_guard || !input.contains(&u16::from(b'\r')))
        {
            if let Some(compiled) = self.range_matcher() {
                let bytes = input.iter().map(|unit| *unit as u8).collect::<Vec<_>>();
                return compiled
                    .find_at(&bytes, start)
                    .map(|matched| matched.range());
            }
        }
        self.find_from_utf16(input, start)
            .next()
            .map(|matched| matched.range)
    }

    fn find_units(&self, input: &[Unit], first: usize) -> Option<Match> {
        let required_literal = &self.complete().required_literal;
        if !required_literal.is_empty()
            && !contains_literal(&input[first.min(input.len())..], required_literal)
        {
            return None;
        }
        self.match_at_positions(input, first..=input.len())
    }

    /// The first match starting at one of `positions`, tried in order.
    fn match_at_positions(
        &self,
        input: &[Unit],
        positions: std::ops::RangeInclusive<usize>,
    ) -> Option<Match> {
        let unset = std::rc::Rc::new(vec![None; self.capture_names.len()]);
        for position in positions {
            let state = State {
                position,
                captures: std::rc::Rc::clone(&unset),
            };
            if let Some(found) = match_expr(self.tree(), input, state, self.flags) {
                let captures = found.captures.iter().cloned().collect();
                return Some(self.unit_match(input, position, found.position, captures));
            }
        }
        None
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
    /// Whether the text holds a carriage return, found on first use: only the automata
    /// backend's dot guard asks (`compiled_dot_requires_cr_guard`).
    carriage_return: std::cell::OnceCell<bool>,
    /// The units decoded for the backtracking matcher, by code unit and by code point
    /// (`unicode`), each decoded on first use.
    decoded: [std::cell::OnceCell<Vec<Unit>>; 2],
}

impl Subject {
    pub fn new(units: std::rc::Rc<[u16]>) -> Self {
        // One branch-free pass classifies the text (vectorizable); a second narrows ASCII
        // text to bytes. Early-exit scans would stop the compiler from vectorizing either.
        let carriage_return = u16::from(b'\r');
        let (all_bits, has_carriage_return) =
            units.iter().fold((0, false), |(bits, carriage), unit| {
                (bits | *unit, carriage | (*unit == carriage_return))
            });
        let ascii = (all_bits <= ASCII_MAX).then(|| units.iter().map(|unit| *unit as u8).collect());
        Self {
            text: SubjectText::Wide { units, ascii },
            carriage_return: std::cell::OnceCell::from(has_carriage_return),
            decoded: Default::default(),
        }
    }

    /// A subject over UTF-16 text known to hold a non-ASCII unit, as a string's wide
    /// storage always does: neither the ASCII narrowing `new` tries nor its carriage-return
    /// scan can pay off, so preparing the subject touches no unit.
    pub fn from_non_ascii(units: std::rc::Rc<[u16]>) -> Self {
        debug_assert!(units.iter().any(|unit| *unit > ASCII_MAX));
        Self {
            text: SubjectText::Wide { units, ascii: None },
            carriage_return: std::cell::OnceCell::new(),
            decoded: Default::default(),
        }
    }

    /// A subject over the bytes `range` of ASCII text, matched in place without a copy.
    pub fn from_ascii(text: std::rc::Rc<str>, range: std::ops::Range<usize>) -> Self {
        debug_assert!(text.is_ascii() && range.end <= text.len());
        Self {
            text: SubjectText::Ascii { text, range },
            carriage_return: std::cell::OnceCell::new(),
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
            carriage_return: std::cell::OnceCell::new(),
            decoded: Default::default(),
        }
    }

    fn has_carriage_return(&self) -> bool {
        *self.carriage_return.get_or_init(|| match self.ascii() {
            Some(bytes) => bytes.contains(&b'\r'),
            None => self.wide().is_some_and(|units| units.contains(&u16::from(b'\r'))),
        })
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

/// `Regex::range_searcher`'s searcher.
pub struct RangeSearcher<'a> {
    regex: &'a Regex,
    subject: &'a Subject,
    backend: RangeBackend<'a>,
}

/// The backend that answers a range search of an ASCII subject: the steps
/// `find_range_in_subject` takes for every search, decided once.
enum RangeBackend<'a> {
    /// The automata serve ranges and the subject passes their carriage-return guard.
    Automata { automata: &'a regex::bytes::Regex, bytes: &'a [u8] },
    /// The backtracking program, with the step budget when the automata can take over.
    Backtracking { program: &'a backtrack::Program, bytes: &'a [u8], linear: bool },
    General,
}

impl RangeSearcher<'_> {
    /// `Regex::find_range_in_subject(subject, start)`.
    pub fn find(&self, start: usize) -> Option<Range<usize>> {
        let regex = self.regex;
        if start > self.subject.len() {
            return None;
        }
        match self.backend {
            RangeBackend::Automata { automata, bytes } => {
                automata.find_at(bytes, start).map(|matched| matched.range())
            }
            RangeBackend::Backtracking { program, bytes, linear } => {
                let complete = regex.complete();
                if bytes.len().saturating_sub(start) < complete.min_length {
                    return None;
                }
                if let Some(literal) = complete.required_bytes.as_ref() {
                    if literal.find(&bytes[start..]).is_none() {
                        return None;
                    }
                }
                let budget = linear.then(|| backtrack::Program::budget(bytes.len()));
                match program.find_bounds(bytes, start, false, budget) {
                    Ok(found) => found,
                    // Past the budget the automata answer, as `find_range_in_subject` does.
                    Err(_) => regex.find_range_in_subject(self.subject, start),
                }
            }
            RangeBackend::General => regex.find_range_in_subject(self.subject, start),
        }
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

fn compiled_test_source_is_safe(source: &str) -> bool {
    compiled_source_syntax_is_safe(source, NonCapturingGroups::Rejected)
        && !has_backreference(source)
}

/// Whether the byte backend may see `(?:` groups; any other `(?` form stays on the
/// repository matcher.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NonCapturingGroups {
    Allowed,
    Rejected,
}

/// Whether the byte backend's leftmost-first matching agrees with ECMAScript on this
/// pattern's structure. ECMAScript fails a quantifier iteration that matches empty while the
/// backend accepts it, and resets a repeated group's captures on every iteration while the
/// backend keeps a capture from an earlier iteration that the last one skipped; modifier
/// groups, lookaround and backreferences are left to the repository matcher.
fn compiled_structure_is_safe(disjunction: &ast::Disjunction<'_>) -> bool {
    disjunction
        .body
        .iter()
        .all(|alternative| alternative.body.iter().all(term_structure_is_safe))
}

fn term_structure_is_safe(term: &ast::Term<'_>) -> bool {
    match term {
        ast::Term::Quantifier(quantifier) => {
            !term_is_nullable(&quantifier.body)
                && (quantifier.max == Some(1) || captures_always_participate(&quantifier.body))
                && term_structure_is_safe(&quantifier.body)
        }
        ast::Term::CapturingGroup(group) => compiled_structure_is_safe(&group.body),
        ast::Term::IgnoreGroup(group) => {
            group.modifiers.is_none() && compiled_structure_is_safe(&group.body)
        }
        ast::Term::LookAroundAssertion(_)
        | ast::Term::IndexedReference(_)
        | ast::Term::NamedReference(_) => false,
        _ => true,
    }
}

/// Whether `term` can match the empty string; assertions and references count as nullable.
fn term_is_nullable(term: &ast::Term<'_>) -> bool {
    match term {
        ast::Term::Quantifier(quantifier) => {
            quantifier.min == 0 || term_is_nullable(&quantifier.body)
        }
        ast::Term::CapturingGroup(group) => disjunction_is_nullable(&group.body),
        ast::Term::IgnoreGroup(group) => disjunction_is_nullable(&group.body),
        ast::Term::Character(_)
        | ast::Term::Dot(_)
        | ast::Term::CharacterClassEscape(_)
        | ast::Term::UnicodePropertyEscape(_)
        | ast::Term::CharacterClass(_) => false,
        _ => true,
    }
}

fn disjunction_is_nullable(disjunction: &ast::Disjunction<'_>) -> bool {
    disjunction
        .body
        .iter()
        .any(|alternative| alternative.body.iter().all(term_is_nullable))
}

/// Whether every capture in `term` participates whenever `term` matches, so a repeated
/// `term`'s captures come from its last iteration under both reset and keep semantics.
fn captures_always_participate(term: &ast::Term<'_>) -> bool {
    match term {
        ast::Term::CapturingGroup(group) => disjunction_captures_always_participate(&group.body),
        ast::Term::IgnoreGroup(group) => disjunction_captures_always_participate(&group.body),
        ast::Term::Quantifier(quantifier) => {
            !term_has_capture(&quantifier.body)
                || (quantifier.min > 0 && captures_always_participate(&quantifier.body))
        }
        ast::Term::LookAroundAssertion(assertion) => !assertion
            .body
            .body
            .iter()
            .any(|alternative| alternative.body.iter().any(term_has_capture)),
        _ => true,
    }
}

fn disjunction_captures_always_participate(disjunction: &ast::Disjunction<'_>) -> bool {
    match disjunction.body.as_slice() {
        [alternative] => alternative.body.iter().all(captures_always_participate),
        alternatives => !alternatives
            .iter()
            .any(|alternative| alternative.body.iter().any(term_has_capture)),
    }
}

fn term_has_capture(term: &ast::Term<'_>) -> bool {
    let disjunction_has_capture = |disjunction: &ast::Disjunction<'_>| {
        disjunction
            .body
            .iter()
            .any(|alternative| alternative.body.iter().any(term_has_capture))
    };
    match term {
        ast::Term::CapturingGroup(_) => true,
        ast::Term::Quantifier(quantifier) => term_has_capture(&quantifier.body),
        ast::Term::IgnoreGroup(group) => disjunction_has_capture(&group.body),
        ast::Term::LookAroundAssertion(assertion) => disjunction_has_capture(&assertion.body),
        _ => false,
    }
}

fn has_bare_dot(source: &str) -> bool {
    let mut escaped = false;
    let mut in_class = false;
    for byte in source.bytes() {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'[' if !in_class => in_class = true,
            b']' => in_class = false,
            b'.' if !in_class => return true,
            _ => {}
        }
    }
    false
}

fn compiled_source_syntax_is_safe(source: &str, non_capturing: NonCapturingGroups) -> bool {
    let mut escaped = false;
    let mut bytes = source.bytes().peekable();
    while let Some(byte) = bytes.next() {
        if escaped {
            // In legacy mode, \u{n} is identity escape `u` followed by a
            // quantifier; the backend treats it as a Unicode code point.
            if byte == b'u' && bytes.peek() == Some(&b'{') {
                return false;
            }
            // The byte regexp backend accepts a narrower identity-escape
            // language than ECMAScript. Route legacy identity escapes to the
            // repository matcher instead of changing the matched value.
            if !matches!(
                byte,
                b'0'..=b'9'
                    | b'b'
                    | b'B'
                    | b'd'
                    | b'D'
                    | b'f'
                    | b'n'
                    | b'r'
                    | b's'
                    | b'S'
                    | b't'
                    | b'v'
                    | b'w'
                    | b'W'
                    | b'x'
                    | b'u'
                    | b'p'
                    | b'P'
                    | b'k'
                    | b'/'
                    | b'\\'
                    | b'['
                    | b']'
                    | b'('
                    | b')'
                    | b'{'
                    | b'}'
                    | b'*'
                    | b'+'
                    | b'?'
                    | b'|'
                    | b'.'
                    | b'^'
                    | b'$'
                    | b'-'
            ) && !escaped_punctuation_is_literal(byte)
            {
                return false;
            }
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
            continue;
        }
    }
    if escaped {
        return false;
    }
    let other_group_forms = match non_capturing {
        NonCapturingGroups::Allowed => source.replace("(?:", "").contains("(?"),
        NonCapturingGroups::Rejected => source.contains("(?"),
    };
    if other_group_forms {
        return false;
    }
    // A bare dot is cheap to match in the VM, while the byte backend treats
    // some JavaScript line terminators differently. Keep this narrow shape on
    // the semantic path without disabling ordinary dot-containing patterns.
    if matches!(source, "." | "^.$" | "$^.$") {
        return false;
    }
    true
}

/// Whether `\byte` is an identity escape of the same literal in ECMAScript and the byte
/// backend: ASCII punctuation, except `<` and `>`, which the backend reads as word
/// boundaries.
fn escaped_punctuation_is_literal(byte: u8) -> bool {
    byte.is_ascii_punctuation() && !matches!(byte, b'<' | b'>')
}

fn has_backreference(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'\\' {
            if matches!(bytes[index + 1], b'0'..=b'9' | b'k') {
                return true;
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    false
}

fn compile_byte_regex(source: &str, flags: Flags) -> Option<regex::bytes::Regex> {
    let mut builder = regex::bytes::RegexBuilder::new(source);
    builder
        .case_insensitive(flags.ignore_case)
        .multi_line(flags.multiline)
        .dot_matches_new_line(flags.dot_all)
        .unicode(false);
    builder.build().ok()
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
    fn class_has_strings(class: &ClassExpr) -> bool {
        class.items.iter().any(|item| match item {
            ClassItem::String(_) => true,
            ClassItem::Nested(nested) => class_has_strings(nested),
            _ => false,
        })
    }
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

fn required_literal(program: &Expr, flags: Flags) -> Box<[u32]> {
    fn visit(expr: &Expr, run: &mut Vec<u32>, best: &mut Vec<u32>) {
        match expr {
            Expr::Sequence(parts) => parts.iter().for_each(|part| visit(part, run, best)),
            Expr::Capture { body, .. } => visit(body, run, best),
            Expr::Literal(value) => run.push(*value),
            _ => end_run(run, best),
        }
    }
    fn end_run(run: &mut Vec<u32>, best: &mut Vec<u32>) {
        if run.len() > best.len() {
            std::mem::swap(run, best);
        }
        run.clear();
    }
    if flags.ignore_case {
        return Box::default();
    }
    let (mut run, mut best) = (Vec::new(), Vec::new());
    visit(program, &mut run, &mut best);
    end_run(&mut run, &mut best);
    best.into()
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

/// The index of the first decoded unit at or after UTF-16 offset `start`.
fn first_unit_at(units: &[Unit], start: usize) -> usize {
    units.partition_point(|unit| unit.offset_start < start)
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
        ast::Term::Character(character) => lower_character(character, &lowering.source),
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
        ast::Term::CharacterClass(class) => Expr::Class(lower_class(class, &lowering.source)),
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

fn decode_identifier_escapes(name: &str) -> Option<String> {
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

fn match_expr(expr: &Expr, input: &[Unit], state: State, flags: Flags) -> Option<State> {
    match_expr_with(expr, input, state, flags, &mut Some)
}

fn match_expr_with(
    expr: &Expr,
    input: &[Unit],
    state: State,
    flags: Flags,
    continuation: &mut dyn FnMut(State) -> Option<State>,
) -> Option<State> {
    match expr {
        Expr::Sequence(parts) => match_sequence_with(parts, 0, input, state, flags, continuation),
        Expr::Alternation(alternatives) => {
            for alternative in alternatives {
                if let Some(found) =
                    match_expr_with(alternative, input, state.clone(), flags, continuation)
                {
                    return Some(found);
                }
            }
            None
        }
        Expr::Literal(value) => input
            .get(state.position)
            .filter(|unit| equal(*value, unit.value, flags.ignore_case, flags.unicode))
            .map(|_| State {
                position: state.position + 1,
                ..state
            })
            .and_then(continuation),
        Expr::Dot => input
            .get(state.position)
            .filter(|unit| flags.dot_all || !is_line_terminator(unit.value))
            .map(|_| State {
                position: state.position + 1,
                ..state
            })
            .and_then(continuation),
        Expr::Class(class) => class_first_width(
            class,
            input,
            state.position,
            flags.ignore_case,
            flags.unicode,
        )
        .map(|width| State {
            position: state.position + width,
            ..state
        })
        .and_then(continuation),
        Expr::Capture { index, body } => {
            let start = state.position;
            match_expr_with(body, input, state, flags, &mut |mut result| {
                if result
                    .captures
                    .get(*index)
                    .is_some_and(|capture| !flags.reverse || capture.is_none())
                {
                    result.set_capture(*index, Some(start..result.position));
                }
                continuation(result)
            })
        }
        Expr::Repeat {
            body,
            min,
            max,
            greedy,
            captures,
        } => repeat_with(
            body,
            captures,
            input,
            state,
            flags,
            *min,
            *max,
            *greedy,
            0,
            continuation,
        ),
        Expr::Assertion(assertion) => assertion_matches(*assertion, input, state.position, flags)
            .then_some(state)
            .and_then(continuation),
        Expr::Lookaround { kind, body } => {
            lookaround_match(*kind, body, input, state, flags).and_then(continuation)
        }
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
            let next = if let Some(range) = range {
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
            } else if flags.reverse {
                None
            } else {
                Some(state)
            };
            next.and_then(continuation)
        }
        Expr::Mode { body, flags: local } => {
            match_expr_with(body, input, state, merge_flags(flags, *local), continuation)
        }
    }
}

fn match_sequence_with(
    parts: &[Expr],
    index: usize,
    input: &[Unit],
    state: State,
    flags: Flags,
    continuation: &mut dyn FnMut(State) -> Option<State>,
) -> Option<State> {
    let Some(part) = parts.get(index) else {
        return continuation(state);
    };
    match_expr_with(part, input, state, flags, &mut |candidate| {
        match_sequence_with(parts, index + 1, input, candidate, flags, continuation)
    })
}

fn repeat_with(
    body: &Expr,
    captures: &[usize],
    input: &[Unit],
    state: State,
    flags: Flags,
    min: usize,
    max: Option<usize>,
    greedy: bool,
    count: usize,
    continuation: &mut dyn FnMut(State) -> Option<State>,
) -> Option<State> {
    if matches!(body, Expr::Literal(_) | Expr::Dot | Expr::Class(_)) {
        return repeat_simple_with(body, input, state, flags, min, max, greedy, continuation);
    }
    let limit = max.unwrap_or(input.len().saturating_add(1));
    if !greedy && count >= min {
        if let Some(found) = continuation(state.clone()) {
            return Some(found);
        }
    }
    if count < limit {
        let mut iteration_state = state.clone();
        if !flags.reverse {
            for &index in captures {
                if iteration_state
                    .captures
                    .get(index)
                    .is_some_and(Option::is_some)
                {
                    iteration_state.set_capture(index, None);
                }
            }
        }
        let position = state.position;
        if let Some(found) = match_expr_with(body, input, iteration_state, flags, &mut |next| {
            if next.position != position || count < min {
                repeat_with(
                    body,
                    captures,
                    input,
                    next,
                    flags,
                    min,
                    max,
                    greedy,
                    count + 1,
                    continuation,
                )
            } else {
                None
            }
        }) {
            return Some(found);
        }
    }
    if greedy && count >= min {
        continuation(state)
    } else {
        None
    }
}

fn repeat_simple_with(
    body: &Expr,
    input: &[Unit],
    state: State,
    flags: Flags,
    min: usize,
    max: Option<usize>,
    greedy: bool,
    continuation: &mut dyn FnMut(State) -> Option<State>,
) -> Option<State> {
    let limit = max.unwrap_or(input.len().saturating_add(1));
    let mut positions = Vec::new();
    let mut position = state.position;
    let mut count = 0;
    while count < limit {
        let current = State {
            position,
            captures: state.captures.clone(),
        };
        let Some(next) = match_expr(body, input, current, flags) else {
            break;
        };
        if next.position == position {
            break;
        }
        position = next.position;
        count += 1;
        if count >= min {
            positions.push(position);
        }
    }

    let try_position = |position, continuation: &mut dyn FnMut(State) -> Option<State>| {
        let mut candidate = state.clone();
        candidate.position = position;
        continuation(candidate)
    };
    if greedy {
        for position in positions.into_iter().rev() {
            if let Some(found) = try_position(position, continuation) {
                return Some(found);
            }
        }
        if min == 0 {
            try_position(state.position, continuation)
        } else {
            None
        }
    } else {
        if min == 0 {
            if let Some(found) = try_position(state.position, continuation) {
                return Some(found);
            }
        }
        for position in positions {
            if let Some(found) = try_position(position, continuation) {
                return Some(found);
            }
        }
        None
    }
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
        Assertion::StartBuffer => position == 0,
        Assertion::EndBuffer => position == input.len(),
        Assertion::EndBufferOptionalNewline => {
            position == input.len()
                || (position + 1 == input.len()
                    && input
                        .get(position)
                        .is_some_and(|unit| is_line_terminator(unit.value)))
                || (position + 2 == input.len()
                    && input
                        .get(position)
                        .is_some_and(|unit| unit.value == '\r' as u32)
                    && input
                        .get(position + 1)
                        .is_some_and(|unit| unit.value == '\n' as u32))
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
                    result.set_capture(*index, Some(result.position..end));
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
            ..
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
        if output.len() == MAX_BACKTRACK_STATES {
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
    struct Visit<'a> {
        body: &'a Expr,
        input: &'a [Unit],
        flags: Flags,
        min: usize,
        limit: usize,
        greedy: bool,
    }
    fn visit(context: &Visit<'_>, state: State, count: usize, output: &mut Vec<State>) {
        if output.len() == MAX_BACKTRACK_STATES {
            return;
        }
        if !context.greedy && count >= context.min {
            output.push(state.clone());
            if output.len() == MAX_BACKTRACK_STATES {
                return;
            }
        }
        if count < context.limit {
            for next in reverse_options(context.body, context.input, state.clone(), context.flags) {
                if next.position != state.position || count < context.min {
                    visit(context, next, count + 1, output);
                    if output.len() == MAX_BACKTRACK_STATES {
                        return;
                    }
                }
            }
        }
        if context.greedy && count >= context.min && output.len() < MAX_BACKTRACK_STATES {
            output.push(state);
        }
    }
    let mut output = Vec::new();
    visit(
        &Visit {
            body,
            input,
            flags,
            min,
            limit,
            greedy,
        },
        state,
        0,
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

/// Whether every item of `class` matches exactly one unit, so its widths at any position are
/// `[1]` or none.
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

/// The longest width `class` matches at `position`: the first of `class_match_widths`.
fn class_first_width(
    class: &ClassExpr,
    input: &[Unit],
    position: usize,
    ignore_case: bool,
    unicode: bool,
) -> Option<usize> {
    if class_is_single_unit(class) {
        return input
            .get(position)
            .filter(|unit| class_matches(class, unit.value, ignore_case, unicode))
            .map(|_| 1);
    }
    class_match_widths(class, input, position, ignore_case, unicode)
        .into_iter()
        .next()
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
    resolved_property_matches(compile_property_matcher(name, value), name, value, character)
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
            assert!(regex.find_from("αβγδεζηθικλμνξοπρςστυφχψω", 0).next().is_some(), "{flags}");
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
        assert_eq!(regex.find_sticky_in_subject(&subject, 4).next().unwrap().range, 4..5);
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
    fn byte_backend_is_built_on_first_match_and_keeps_its_results() {
        // A pattern the byte backend serves builds no automaton until a match needs it, and
        // matching through it, before and after, gives the same results. The backend is
        // opt-in (`QUENCH_COMPILED_REGEXP`), so without it no automaton is ever built.
        let host = r"^(?:[a-z][a-z0-9+\-.]*:)?\/\/(?:[a-z0-9\-._~]|%[0-9a-f]{2})+(?::\d{1,5})?$";
        let regex = Regex::with_flags(host, Flags::from("i")).unwrap();
        let automaton = || regex.byte_backend.as_ref().and_then(|backend| backend.automaton.get());
        assert!(automaton().is_none());
        let found = |text: &str| regex.find_from(text, 0).next().map(|m| m.range);
        assert_eq!(found("HTTP://Example.COM:8080"), Some(0..23));
        assert_eq!(automaton().is_some(), regex.byte_backend.as_ref().is_some_and(|backend| backend.serves_matches));
        assert_eq!(found("http://exa mple"), None);
        let captures = Regex::with_flags(r"(\w+)-(\d+)", Flags::from("")).unwrap();
        let matched = captures.find_from("id: abc-42", 0).next().unwrap();
        assert_eq!((matched.range, matched.captures), (4..10, vec![Some(4..7), Some(8..10)]));
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
            let range = regex.find_range_in_subject(&subject, start);
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
