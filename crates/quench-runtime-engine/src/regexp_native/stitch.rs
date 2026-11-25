//! The b-jit's regexp programs: each instruction becomes the stencil row of its shape
//! (`quench-bjit/stencils/regexp_rows.rs`, `quench_regexp`'s own row run on the stencil
//! projection), copied and patched behind the entry routine with the u-jit's `NativeFn` ABI,
//! so a stitched search is placed and called where a compiled one is. What an instruction
//! names in the program's tables (sets, lists, guards, dispatch intervals, the rows of each pc
//! and resume) the rows read through the program record (`RegexpProgram`), which lives beside
//! the code.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_bjit::regexp::{self as codes, RegexpCode, RegexpProgram};
use crate::compiled::stitcher::{self, Behind};
use quench_bjit::{HOLES, Row, Stencil, padded_holes};
use quench_regexp::backtrack::matcher::Admission;
use quench_regexp::backtrack::native::{NativeCode, NativeFn, NativePlan, NativeRestarts, NATIVE_FRAME_WORDS};
use quench_regexp::backtrack::{Assertion, Inst, UnitTest};

/// The tables the program record points into, kept with the code.
struct Tables {
    pcs: Box<[usize]>,
    resumes: Box<[usize]>,
    masks: Box<[[u64; 2]]>,
    lists: Box<[u32]>,
    guards: Box<[[u64; 3]]>,
    intervals: Box<[[u32; 4]]>,
    dispatch_starts: Box<[u32]>,
    followers: Box<[u64]>,
}

/// A stitched search: its code, and the record and tables its rows read.
pub(crate) struct Stitched {
    code: Vec<u8>,
    program: Box<RegexpProgram>,
    tables: Tables,
    /// The code offsets of each pc's row, each resume's row (by id past `FIRST_RESUME`), and
    /// the accept, fail and overflow leaves, which the record holds as addresses once placed.
    pc_offsets: Vec<usize>,
    resume_offsets: Vec<Option<usize>>,
    leaves: [usize; 3],
}

const _: () = assert!(codes::FRAME_WORDS == NATIVE_FRAME_WORDS, "the stencil frame is the u-jit's");

fn test_word(test: UnitTest) -> u64 {
    match test {
        UnitTest::Exact(unit) => u64::from(unit),
        UnitTest::Set(set) => codes::TEST_SET | u64::from(set),
    }
}

/// A test's kind and its hole.
fn test_code(test: UnitTest) -> (u8, u64) {
    match test {
        UnitTest::Exact(unit) => (codes::EXACT, u64::from(unit)),
        UnitTest::Set(set) => (codes::SET, u64::from(set)),
    }
}

fn admission_words(admission: Admission<UnitTest>) -> [u64; 3] {
    [u64::from(admission.at_start), u64::from(admission.at_end), test_word(admission.units)]
}

fn assertion_code(assertion: Assertion) -> u8 {
    match assertion {
        Assertion::Start => 0,
        Assertion::End => 1,
        Assertion::StartBuffer => 2,
        Assertion::EndBuffer => 3,
        Assertion::EndBufferOptionalNewline => 4,
        Assertion::Boundary => 5,
        Assertion::NonBoundary => 6,
    }
}


/// Whether this target's library has the regexp rows (x86-64 builds them; `regexp_entry.rs`).
pub(crate) fn available() -> bool {
    stitcher::routine("entry_regexp").is_ok()
}

/// One placed row: a library row by its codes or a routine, its operand holes (2 to 4) and the
/// pc its taken successor is.
struct Planned {
    name: String,
    holes: [u64; 3],
    pc: u32,
    taken: Option<Target>,
}

#[derive(Clone, Copy)]
enum Target {
    Pc(u32),
    Backtrack,
    Restart,
}

fn row((kind, first, second): RegexpCode, pc: u32, holes: [u64; 3], taken: Option<Target>) -> Planned {
    Planned { name: codes::regexp_name((kind, first, second)), holes, pc, taken }
}

