//! Per-site operand-kind feedback of the lane's arithmetic and comparison records: which kinds of
//! operand a site met on the paths its quickened int32 handler declined, kept as the lattice the
//! specializers choose their variants from (`OperandClass`).
//!
//! A site is the record of a `Binary` or `JumpBinaryFalse` instruction, and its feedback one byte
//! of the view's feedback bytes, which lie in front of the records (`Records`): a handler finds
//! its byte from its record alone, `ip - base` bytes before the first record. A byte is written
//! only where a quickened handler declined (its first miss, and the out-of-line handlers that
//! run past it), and only ever gains bits, so a path that records nothing costs nothing and a
//! site that never declined holds `OperandKinds::NONE`: all int32, or never run. Readers combine
//! it with what they sampled (`Vm::site_operand_kinds`).

use super::*;

/// The size of one record, which the feedback bytes in front of the records are counted in.
const RECORD_BYTES: usize = std::mem::size_of::<LaneInstruction>();

/// The kinds of operand a site met where its quickened int32 handler declined, as a set of bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct OperandKinds(u8);

/// The kinds of a site's operands ordered by how much a specialization must allow: a site never
/// declined (`Unseen`), only int32s, Numbers whose int32 step failed, anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OperandClass {
    Unseen,
    Int32,
    Number,
    Any,
}

#[allow(dead_code)]
impl OperandKinds {
    /// A site that never declined its int32 handler.
    pub(crate) const NONE: Self = Self(0);
    /// An int32 operand met in an execution that declined.
    pub(crate) const INT32: Self = Self(1 << 0);
    /// A Number that is no int32.
    pub(crate) const DOUBLE: Self = Self(1 << 1);
    /// A string operand.
    pub(crate) const STRING: Self = Self(1 << 2);
    /// A BigInt operand.
    pub(crate) const BIGINT: Self = Self(1 << 3);
    /// Any other operand: undefined, null, a boolean, an object, a symbol.
    pub(crate) const OTHER: Self = Self(1 << 4);
    /// Two int32 operands whose int32 step declined: the result left the int32 range.
    pub(crate) const WIDENED: Self = Self(1 << 5);
    /// The kinds past Numbers.
    const BEYOND_NUMBER: Self = Self(Self::STRING.0 | Self::BIGINT.0 | Self::OTHER.0);
    /// The kinds that make a Number site's int32 step fail.
    const WIDER_NUMBER: Self = Self(Self::DOUBLE.0 | Self::WIDENED.0);

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Where the site lies in the lattice `Unseen < Int32 < Number < Any`.
    pub(crate) const fn class(self) -> OperandClass {
        if self.intersects(Self::BEYOND_NUMBER) {
            OperandClass::Any
        } else if self.intersects(Self::WIDER_NUMBER) {
            OperandClass::Number
        } else if self.contains(Self::INT32) {
            OperandClass::Int32
        } else {
            OperandClass::Unseen
        }
    }

    /// Whether an int32 speculation at the site has failed: it met a double, an int32 result
    /// out of range, or an operand other than a Number.
    pub(crate) const fn declined_int32(self) -> bool {
        matches!(self.class(), OperandClass::Number | OperandClass::Any)
    }

    pub(crate) const fn has_string(self) -> bool {
        self.contains(Self::STRING)
    }

    pub(crate) const fn has_bigint(self) -> bool {
        self.contains(Self::BIGINT)
    }

    /// The kind of one operand: int32 and double by their tags, the rest out of line.
    #[inline(always)]
    fn of(heap: &crate::heap::Heap, value: Value) -> Self {
        if value.as_int().is_some() {
            Self::INT32
        } else if value.as_number().is_some() {
            Self::DOUBLE
        } else {
            Self::of_heap(heap, value)
        }
    }

    #[cold]
    #[inline(never)]
    fn of_heap(heap: &crate::heap::Heap, value: Value) -> Self {
        if heap.is_string(value) {
            Self::STRING
        } else if matches!(heap.get(value), Some(crate::heap::Cell::BigInt(_))) {
            Self::BIGINT
        } else {
            Self::OTHER
        }
    }

    /// The kinds of an execution's two operands; two int32s that reach here widened.
    #[inline(always)]
    fn of_pair(heap: &crate::heap::Heap, left: Value, right: Value) -> Self {
        let kinds = Self::of(heap, left).union(Self::of(heap, right));
        if kinds == Self::INT32 { kinds.union(Self::WIDENED) } else { kinds }
    }
}

