//! The fold of a native string method's field read into its call. `s.charCodeAt(i)` reads the method off the receiver `s` into
//! the callee register, then calls it. Where the read is the global-held native `charCodeAt` (`GlobalRead::NativeMember`), its
//! callee dies in the call, and the instructions between the two are moves and constants that write neither the receiver nor the
//! callee and read nothing they write, the pair is one row: the call's `Heap::CharCodeAt`, which reads the receiver and the argument
//! directly. The read plans no row (`What::Folded`), and the row leaves at the read's PC, where the interpreter redoes the read and the
//! moves (a second run of them is the first, as they read nothing they write). The method's identity is held by the native holder's
//! exact shape (`Decoded::holder`): a change of its values retires the code, and the row holds that Stable fact, so a call the code
//! makes that changes it is read through the invalidation word.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::call_plan::{CallForm, CallShape};
use quench_runtime::staged::native_row::{native_row_of, NativeRow};

/// A field read folded into the call that invokes it: the read's PC, the call's, the call's row (which leaves at the read) and a
/// returning call's completion.
pub(crate) struct Fold {
    pub(crate) read: u32,
    pub(crate) call: u32,
    row: Decoded,
    completion: Option<Decoded>,
    /// The Number rows between the read and the call (local index, row), each leaving at the read where its operands are no Numbers.
    pub(crate) arithmetic: Vec<(usize, Decoded)>,
}

impl Fold {
    /// The rows of the call, in place of its own: the row that leaves at the read, then the completion.
    pub(crate) fn whats(self) -> Vec<What> {
        let mut whats = vec![What::Resume(self.row, self.read)];
        whats.extend(self.completion.map(What::Row));
        whats
    }
}

/// The folds of the field reads of `region`, where `plans` takes the calls the stitch plans (`Sites::plans`).
pub(crate) fn pairs(region: &LoopRegion, known: Known<'_>, plans: impl Fn(WideInstruction) -> bool) -> Vec<Fold> {
    let code = region.instructions();
    // The region's instructions some jump lands on: a row entered there would skip the read.
    let mut lands = Bits::default();
    for instruction in code.iter().filter(|instruction| instruction.op().control_flow_layout().jumps()) {
        if let Some(local) = (instruction.jump_target() as usize).checked_sub(region.header as usize) {
            lands.insert(local);
        }
    }
    (0..code.len()).filter_map(|k| fold_at(region, known, &plans, &lands, k)).collect()
}

/// The field read at the instruction `k` of `region`, with the call it folds into, if it is one (see the module's notes).
fn fold_at(region: &LoopRegion, known: Known<'_>, plans: &impl Fn(WideInstruction) -> bool, lands: &Bits, k: usize) -> Option<Fold> {
    use quench_runtime::vm::global_facts::GlobalRead;
    let code = region.instructions();
    let read = code[k];
    let pc = region.header + k as u32;
    (!lands.contains(k) && crate::compiled::meaning_compiled::is_get_field(read) && read.result_flags() == 0).then_some(())?;
    let &GlobalRead::NativeMember { native, holder, cell, .. } = known.globals?.get(&(pc as usize))? else { return None };
    (matches!(native_row_of(native), Some(NativeRow::CharCodeAt)) || math_kind(native).is_some()).then_some(())?;
    let callee = read.result_register();
    let Some(FieldLookup::Atom { base, .. }) = read.field_lookup() else { return None };
    let receiver = base.register_index()?;
    (callee != receiver).then_some(())?;
    let Between { call: j, reads, writes, arithmetic } = between(region, lands, k, plans)?;
    let at = region.header + j as u32;
    let (plain, returning) = code[j].split_return();
    let shape = CallShape { returning, ..CallShape::of(at, plain, CallForm::Plain) };
    // One or two arguments: `row_of` takes exactly the arity the native's operation reads (`Math.sqrt(x)`, `Math.min(i, 777)`).
    (shape.callee == callee && shape.this == receiver && (1..=2).contains(&shape.argc) && j + 1 < code.len() && !region.is_live(at + 1, callee)).then_some(())?;
    (reads.iter().all(|register| !writes.contains(register)) && !writes.contains(&receiver) && !writes.contains(&callee) && !reads.contains(&callee)).then_some(())?;
    let row = Decoded { holder: Some(holder), cell, ..row_of(native, &shape)? };
    let completion = shape.returning.then(|| Decoded { a: Some(Source::Register(shape.result)), ..Decoded::step(Step::Return, None) });
    Some(Fold { read: pc, call: region.header + j as u32, row, completion, arithmetic })
}