/// The row of the instruction at `pc`: its shape's codes and operands, and the pc it takes.
fn instruction(plan: &NativePlan, looks: &[u32], pc: u32) -> Planned {
    let slot = |look: u32| looks.iter().position(|known| *known == look).expect("a listed lookahead") as u64 * 8;
    let taken = |target: u32| Some(Target::Pc(target));
    match plan.insts[pc as usize] {
        Inst::Unit(test) => {
            let (kind, value) = test_code(test);
            row((codes::UNIT, kind, 0), pc, [value, 0, 0], None)
        }
        Inst::UnitBack(test) => {
            let (kind, value) = test_code(test);
            row((codes::UNIT_BACK, kind, 0), pc, [value, 0, 0], None)
        }
        Inst::UnitRepeat { test, min, max, greedy, atomic } => {
            let (kind, value) = test_code(test);
            let shape = if !greedy { 0 } else if atomic { 3 } else { 1 };
            row((codes::UNIT_REPEAT, kind, shape), pc, [value, u64::from(min), u64::from(max)], None)
        }
        Inst::UnitRepeatBack { test, min, max, greedy } => {
            let (kind, value) = test_code(test);
            row((codes::UNIT_REPEAT_BACK, kind, u8::from(greedy)), pc, [value, u64::from(min), u64::from(max)], None)
        }
        Inst::Assert(test) => row((codes::ASSERT, assertion_code(test.assertion), u8::from(test.multiline) | u8::from(test.word_ignore_case) << 1), pc, [0; 3], None),
        Inst::Split { alternative } => row((codes::SPLIT, 0, 0), pc, [u64::from(alternative), 0, 0], None),
        Inst::GuardedSplit { alternative, guard } => row((codes::GUARDED_SPLIT, 0, 0), pc, [u64::from(alternative), u64::from(guard), 0], taken(alternative)),
        Inst::Dispatch { table } => row((codes::DISPATCH, 0, 0), pc, [u64::from(table), 0, 0], None),
        Inst::Jump(target) => row((codes::JUMP, 0, 0), pc, [0; 3], taken(target)),
        Inst::CaptureOpen { temporary, count, restores } => row((codes::CAPTURE_OPEN, u8::from(restores), 0), pc, [u64::from(temporary), u64::from(count), 0], None),
        Inst::CaptureClose { capture, temporary } => row((codes::CAPTURE_CLOSE, 0, 0), pc, [u64::from(capture), u64::from(temporary), 0], None),
        Inst::CaptureCloseBack { capture, temporary } => row((codes::CAPTURE_CLOSE_BACK, 0, 0), pc, [u64::from(capture), u64::from(temporary), 0], None),
        Inst::ResetCaptures { first, count } => row((codes::RESET_CAPTURES, 0, 0), pc, [u64::from(first), u64::from(count), 0], None),
        Inst::LoopInit { counter } => row((codes::LOOP_INIT, 0, 0), pc, [u64::from(counter), 0, 0], None),
        Inst::LoopHead { counter, min, max, greedy, exit } => row((codes::LOOP_HEAD, u8::from(greedy), 0), pc, [u64::from(counter), u64::from(min) << 32 | u64::from(max), u64::from(exit)], taken(exit)),
        Inst::LoopIterate { start } => row((codes::LOOP_ITERATE, 0, 0), pc, [u64::from(start), 0, 0], None),
        Inst::EmptyCheck { start } => row((codes::EMPTY_CHECK, 0, 0), pc, [u64::from(start), 0, 0], None),
        Inst::LoopTail { counter, start, min, head } => row((codes::LOOP_TAIL, 0, 0), pc, [u64::from(counter) << 32 | u64::from(start), u64::from(min), u64::from(head)], taken(head)),
        Inst::Backreference { first, count, ignore_case, unicode } => row((codes::BACKREFERENCE, u8::from(ignore_case) | u8::from(unicode) << 1, 0), pc, [u64::from(first), u64::from(count), 0], None),
        Inst::BackreferenceBack { first, count, ignore_case, unicode } => row((codes::BACKREFERENCE, u8::from(ignore_case) | u8::from(unicode) << 1, 1), pc, [u64::from(first), u64::from(count), 0], None),
        Inst::ClassWidths { class, backward } => row((codes::CLASS_WIDTHS, u8::from(backward), 0), pc, [u64::from(class), 0, 0], None),
        Inst::Look { negative, end } => row((codes::LOOK, u8::from(negative), 0), pc, [u64::from(end), slot(pc), 0], None),
        Inst::LookSucceed => {
            let (look, negative, end) = look_of(plan, pc);
            row((codes::LOOK_SUCCEED, u8::from(negative), 0), pc, [u64::from(end), slot(look), 0], taken(end))
        }
        Inst::Match => Planned { name: "leave_regexp_match".to_string(), holes: [0; 3], pc, taken: None },
    }
}

/// The lookahead whose body ends at the `LookSucceed` at `pc`: its pc, negativity and end.
fn look_of(plan: &NativePlan, pc: u32) -> (u32, bool, u32) {
    plan.insts.iter().enumerate().find_map(|(look, inst)| match inst {
        Inst::Look { negative, end } if *end == pc + 1 => Some((look as u32, *negative, *end)),
        _ => None,
    }).expect("a LookSucceed ends a lookahead's body")
}

