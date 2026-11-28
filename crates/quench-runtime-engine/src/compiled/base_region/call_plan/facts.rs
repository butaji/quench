//! What a call site's target record says of its callee (`DirectFacts`), and why a call takes the helper row (`choose`).
//! Split from `call_plan.rs` for its size.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// What the call site's target record says of its callee, read once (`DirectFacts::of`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DirectFacts {
    pub(crate) callee: u64,
    pub(crate) cell: usize,
    pub(crate) facts: CellFacts,
    /// The caller's call record and base, and the callee's function: the header words (`CallRecord::call`,
    /// `caller_base`, and `CalleeRecord::function` with the site's `CallRecord::packed`).
    pub(crate) call: u64,
    pub(crate) caller_base: u64,
    pub(crate) function: u32,
    pub(crate) captures: u64,
    /// The callee's closure environment and captures pointer (`CalleeRecord::env`, `capture`).
    pub(crate) env: u64,
    pub(crate) capture: u64,
    /// The global object a sloppy callee binds an `undefined` or `null` receiver to.
    pub(crate) global_this: Option<u64>,
    /// An arrow, which binds no receiver (only an AOT image's proven row names one).
    pub(crate) lexical: bool,
    pub(crate) registers: u16,
    /// The site's callee register holds this callee by a Stable fact (`Callees::stable`): the body
    /// subscribes to the fact, so its change retires the body, and the row need not check the register.
    pub(crate) held: bool,
    /// The last arm of a site whose plan has no miss (`Miss::None`): once every arm is chained, the row
    /// need not check the callee register, the closed world having no other callee (`plan_call`).
    pub(crate) closed: bool,
    /// The address of the callee's `DirectArm`, which the body owns (`Callees::owned`, moved into the
    /// published method or region whole, so the address holds): the native header's descriptor word
    /// (`CalleeRecord::descriptor`), as the u-jit's `push_native` writes it. Zero where none is known yet
    /// (an AOT record before its binding, `aot_image::bound`), which the row refuses to the helper.
    pub(crate) descriptor: u64,
}

impl DirectFacts {
    /// The facts of `callee`, whose cell's facts word is read now; `held` is whether the site's
    /// callee register is a Stable fact's (`DirectFacts::held`).
    ///
    /// # Safety
    /// `callee.cell` is the address of the callee function's live entry cell.
    pub(crate) unsafe fn of(callee: &DirectArm, held: bool) -> Self {
        // SAFETY: the caller's contract; the words are at the designed cell's offsets.
        let word = |offset: i32| unsafe { ((callee.body.cell + offset as usize) as *const u64).read() };
        Self {
            callee: callee.closure.callee.to_bits(),
            cell: callee.body.cell,
            facts: CellFacts::of_word(word(ENTRY_CELL.facts_offset)),
            call: callee.body.call as u64,
            caller_base: callee.body.caller_base as u64,
            function: callee.body.target.function,
            captures: callee.captures,
            env: callee.closure.context_words().0,
            capture: callee.closure.context_words().1,
            global_this: callee.sloppy().map(|global| global.to_bits()),
            // A live callee is never an arrow: no direct row binds a lexical `this` (only an AOT image's proven row does).
            lexical: false,
            registers: callee.body.target.registers,
            held,
            closed: false,
            descriptor: std::ptr::from_ref(&callee.body).addr() as u64,
        }
    }

    /// Whether the cell's facts describe this callee as a plain activation that takes `argc`
    /// arguments, so the stencil's admission (room) is all that is left to check.
    fn describes(&self, argc: u16) -> bool {
        let facts = self.facts;
        facts.registers == self.registers && facts.room == self.registers && facts.params >= argc && u64::from(facts.entry) == ENTRY_ACTIVATES
    }
}

/// Why a call takes the helper row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Declined {
    /// The site recorded no stable target.
    NoTarget,
    /// More arguments than the direct rows pass (`MAX_DIRECT_ARGC`).
    Arguments,
    /// The call is not a plain `Call`: no direct row makes it (`CallForm`).
    Form,
    /// A callee with no registers has no window the room check can refuse.
    Window,
    /// The cell's facts do not describe the callee (not yet published, or another function's).
    Facts,
    /// A returning call: its callee's activation under a host that makes proper tail calls must not
    /// grow the stack, which only the helper row's own admission (`returning_call_admitted`) knows.
    Returning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Direct,
    Helper(Declined),
}

/// The row class of a call: direct when its target's cell is stable and describes the callee,
/// else the helper row, with the first reason it cannot be direct.
pub(super) fn choose(shape: &CallShape, direct: Option<&DirectFacts>) -> Choice {
    let method = shape.form == CallForm::Method && shape.method.is_some();
    if shape.form != CallForm::Plain && !method {
        return Choice::Helper(Declined::Form);
    }
    let Some(facts) = direct else { return Choice::Helper(Declined::NoTarget) };
    let declined = [
        (usize::from(shape.argc) > MAX_DIRECT_ARGC, Declined::Arguments),
        (facts.registers == 0, Declined::Window),
        (!facts.describes(shape.argc), Declined::Facts),
    ];
    // A returning call is a candidate only where the helper would admit it: `lane_direct_callees` withholds
    // returning sites under a host with proper tail calls and a strict body (`lane_returning_call_admitted`).
    declined.into_iter().find(|(declined, _)| *declined).map_or(Choice::Direct, |(_, why)| Choice::Helper(why))
}