/// The instructions between the read at `k` and the call they lead to: the call's index `call`, the registers the moves and
/// Number rows among them read and write, and those rows (`arithmetic`). Only moves, constants and leaving Number rows
/// (`leaving_number`); none a jump lands on; the call a planned plain call.
struct Between {
    call: usize,
    reads: Vec<Register>,
    writes: Vec<Register>,
    arithmetic: Vec<(usize, Decoded)>,
}

fn between(region: &LoopRegion, lands: &Bits, k: usize, plans: &impl Fn(WideInstruction) -> bool) -> Option<Between> {
    let code = region.instructions();
    let mut between = Between { call: 0, reads: Vec::new(), writes: Vec::new(), arithmetic: Vec::new() };
    for j in k + 1..code.len() {
        if lands.contains(j) {
            return None;
        }
        let next = code[j];
        if CallForm::of(next.split_return().0) == Some(CallForm::Plain) {
            return plans(next).then(|| Between { call: j, ..between });
        }
        match () {
            _ if quench_runtime::staged::meaning::means(next, quench_runtime::staged::meaning::Meaning::Move) && next.result_flags() == 0 => between.reads.push(next.register_b()),
            _ if quench_runtime::staged::meaning::means(next, quench_runtime::staged::meaning::Meaning::LoadConst) && next.result_flags() == 0 => {}
            _ => {
                let row = leaving_number(region, next)?;
                between.reads.extend(row.registers().filter(|register| Some(*register) != row.dst));
                between.writes.extend(row.dst);
                between.arithmetic.push((j, row));
                continue;
            }
        }
        between.writes.extend(next.written_registers());
    }
    None
}

/// The Number row of `instruction` that leaves on any operand which is no Number (`numbers::self_checking`), so it runs no user
/// code: a leave re-runs the folded read from its PC, which is side-effect free. `Add` is excluded, its string variant concatenates.
fn leaving_number(region: &LoopRegion, instruction: WideInstruction) -> Option<Decoded> {
    let row = super::decode::numeric_row(&|index| region.number_constant(index), instruction)??;
    (row.kind != Kind::Add && instruction.result_flags() == 0 && super::numbers::self_checking(&row, None)).then_some(row)
}

/// The Number operation a native Math method computes (`math_call`'s row), if it is one: the row's
/// `MathCall` read as the base tier's `Kind`. The libm and two-Number functions are no row.
pub(crate) fn math_kind(native: quench_runtime::tier_surface::Native) -> Option<Kind> {
    use quench_runtime::staged::call_native::MathCall;
    use quench_runtime::staged::math::FloatUnary;
    Some(match quench_runtime::vm::number::math_call(native)? {
        MathCall::Unary(FloatUnary::Sqrt) => Kind::Sqrt,
        MathCall::Unary(FloatUnary::Floor) => Kind::Floor,
        MathCall::Unary(FloatUnary::Ceil) => Kind::Ceil,
        MathCall::Unary(FloatUnary::Trunc) => Kind::Trunc,
        MathCall::Unary(FloatUnary::Abs) => Kind::Abs,
        MathCall::Unary(FloatUnary::Round) => Kind::Round,
        MathCall::Unary(FloatUnary::Fround) => Kind::Fround,
        MathCall::Unary(FloatUnary::Sign) => Kind::Sign,
        MathCall::Extreme(true) => Kind::Max,
        MathCall::Extreme(false) => Kind::Min,
        MathCall::Imul => Kind::Imul,
        MathCall::LeadingZeros => Kind::Clz32,
        MathCall::Libm(_) | MathCall::Binary(_) => return None,
    })
}

/// The row a call of `native` with `shape` runs: `charCodeAt`'s `Heap::CharCodeAt` on the receiver and argument, or a Math
/// method's Number operation on its arguments, if the call takes the arguments it computes.
fn row_of(native: quench_runtime::tier_surface::Native, shape: &CallShape) -> Option<Decoded> {
    if matches!(native_row_of(native), Some(NativeRow::CharCodeAt)) {
        return (shape.argc == 1).then(|| Decoded::heap(Heap::CharCodeAt, Some(shape.result), Some(Source::Register(shape.this)), Some(Source::Register(shape.window))));
    }
    number_row(math_kind(native)?, shape)
}

/// The Number operation `kind` on a call's arguments: the window's first, and the second window register where the operation
/// takes two (`Math.min`, `Math.max`, `Math.imul`). The call must pass exactly those arguments.
pub(crate) fn number_row(kind: Kind, shape: &CallShape) -> Option<Decoded> {
    let binary = matches!(kind, Kind::Min | Kind::Max | Kind::Imul);
    (u16::from(binary) + 1 == shape.argc).then_some(())?;
    let second = binary.then(|| Source::Register(shape.window + 1));
    Some(Decoded::number(kind, Some(shape.result), Some(Source::Register(shape.window)), second, None))
}
