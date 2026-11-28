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

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use oxc_ast::ast::BinaryOperator;

/// The size of one record, which the feedback bytes in front of the records are counted in.
const RECORD_BYTES: usize = std::mem::size_of::<LaneInstruction>();

pub(crate) use quench_runtime::vm::engine_types::OperandKinds;

/// The kinds of a site's operands ordered by how much a specialization must allow: a site never
/// declined (`Unseen`), only int32s, Numbers whose int32 step failed, anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OperandClass {
    Unseen,
    Int32,
    Number,
    Any,
}

#[quench_macros::ext(OperandKindsOps1)]
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
    const ELEMENTS: Self = Self(Self::DENSE.0 | Self::TYPED.0);
    /// The kinds past Numbers.
    const BEYOND_NUMBER: Self = Self(Self::STRING.0 | Self::BIGINT.0 | Self::OTHER.0);
    /// The kinds that make a Number site's int32 step fail.
    const WIDER_NUMBER: Self = Self(Self::DOUBLE.0 | Self::WIDENED.0);

    #[inline]
    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline(always)]
    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[inline(always)]
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

    pub(crate) const fn has_string(self) -> bool {
        self.contains(Self::STRING)
    }

    /// Whether an element site met a dense or typed element.
    pub(crate) const fn element_admitted(self) -> bool {
        self.intersects(Self::ELEMENTS)
    }

    /// The kind of one operand: int32 and double by their tags, a string or record by its handle's
    /// family, the rest out of line.
    #[inline(always)]
    fn of(heap: quench_runtime::vm::tier_queries::HeapReads<'_>, value: Value) -> Self {
        if value.as_int().is_some() {
            Self::INT32
        } else if value.is_double() {
            Self::DOUBLE
        } else if value.is_record() {
            // An object or array record, by its handle's family: no cell lookup, and no call
            // per execution for the `===` of two objects that notes its operands every time.
            Self::OTHER
        } else if value.direct_string_index().is_some() {
            Self::STRING
        } else {
            Self::of_cell(heap, value)
        }
    }

    /// The kind of a value whose family does not decide it: a legacy cell, a BigInt or other.
    #[cold]
    #[inline(never)]
    fn of_cell(heap: quench_runtime::vm::tier_queries::HeapReads<'_>, value: Value) -> Self {
        if matches!(heap.get(value), Some(quench_runtime::tier_surface::Cell::BigInt(_))) {
            Self::BIGINT
        } else {
            Self::OTHER
        }
    }

    /// The kinds of an execution's two operands; two int32s that reach here widened.
    #[inline(always)]
    fn of_pair(heap: quench_runtime::vm::tier_queries::HeapReads<'_>, left: Value, right: Value) -> Self {
        let kinds = Self::of(heap, left).union(Self::of(heap, right));
        if kinds == Self::INT32 { kinds.union(Self::WIDENED) } else { kinds }
    }
}

/// Whether an instruction's record has a feedback byte: the arithmetic and comparison
/// instructions whose lane handlers quicken on int32 operands.
pub(crate) fn has_operand_feedback(op: Op) -> bool {
    quench_runtime::staged::meaning::records_operand_feedback(op) || op.compares_and_jumps()
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
pub(crate) unsafe fn note_operands(cx: *mut LaneContext, ip: Ip, left: Value, right: Value) {
    // SAFETY: as the function's own.
    let (heap, cell) = unsafe { (vm(cx).heap_reads(), feedback_cell((*cx).base, ip)) };
    let seen = OperandKinds(cell.get());
    let kinds = seen.union(OperandKinds::of_pair(heap, left, right));
    if kinds != seen {
        cell.set(kinds.0);
    }
}

pub(crate) use quench_runtime::vm::engine_types::Records;

#[quench_macros::ext(RecordsOps1)]
impl Records {
    /// A record that holds nothing, whose bytes the feedback bytes are.
    pub(crate) fn blank() -> LaneInstruction {
        LaneInstruction {
            handler: std::ptr::null(),
            operand: LaneOperand { jump: 0 },
            a: 0,
            b: 0,
            c: 0,
            call_link: std::cell::Cell::new(0),
            mode: 0,
        }
    }

    /// `records`, with feedback bytes (all `NONE`) in front of them where `feedback`.
    pub(crate) fn new(records: Box<[LaneInstruction]>, feedback: bool) -> Self {
        let front = if feedback { records.len().div_ceil(RECORD_BYTES) } else { 0 };
        // The records move into their own allocation, grown by the feedback records in front
        // (`reserve_exact`, so no slack), instead of being copied into a second one: a derivation
        // holds one copy of its records at its peak, not two.
        let mut vector = Vec::from(records);
        vector.reserve_exact(front);
        vector.splice(0..0, std::iter::repeat_with(Self::blank).take(front));
        let cells: Box<[std::cell::UnsafeCell<LaneInstruction>]> =
            vector.into_iter().map(std::cell::UnsafeCell::new).collect::<Vec<_>>().into_boxed_slice();
        // SAFETY: `front` records lie before the end of `cells`; the pointer keeps the whole
        // allocation's provenance, which the feedback bytes before it need, and the boxed
        // records never move.
        let first = unsafe { std::cell::UnsafeCell::raw_get(cells.as_ptr().add(front)).cast_const() };
        Self { cells, front, first }
    }

    fn has_feedback(&self) -> bool {
        self.front != 0
    }
}



#[quench_macros::ext(LaneViewOpsX2)]
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

    /// Adds `kind` to the feedback byte of the element record at `pc` (`note_element_kind`), unless it holds it.
    pub(crate) fn note_element(&self, pc: usize, kind: OperandKinds) {
        if self.records.has_feedback() && pc < self.records.len() {
            let base = self.records.base();
            // SAFETY: `pc` names a record of this view, which holds feedback bytes.
            let cell = unsafe { feedback_cell(base, base.add(pc)) };
            cell.set(cell.get() | (kind.0 & !cell.get()));
        }
    }
}

