//! Expensive fallbacks, as errors (`QUENCH_DEV=strict-fallback`): every place a tier gives up on
//! an operation and pays the general path names it here, with the operation and its source
//! location, so each one is fixed where it appears instead of hiding in a profile.
//!
//! - `jit-reject`: a compile that wanted to run refused (`compiled::admission::Refusal`, past the
//!   tier model's pricing), or a hot loop retired to the interpreter because no region runs it.
//! - `helper-call`: an instruction a compiled body hands back to the lane or a helper row instead
//!   of running it in its own code.
//! - `deopt`: a compiled run that leaves its code for the interpreter before it completes.
//! - `opaque-native`: a compiled body (JIT or ahead image) that calls a native with no staged row,
//!   out of its own code.
//!
//! `strict-fallback` stops the process at the first fallback of any kind; `strict-fallback=report`
//! stops nowhere and prints each distinct fallback with its count at exit (`report`); kinds joined
//! by `+` (`strict-fallback=deopt+helper-call`, `=report+deopt`) stop at those kinds only. A kind
//! the census counts at zero is `Fallback::hard`: it stops the process whatever the setting, so it
//! cannot come back. Off, `on` is one load and every hook is a branch on a path that is already slow.

use crate::bytecode::ResidualProgram;
use crate::trace::Dev;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{Mutex, Once, OnceLock};

/// One kind of expensive fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fallback {
    JitReject,
    HelperCall,
    Deopt,
    OpaqueNative,
}

impl Fallback {
    /// Every kind, in `name` order.
    pub const ALL: [Fallback; 4] = [Self::JitReject, Self::HelperCall, Self::Deopt, Self::OpaqueNative];

    /// The kind's name in the setting, the report and the census (`fallbacks.txt`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::JitReject => "jit-reject",
            Self::HelperCall => "helper-call",
            Self::Deopt => "deopt",
            Self::OpaqueNative => "opaque-native",
        }
    }

    /// Whether the census holds this kind at zero: any fallback of it is an error in every run.
    const fn hard(self) -> bool {
        match self {
            Self::JitReject | Self::HelperCall | Self::Deopt | Self::OpaqueNative => false,
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// Whether some kind is hard, so the hooks run whatever the setting.
const ANY_HARD: bool = {
    let mut index = 0;
    let mut any = false;
    while index < Fallback::ALL.len() {
        any |= Fallback::ALL[index].hard();
        index += 1;
    }
    any
};

/// What the setting asks: the kinds that stop the process, and whether fallbacks are reported.
struct Mode {
    stops: u8,
    report: bool,
}

fn mode() -> &'static Mode {
    static MODE: OnceLock<Mode> = OnceLock::new();
    MODE.get_or_init(|| {
        let hard = Fallback::ALL.iter().filter(|kind| kind.hard()).fold(0, |bits, kind| bits | kind.bit());
        if !Dev::StrictFallback.on() {
            return Mode { stops: hard, report: false };
        }
        let Some(value) = Dev::StrictFallback.value() else {
            return Mode { stops: u8::MAX, report: false };
        };
        let mut mode = Mode { stops: hard, report: false };
        for part in value.split('+').filter(|part| !part.is_empty()) {
            match Fallback::ALL.iter().find(|kind| kind.name() == part) {
                Some(kind) => mode.stops |= kind.bit(),
                None if part == "report" => mode.report = true,
                None => eprintln!("Quench: unknown strict-fallback part `{part}`; known: report,{}",
                    Fallback::ALL.map(Fallback::name).join(",")),
            }
        }
        mode
    })
}

/// Whether fallbacks are noted: `QUENCH_DEV=strict-fallback`, or a kind is hard. The hooks ask this
/// before they gather what a note names.
#[inline]
pub fn on() -> bool {
    ANY_HARD || Dev::StrictFallback.on()
}

/// Where a fallback happened: an instruction of a function, in its program when the hook knows it.
#[derive(Clone, Copy)]
pub struct Site<'a> {
    pub program: Option<&'a ResidualProgram>,
    pub function: u32,
    pub pc: u32,
}

impl<'a> Site<'a> {
    pub fn new(program: &'a ResidualProgram, function: u32, pc: u32) -> Self {
        Self { program: Some(program), function, pc }
    }

    /// A site whose program the hook does not hold: named by its function and pc only.
    pub fn bare(function: u32, pc: u32) -> Self {
        Self { program: None, function, pc }
    }

    /// The operation of the instruction at the site, `?` where the hook holds no program.
    fn operation(&self) -> String {
        self.program
            .and_then(|program| program.function_get(self.function as usize))
            .and_then(|function| function.code.get(self.pc as usize).map(|packed| function.widened(*packed).op()))
            .map_or_else(|| "?".to_owned(), |op| format!("{op:?}"))
    }