/// The resumes the program's instructions push, each with its row: a choice at every pc (a
/// split's alternative, a dispatch's branch, a loop's other way, a class's shorter match), a
/// repeat's give-back or take-more after it, a lookahead's failure.
fn resumes(plan: &NativePlan) -> Vec<(u8, u32, Planned)> {
    let mut rows = Vec::new();
    let resume = |kind: u8, second: u8, pc: u32, holes: [u64; 3], taken: u32| row((codes::RESUME, kind, second), pc, holes, Some(Target::Pc(taken)));
    for pc in 0..plan.insts.len() as u32 {
        rows.push((codes::RESUME_CHOICE, pc, resume(codes::RESUME_CHOICE, 0, pc, [0; 3], pc)));
    }
    for (repeat, inst) in plan.insts.iter().enumerate() {
        let after = repeat as u32 + 1;
        match *inst {
            Inst::UnitRepeat { greedy: true, atomic: false, .. } => rows.push((codes::RESUME_GREEDY, after, resume(codes::RESUME_GREEDY, 0, after, [0; 3], after))),
            Inst::UnitRepeat { test, greedy: false, .. } => {
                let (kind, value) = test_code(test);
                rows.push((codes::RESUME_LAZY, after, resume(codes::RESUME_LAZY, kind, after, [value, 0, 0], after)));
            }
            Inst::UnitRepeatBack { greedy: true, .. } => rows.push((codes::RESUME_GREEDY_BACK, after, resume(codes::RESUME_GREEDY_BACK, 0, after, [0; 3], after))),
            Inst::UnitRepeatBack { test, greedy: false, .. } => {
                let (kind, value) = test_code(test);
                rows.push((codes::RESUME_LAZY_BACK, after, resume(codes::RESUME_LAZY_BACK, kind, after, [value, 0, 0], after)));
            }
            Inst::Look { negative, end } => {
                let look = repeat as u32;
                rows.push((codes::RESUME_LOOK_FAIL, look, resume(codes::RESUME_LOOK_FAIL, u8::from(negative), look, [u64::from(end), 0, 0], end)));
            }
            _ => {}
        }
    }
    rows
}

/// The context's bytes past the six saved registers: its words and the lookaheads' marks, 8
/// more than a multiple of 16 so the call of the first row is aligned (`regexp_entry.rs`).
fn frame_bytes(looks: usize) -> u64 {
    let bytes = (codes::CONTEXT_MARKS + 8 * looks) as u64;
    if bytes % 16 == 8 { bytes } else { bytes + 8 }
}