#[quench_macros::ext(OperandFeedbackVm)]
impl Vm {
    /// Notes that the element access at `pc` of frame `f` met `kind`, in the frame function's lane view.
    pub(crate) fn note_element_kind(&self, f: usize, pc: usize, kind: OperandKinds) {
        let function = self.frames[f].function as usize;
        let view = self.programs.made_lane_views(self.active_program).and_then(|views| views.get(function));
        if let Some(view) = view.and_then(quench_runtime::vm::program_store::derived_lane_view) {
            view.note_element(pc, kind);
        }
    }

    /// The kinds the instruction at `pc` of `function` met where its int32 handler declined,
    /// for the specializers that choose int32, double, string or generic variants: `NONE` for
    /// a function with no lane view (the kinds are recorded by it only), an instruction with
    /// no feedback byte, and a site that never declined, whose readers fall back to what they
    /// sampled.
    pub(crate) fn site_operand_kinds(&self, program: ProgramId, function: u32, pc: usize) -> OperandKinds {
        self.programs
            .views_in_lane(program)
            .and_then(|views| views.get(function as usize))
            .and_then(quench_runtime::vm::program_store::derived_lane_view)
            .map_or(OperandKinds::NONE, |view| view.operand_kinds(pc))
    }

    /// `site_operand_kinds` of each instruction in `pcs` of `function`, in order.
    pub(crate) fn region_operand_kinds(&self, program: ProgramId, function: u32, pcs: std::ops::RangeInclusive<usize>) -> Vec<OperandKinds> {
        pcs.map(|pc| self.site_operand_kinds(program, function, pc)).collect()
    }
}

/// The language set of exactly String, as a declared parameter names it.
const STRING_LANGUAGES: u16 = quench_runtime::typed_ir::language_bit(quench_runtime::ValueType::String);
/// The language set of exactly Number, as a declared parameter names it.
const NUMBER_LANGUAGES: u16 = quench_runtime::typed_ir::language_bit(quench_runtime::ValueType::Number);

/// The declared language set (`Function::declared_parameters`) of the parameter that `register`
/// holds at entry, when it is exactly String or exactly Number.
fn declared_parameter_languages(function: &quench_runtime::bytecode::Function, register: u16) -> Option<u16> {
    let index = match function.parameter_registers {
        Some(base) => register.checked_sub(base).filter(|index| *index < function.params)?,
        None => function.local_registers.iter()
            .find(|entry| entry.register == register && entry.local < function.params)?.local,
    };
    let languages = *function.declared_parameters().get(usize::from(index))?;
    (languages == STRING_LANGUAGES || languages == NUMBER_LANGUAGES).then_some(languages)
}

/// The operator (a `BinaryOperator`'s discriminant) of a `Binary`, `NumericAdd` or
/// `JumpBinaryFalse` instruction; `None` for any other.
fn site_operator(instruction: WideInstruction) -> Option<u32> {
    let op = instruction.op();
    match op.immediate_role() {
        quench_runtime::bytecode::ImmediateRole::AdditionOperator => return Some(BinaryOperator::Addition as u32),
        quench_runtime::bytecode::ImmediateRole::BinaryOperator => return Some(instruction.binary_operator()),
        _ => {}
    }
    quench_runtime::bytecode::InstructionField::ALL.iter()
        .find(|&&field| op.field_layout(field) == quench_runtime::bytecode::FieldLayout::BinaryOperator)
        .map(|&field| u32::from(instruction.field_value(field)))
}