    /// `file:line:column` of the site, or its function and pc where the source does not say.
    fn location(&self) -> String {
        let (function, pc) = (self.function, self.pc);
        match self.program {
            Some(program) => match program.source_position(function, pc) {
                Some(position) => format!("{}:{}:{}", program.source_name, position.line, position.column),
                None => format!("{} f{function}@{pc}", program.source_name),
            },
            None => format!("f{function}@{pc}"),
        }
    }
}

thread_local! {
    /// The function the compile in progress on this thread is about (`focus`): where a refusal
    /// that names no instruction happened.
    static FOCUS: std::cell::Cell<(*const ResidualProgram, u32, u32)> = const { std::cell::Cell::new((std::ptr::null(), 0, 0)) };
}

/// Restores the focus before it on drop (`focus`).
pub struct Focus(Option<(*const ResidualProgram, u32, u32)>);

impl Drop for Focus {
    fn drop(&mut self) {
        if let Some(before) = self.0 {
            FOCUS.with(|focus| focus.set(before));
        }
    }
}

/// Names `function` of `program`, at `pc`, as the subject of the compile in progress on this
/// thread until the guard drops: a refusal noted meanwhile (`note_in_focus`) happened there.
/// The program outlives the guard.
#[must_use]
pub fn focus(program: &ResidualProgram, function: u32, pc: u32) -> Focus {
    if !on() {
        return Focus(None);
    }
    Focus(Some(FOCUS.with(|focus| focus.replace((std::ptr::from_ref(program), function, pc)))))
}

/// `note` at the focused site (`focus`), or at no site where none is focused.
#[cold]
pub fn note_in_focus(kind: Fallback, what: std::fmt::Arguments<'_>) {
    let (program, function, pc) = FOCUS.with(std::cell::Cell::get);
    // SAFETY: a focused program outlives its guard, which restores the focus before it drops.
    let site = Site { program: unsafe { program.as_ref() }, function, pc };
    note_with_operation(kind, what, site, program.is_null().then(|| "?".to_owned()));
}

/// `note` at `pc` of `function` in the focused program (`focus`): a hook that knows the instruction
/// but not its program.
#[cold]
pub fn note_at_focus(kind: Fallback, what: std::fmt::Arguments<'_>, function: u32, pc: u32) {
    let (program, ..) = FOCUS.with(std::cell::Cell::get);
    // SAFETY: as `note_in_focus`.
    note(kind, what, Site { program: unsafe { program.as_ref() }, function, pc });
}

/// Notes a fallback of `kind` at `site`, for the reason `what`: stops the process when the setting
/// stops at `kind`, and counts it for the exit report when reporting.
#[cold]
pub fn note(kind: Fallback, what: std::fmt::Arguments<'_>, site: Site<'_>) {
    note_with_operation(kind, what, site, None);
}

fn note_with_operation(kind: Fallback, what: std::fmt::Arguments<'_>, site: Site<'_>, operation: Option<String>) {
    let mode = mode();
    let stops = mode.stops & kind.bit() != 0;
    if !stops && !mode.report {
        return;
    }
    let operation = operation.unwrap_or_else(|| site.operation());
    let mut what_text = String::new();
    let _ = what_text.write_fmt(what);
    let location = site.location();
    if stops {
        eprintln!(
            "Quench: strict-fallback: {} {operation} ({what_text}) at {location}\n  \
             QUENCH_DEV=strict-fallback=report lists every fallback of a run",
            kind.name()
        );
        std::process::exit(EXIT_STATUS);
    }
    REPORT.call_once(|| {
        // SAFETY: `report` is a plain function that touches only statics.
        unsafe { libc::atexit(report) };
    });
    let mut counts = COUNTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    *counts.entry((kind, operation, what_text, location)).or_insert(0) += 1;
}

/// The exit status of a run that a fallback stopped.
pub const EXIT_STATUS: i32 = 86;

/// The prefix of each report line.
pub const REPORT_PREFIX: &str = "QUENCH_FALLBACK";

type Counts = BTreeMap<(Fallback, String, String, String), u64>;

static COUNTS: Mutex<Counts> = Mutex::new(BTreeMap::new());
static REPORT: Once = Once::new();

/// The report: one line per distinct fallback, `QUENCH_FALLBACK kind operation (reason) location
/// count`, kinds in `Fallback::ALL` order and the most frequent first within a kind.
pub fn report_lines() -> Vec<String> {
    let counts = COUNTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut rows: Vec<_> = counts.iter().collect();
    rows.sort_by(|a, b| a.0 .0.cmp(&b.0 .0).then(b.1.cmp(a.1)).then(a.0.cmp(b.0)));
    rows.into_iter()
        .map(|((kind, operation, what, location), count)| format!("{REPORT_PREFIX} {} {operation} ({what}) {location} {count}", kind.name()))
        .collect()
}

extern "C" fn report() {
    for line in report_lines() {
        eprintln!("{line}");
    }
}