/// The program's stitched search, or `None` where a row or routine is not in the library
/// (a target without the regexp rows).
pub(crate) fn stitch(plan: &NativePlan) -> Option<Stitched> {
    let count = plan.insts.len();
    let looks: Vec<u32> = plan.insts.iter().enumerate().filter_map(|(pc, inst)| matches!(inst, Inst::Look { .. }).then_some(pc as u32)).collect();
    let (restart_mode, restart) = match plan.restarts {
        NativeRestarts::Caller => (codes::RESTART_CALLER, [0; 3]),
        NativeRestarts::Every => (codes::RESTART_EVERY, [0; 3]),
        NativeRestarts::Admitted(admission) => (codes::RESTART_ADMITTED, admission_words(admission)),
    };
    // Rows: the enter row, each pc's row in order (so a pc's next row is the next pc's), the
    // backtrack and restart rows, each resume's row, then the fail and overflow leaves.
    let mut planned = vec![row((codes::ENTER, 0, 0), 0, [0; 3], Some(Target::Pc(plan.entry)))];
    planned.extend((0..count as u32).map(|pc| instruction(plan, &looks, pc)));
    let backtrack = planned.len();
    planned.push(row((codes::BACKTRACK, 0, 0), 0, [0; 3], Some(Target::Restart)));
    let restart_row = planned.len();
    planned.push(row((codes::RESTART, restart_mode, u8::from(plan.restart_skip.is_some())), 0, [0; 3], Some(Target::Pc(plan.entry))));
    let resume_rows = resumes(plan);
    let first_resume = planned.len();
    let ids: Vec<usize> = resume_rows.iter().map(|(kind, pc, _)| usize::from(*kind) * count + *pc as usize).collect();
    planned.extend(resume_rows.into_iter().map(|(_, _, row)| row));
    let fail = planned.len();
    planned.push(Planned { name: "leave_regexp_fail".to_string(), holes: [0; 3], pc: 0, taken: None });
    let overflow = planned.len();
    planned.push(Planned { name: "leave_regexp_overflow".to_string(), holes: [0; 3], pc: 0, taken: None });

    let mut tables = Tables {
        pcs: vec![0; count].into_boxed_slice(),
        resumes: vec![0; codes::RESUME_KINDS * count].into_boxed_slice(),
        masks: plan.masks.clone(),
        lists: plan.lists.clone(),
        guards: plan.guards.iter().map(|guard| admission_words(*guard)).collect(),
        intervals: Box::default(),
        dispatch_starts: Box::default(),
        followers: (0..=count).map(|pc| match plan.insts.get(pc) {
            Some(Inst::Unit(test)) => test_word(*test),
            _ => codes::NO_TEST,
        }).collect(),
    };
    let mut intervals = Vec::new();
    let mut starts = vec![0u32];
    for table in plan.dispatches.iter() {
        // An ASCII subject reaches only an interval's ASCII part.
        intervals.extend(table.iter().filter(|interval| interval.lo < 128).map(|interval| [interval.lo, interval.hi.min(127), interval.first, interval.count]));
        starts.push(intervals.len() as u32);
    }
    tables.intervals = intervals.into_boxed_slice();
    tables.dispatch_starts = starts.into_boxed_slice();
    let program = Box::new(RegexpProgram {
        pcs: tables.pcs.as_ptr(),
        resumes: tables.resumes.as_ptr(),
        masks: tables.masks.as_ptr(),
        lists: tables.lists.as_ptr(),
        guards: tables.guards.as_ptr(),
        intervals: tables.intervals.as_ptr(),
        dispatch_starts: tables.dispatch_starts.as_ptr(),
        followers: tables.followers.as_ptr(),
        folds: plan.folds,
        classes: plan.classes,
        class_ends: quench_regexp::backtrack::native::native_class_ends as *const () as usize,
        unset: plan.unset,
        insts: count,
        entry: plan.entry as usize,
        accept: 0,
        fail: 0,
        overflow: 0,
        restart,
        restart_skip: plan.restart_skip.map_or(codes::NO_TEST, test_word),
    });
    let address = &*program as *const RegexpProgram as u64;
    let holes: Vec<[u64; HOLES]> = planned.iter().map(|row| padded_holes([address, u64::from(row.pc), row.holes[0], row.holes[1], row.holes[2]])).collect();
    let stencils: Vec<Stencil> = planned.iter().map(|row| stitcher::library(&row.name)).collect::<Option<_>>()?;
    let index_of = |target: Target| match target {
        Target::Pc(pc) => 1 + pc as usize,
        Target::Backtrack => backtrack,
        Target::Restart => restart_row,
    };
    let placed: Vec<Row> = planned.iter().zip(&stencils).zip(&holes)
        .map(|((planned, stencil), hole)| Row { stencil, hole, taken: planned.taken.map(index_of), cold: false })
        .collect();

    let entry = stitcher::routine("entry_regexp").ok()?;
    // Every row's exit 0 (a failed step) is the backtrack row.
    let (code, _, offsets) = stitcher::stitch_behind((entry, &padded_holes([frame_bytes(looks.len())])), &[], &placed, Behind::Row(backtrack)).ok()?;
    let at = |row: usize| offsets[row];
    let mut resume_offsets = vec![None; codes::RESUME_KINDS * count];
    for (index, id) in ids.iter().enumerate() {
        resume_offsets[*id] = Some(at(first_resume + index));
    }
    let match_row = (0..count).find(|pc| matches!(plan.insts[*pc], Inst::Match)).map(|pc| 1 + pc);
    Some(Stitched {
        code,
        program,
        tables,
        pc_offsets: (0..count).map(|pc| at(1 + pc)).collect(),
        resume_offsets,
        leaves: [match_row.map_or(at(fail), at), at(fail), at(overflow)],
    })
}

impl Stitched {
    /// The search in executable memory: the record's code addresses filled in from where the
    /// code was placed.
    pub(crate) fn publish(self) -> Option<NativeCode> {
        let Self { code, mut program, mut tables, pc_offsets, resume_offsets, leaves } = self;
        let memory = crate::compiled::CodeMemory::new(&code)?;
        let base = memory.pointer() as usize;
        for (slot, offset) in tables.pcs.iter_mut().zip(&pc_offsets) {
            *slot = base + offset;
        }
        // A resume no instruction pushes leaves through the overflow routine, which hands the
        // search to the interpreter.
        for (slot, offset) in tables.resumes.iter_mut().zip(&resume_offsets) {
            *slot = base + offset.unwrap_or(leaves[2]);
        }
        program.accept = base + leaves[0];
        program.fail = base + leaves[1];
        program.overflow = base + leaves[2];
        // SAFETY: the code begins with the entry routine, of `NativeFn`'s signature.
        let entry = unsafe { std::mem::transmute::<*mut u8, NativeFn>(memory.pointer()) };
        Some(NativeCode { entry, keep: Box::new((memory, program, tables)) })
    }
}