/// Whether `operator` is the addition or a relation (`<`, `<=`, `>`, `>=`, `==`, `!=`, `===`, `!==`)
/// whose feedback a declared operand pair seeds.
fn seeds_operands(operator: u32) -> bool {
    [BinaryOperator::Addition, BinaryOperator::LessThan, BinaryOperator::LessEqualThan,
        BinaryOperator::GreaterThan, BinaryOperator::GreaterEqualThan, BinaryOperator::Equality,
        BinaryOperator::Inequality, BinaryOperator::StrictEquality, BinaryOperator::StrictInequality]
        .iter()
        .any(|seeded| *seeded as u32 == operator)
}

/// The kinds a site's operands would note after one observation, when both of its operands are
/// declared parameters of exactly String or exactly Number: String when either is a String (a
/// string operand is what the site notes), else the Number pair an int32 and a double make.
/// `NONE` for any other site.
fn declared_operand_kinds(function: &quench_runtime::bytecode::Function, instruction: WideInstruction) -> OperandKinds {
    let Some(_operator) = site_operator(instruction).filter(|operator| seeds_operands(*operator)) else {
        return OperandKinds::NONE;
    };
    let languages = |register: Option<u16>| register.and_then(|register| declared_parameter_languages(function, register));
    let (Some(left), Some(right)) = (
        languages(instruction.operand_b().register_index()),
        languages(instruction.operand_c().register_index()),
    ) else {
        return OperandKinds::NONE;
    };
    if left == STRING_LANGUAGES || right == STRING_LANGUAGES {
        OperandKinds::STRING
    } else {
        OperandKinds::INT32.union(OperandKinds::DOUBLE)
    }
}

/// Seeds the feedback of the function's addition and comparison sites whose operands are declared
/// parameters (`declared_operand_kinds`), so each site's first handler choice is the one its
/// declared operands call for. A declaration is only a seed, since TypeScript is unsound: a wrong
/// seed costs one requickening, because the handler's guard checks the operands at run time.
///
/// An operand register counts as its parameter at entry; a parameter reassigned before the site
/// is seeded all the same, which a feedback hint tolerates.
pub(crate) fn seed_declared_feedback(function: &quench_runtime::bytecode::Function, records: &Records) {
    if !records.has_feedback() {
        return;
    }
    let base = records.base();
    for (pc, instruction) in function.instructions().enumerate().take(records.len()) {
        let kinds = declared_operand_kinds(function, instruction);
        if kinds == OperandKinds::NONE {
            continue;
        }
        // SAFETY: `pc` names a record of `records`, which holds feedback bytes (`Records::new`).
        let cell = unsafe { feedback_cell(base, base.add(pc)) };
        cell.set(OperandKinds(cell.get()).union(kinds).0);
    }
}

#[cfg(all(test, not(quench_profiled)))]
mod tests {
    use super::*;
    use quench_runtime::Engine;

    /// The kinds each arithmetic or comparison site of `run` recorded after `source` ran.
    fn recorded(source: &str) -> Vec<OperandKinds> {
        let program = Engine::specialize(source, "operand-feedback.js").unwrap();
        let function = program.functions.iter()
            .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run"))
            .unwrap();
        let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
        vm.set_execution_mode(quench_runtime::ExecutionMode::Interpreted);
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
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

    /// Two int32s whose int32 result leaves the int32 range (an overflowing sum, which the
    /// handler completes itself) widen the site without a double operand.
    #[test]
    fn an_int32_overflow_widens_the_site() {
        let kinds = run_with("let s = 1; for (let i = 0; i < n; i++) { s = s + x; } return s;", "2147483647");
        assert!(kinds.iter().any(|kinds| kinds.contains(OperandKinds::WIDENED) && kinds.class() == OperandClass::Number), "{kinds:?}");
    }

    /// A remainder by zero declines the int32 step of two int32s and widens the site likewise.
    #[test]
    fn an_int32_remainder_by_zero_widens_the_site() {
        let kinds = run_with("let s = 7; for (let i = 0; i < n; i++) { s = s % x; } return s;", "0");
        assert!(kinds.iter().any(|kinds| kinds.contains(OperandKinds::WIDENED) && kinds.class() == OperandClass::Number), "{kinds:?}");
    }

    /// A comparison against a double records it through its out-of-line handler.
    #[test]
    fn a_double_comparison_records_its_operands() {
        let kinds = run_with("let c = 0; for (let i = 0; i < n; i++) { if (i < x) { c = c + 1; } } return c;", "10.5");
        assert!(kinds.iter().any(|kinds| kinds.contains(OperandKinds::DOUBLE)), "{kinds:?}");
    }
}