/// Whether an instruction's record has a feedback byte: the arithmetic and comparison
/// instructions whose lane handlers quicken on int32 operands.
pub(super) fn has_operand_feedback(op: Op) -> bool {
    op.is_binary_arithmetic() || op.compares_and_jumps()
}

/// The feedback byte of the record at `ip` of the view whose first record is `base`.
///
/// # Safety
/// `ip` is a record of the view starting at `base`, which holds feedback bytes (`Records::new`).
#[inline(always)]
unsafe fn feedback_cell<'a>(base: Ip, ip: Ip) -> &'a std::cell::Cell<u8> {
    // SAFETY: the record's byte is `pc + 1` bytes before the first record, inside the
    // allocation of the view's records (`Records`), which the view owns.
    unsafe { &*base.cast::<std::cell::Cell<u8>>().sub(ip.offset_from(base) as usize + 1) }
}

/// Notes the kinds of the operands `left` and `right` on the record at `ip`, where its
/// quickened handler declined them: a byte read, and written only where it gains a bit.
///
/// # Safety
/// `ip` is a record of the running view, an arithmetic or comparison record
/// (`has_operand_feedback`).
#[inline(always)]
pub(super) unsafe fn note_operands<H: Host>(cx: *mut LaneContext<H>, ip: Ip, left: Value, right: Value) {
    // SAFETY: as the function's own.
    let (heap, cell) = unsafe { (&vm(cx).heap, feedback_cell((*cx).base, ip)) };
    let seen = OperandKinds(cell.get());
    let kinds = seen.union(OperandKinds::of_pair(heap, left, right));
    if kinds != seen {
        cell.set(kinds.0);
    }
}

/// `note_operands` for a handler's first miss, out of line so the handler's own code stays as it is.
///
/// # Safety
/// As `note_operands`.
#[cold]
#[inline(never)]
pub(super) unsafe fn note_first_miss<H: Host>(cx: *mut LaneContext<H>, ip: Ip, left: Value, right: Value) {
    // SAFETY: the caller's.
    unsafe { note_operands(cx, ip, left, right) }
}

/// A view's records, with the feedback bytes of its sites in front of them: one byte per record,
/// counted back from the first.
pub(super) struct Records {
    cells: Box<[std::cell::UnsafeCell<LaneInstruction>]>,
    /// The records of `cells` before the first record, which hold the feedback bytes.
    front: usize,
}

impl Records {
    /// A record that holds nothing, whose bytes the feedback bytes are.
    fn blank() -> LaneInstruction {
        LaneInstruction {
            handler: std::ptr::null(),
            operand: LaneOperand { jump: 0 },
            a: 0,
            b: 0,
            c: 0,
            call_link: std::cell::Cell::new(0),
        }
    }

    /// `records`, with feedback bytes (all `NONE`) in front of them where `feedback`.
    pub(super) fn new(records: Box<[LaneInstruction]>, feedback: bool) -> Self {
        let front = if feedback { records.len().div_ceil(RECORD_BYTES) } else { 0 };
        let cells = std::iter::repeat_with(Self::blank).take(front).chain(records).map(std::cell::UnsafeCell::new).collect();
        Self { cells, front }
    }

    /// The first record.
    pub(super) fn base(&self) -> Ip {
        // SAFETY: `front` records lie before the end of `cells`; the pointer keeps the whole
        // allocation's provenance, which the feedback bytes before it need.
        unsafe { std::cell::UnsafeCell::raw_get(self.cells.as_ptr().add(self.front)).cast_const() }
    }

    fn has_feedback(&self) -> bool {
        self.front != 0
    }
}

impl std::ops::Deref for Records {
    type Target = [std::cell::UnsafeCell<LaneInstruction>];

    fn deref(&self) -> &Self::Target {
        &self.cells[self.front..]
    }
}

impl LaneView {
    /// The kinds the record at `pc` met where its int32 handler declined; `NONE` where the
    /// instruction has no feedback byte.
    pub(crate) fn operand_kinds(&self, pc: usize) -> OperandKinds {
        if !self.records.has_feedback() || pc >= self.records.len() {
            return OperandKinds::NONE;
        }
        let base = self.records.base();
        // SAFETY: `pc` names a record of this view, which holds feedback bytes.
        OperandKinds(unsafe { feedback_cell(base, base.add(pc)) }.get())
    }
}

impl<H: Host> Vm<H> {
    /// The kinds the instruction at `pc` of `function` met where its int32 handler declined,
    /// for the specializers that choose int32, double, string or generic variants: `NONE` for
    /// a function with no lane view (the kinds are recorded by it only), an instruction with
    /// no feedback byte, and a site that never declined, whose readers fall back to what they
    /// sampled.
    pub(crate) fn site_operand_kinds(&self, program: ProgramId, function: u32, pc: usize) -> OperandKinds {
        self.programs
            .lane_views(program)
            .and_then(|views| views.get(function as usize))
            .and_then(super::super::program_store::derived_lane_view)
            .map_or(OperandKinds::NONE, |view| view.operand_kinds(pc))
    }

    /// `site_operand_kinds` of each instruction in `pcs` of `function`, in order.
    pub(crate) fn region_operand_kinds(&self, program: ProgramId, function: u32, pcs: std::ops::RangeInclusive<usize>) -> Vec<OperandKinds> {
        pcs.map(|pc| self.site_operand_kinds(program, function, pc)).collect()
    }
}

#[cfg(all(test, not(quench_profiled)))]
mod tests {
    use super::*;
    use crate::Engine;
    use crate::test_host::TestHost;

    /// The kinds each arithmetic or comparison site of `run` recorded after `source` ran.
    fn recorded(source: &str) -> Vec<OperandKinds> {
        let program = Engine::specialize(source, "operand-feedback.js").unwrap();
        let function = program.functions.iter()
            .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run"))
            .unwrap();
        let mut vm = Vm::new(TestHost::writing_to(Default::default()));
        vm.set_execution_mode(crate::ExecutionMode::Interpreted);
        vm.execute(&program).unwrap();
        let sites: Vec<usize> = program.function(function).instructions().enumerate()
            .filter(|(_, instruction)| has_operand_feedback(instruction.op()))
            .map(|(pc, _)| pc)
            .collect();
        assert!(!sites.is_empty(), "the function has arithmetic or comparison sites");
        sites.into_iter().map(|pc| vm.site_operand_kinds(ProgramId::MAIN, function as u32, pc)).collect()
    }

    const DRIVER: &str = "let r = 0; for (let k = 0; k < 64; k++) r = run(50, ARGUMENT); print(r);";

    fn run_with(body: &str, argument: &str) -> Vec<OperandKinds> {
        recorded(&format!("function run(n, x) {{ {body} }} {}", DRIVER.replace("ARGUMENT", argument)))
    }

    /// A site that only ever met int32s records nothing: its first miss never came.
    #[test]
    fn an_int32_site_records_nothing() {
        let kinds = run_with("let s = 0; for (let i = 0; i < n; i++) { s = s + i; } return s;", "1");
        assert!(kinds.iter().all(|kinds| kinds.class() <= OperandClass::Int32), "{kinds:?}");
    }

    /// An int32 and a double operand make the site a Number site, and nothing more.
    #[test]
    fn a_double_operand_makes_a_number_site() {
        let kinds = run_with("let s = 0; for (let i = 0; i < n; i++) { s = s + x; } return s;", "0.5");
        assert!(kinds.iter().any(|kinds| kinds.class() == OperandClass::Number && kinds.contains(OperandKinds::DOUBLE)), "{kinds:?}");
        assert!(kinds.iter().all(|kinds| kinds.class() <= OperandClass::Number), "{kinds:?}");
    }

    /// A concatenation makes the site generic and names the string.
    #[test]
    fn a_string_operand_makes_a_generic_site() {
        let kinds = run_with("let s = ''; for (let i = 0; i < n; i++) { s = s + x; } return s;", "'ab'");
        assert!(kinds.iter().any(|kinds| kinds.class() == OperandClass::Any && kinds.has_string()), "{kinds:?}");
    }

    /// Two int32s whose int32 step declines (an inexact quotient; an overflowing sum or product
    /// the handler completes itself) widen the site without a double operand.
    #[test]
    fn an_inexact_int32_quotient_widens_the_site() {
        let kinds = run_with("let s = 7; for (let i = 0; i < n; i++) { s = s / x; } return s;", "2");
        assert!(kinds.iter().any(|kinds| kinds.contains(OperandKinds::WIDENED) && kinds.class() == OperandClass::Number), "{kinds:?}");
    }

    /// A comparison against a double records it through its out-of-line handler.
    #[test]
    fn a_double_comparison_records_its_operands() {
        let kinds = run_with("let c = 0; for (let i = 0; i < n; i++) { if (i < x) { c = c + 1; } } return c;", "10.5");
        assert!(kinds.iter().any(|kinds| kinds.contains(OperandKinds::DOUBLE)), "{kinds:?}");
    }
}
