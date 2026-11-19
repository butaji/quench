//! The fold of a native string method's field read into its call. `s.charCodeAt(i)` reads the method off the receiver `s` into
//! the callee register, then calls it. Where the read is the global-held native `charCodeAt` (`GlobalRead::NativeMember`), its
//! callee dies in the call, and the instructions between the two are moves and constants that write neither the receiver nor the
//! callee and read nothing they write, the pair is one row: the call's `Heap::CharCodeAt`, which reads the receiver and the argument
//! directly. The read plans no row (`What::Folded`), and the row leaves at the read's PC, where the interpreter redoes the read and the
//! moves (a second run of them is the first, as they read nothing they write). The method's identity is held by the native holder's
//! exact shape (`Decoded::holder`): a change of its values retires the code, and the row holds that Stable fact, so a call the code
//! makes that changes it is read through the invalidation word.
use super::*;
use super::call_plan::{CallForm, CallShape};

/// A field read folded into the call that invokes it: the read's PC, the call's, the call's row (which leaves at the read) and a
/// returning call's completion.
pub(super) struct Fold {
    pub(super) read: u32,
    pub(super) call: u32,
    row: Decoded,
    completion: Option<Decoded>,
}

impl Fold {
    /// The rows of the call, in place of its own: the row that leaves at the read, then the completion.
    pub(super) fn whats(self) -> Vec<What> {
        let mut whats = vec![What::Resume(self.row, self.read)];
        whats.extend(self.completion.map(What::Row));
        whats
    }
}

/// The folds of the field reads of `region`, where `plans` takes the calls the stitch plans (`Sites::plans`).
pub(super) fn pairs(region: &LoopRegion, known: Known<'_>, plans: impl Fn(WideInstruction) -> bool) -> Vec<Fold> {
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
    use crate::vm::global_facts::GlobalRead;
    let code = region.instructions();
    let read = code[k];
    let pc = region.header + k as u32;
    (!lands.contains(k) && read.op().is_get_field() && read.result_flags() == 0).then_some(())?;
    let &GlobalRead::NativeMember { native, holder, .. } = known.globals?.get(&(pc as usize))? else { return None };
    (native == crate::vm::Native::StringCharCodeAt || math_kind(native).is_some()).then_some(())?;
    let callee = read.result_register();
    let Some(FieldLookup::Atom { base, .. }) = read.field_lookup() else { return None };
    let receiver = base.register_index()?;
    (callee != receiver).then_some(())?;
    let (j, reads, writes) = between(code, lands, k, plans)?;
    let at = region.header + j as u32;
    let (plain, returning) = code[j].split_return();
    let shape = CallShape { returning, ..CallShape::of(at, plain, CallForm::Plain) };
    // One or two arguments: `row_of` takes exactly the arity the native's operation reads (`Math.sqrt(x)`, `Math.min(i, 777)`).
    (shape.callee == callee && shape.this == receiver && (1..=2).contains(&shape.argc) && j + 1 < code.len() && !region.is_live(at + 1, callee)).then_some(())?;
    (reads.iter().all(|register| !writes.contains(register)) && !writes.contains(&receiver) && !writes.contains(&callee) && !reads.contains(&callee)).then_some(())?;
    let row = Decoded { holder: Some(holder), ..row_of(native, &shape)? };
    let completion = shape.returning.then(|| Decoded { a: Some(Source::Register(shape.result)), ..Decoded::step(Step::Return, None) });
    Some(Fold { read: pc, call: region.header + j as u32, row, completion })
}

/// The instructions between the read at `k` and the call they lead to, at the index `j` of `code`, with the registers the moves
/// among them read and write: only moves and constants, none a jump lands on, and the call a planned plain call.
fn between(code: &[WideInstruction], lands: &Bits, k: usize, plans: &impl Fn(WideInstruction) -> bool) -> Option<(usize, Vec<Register>, Vec<Register>)> {
    let (mut reads, mut writes) = (Vec::new(), Vec::new());
    for j in k + 1..code.len() {
        if lands.contains(j) {
            return None;
        }
        let next = code[j];
        if CallForm::of(next.split_return().0) == Some(CallForm::Plain) {
            return plans(next).then_some((j, reads, writes));
        }
        match next.op().class() {
            Class::Move if next.result_flags() == 0 => reads.push(next.register_b()),
            Class::LoadConst if next.result_flags() == 0 => {}
            _ => return None,
        }
        writes.extend(next.written_registers());
    }
    None
}

/// The Number operation a native Math method computes (`math_call`'s row), if it is one.
pub(super) fn math_kind(native: crate::vm::Native) -> Option<Kind> {
    use crate::vm::Native;
    Some(match native {
        Native::MathSqrt => Kind::Sqrt,
        Native::MathFloor => Kind::Floor,
        Native::MathCeil => Kind::Ceil,
        Native::MathTrunc => Kind::Trunc,
        Native::MathAbs => Kind::Abs,
        Native::MathRound => Kind::Round,
        Native::MathMin => Kind::Min,
        Native::MathMax => Kind::Max,
        Native::MathImul => Kind::Imul,
        Native::MathClz32 => Kind::Clz32,
        _ => return None,
    })
}

/// The row a call of `native` with `shape` runs: `charCodeAt`'s `Heap::CharCodeAt` on the receiver and argument, or a Math
/// method's Number operation on its arguments, if the call takes the arguments it computes.
fn row_of(native: crate::vm::Native, shape: &CallShape) -> Option<Decoded> {
    if native == crate::vm::Native::StringCharCodeAt {
        return (shape.argc == 1).then(|| Decoded::heap(Heap::CharCodeAt, Some(shape.result), Some(Source::Register(shape.this)), Some(Source::Register(shape.window))));
    }
    let kind = math_kind(native)?;
    let binary = matches!(kind, Kind::Min | Kind::Max | Kind::Imul);
    (u16::from(binary) + 1 == shape.argc).then_some(())?;
    let second = binary.then(|| Source::Register(shape.window + 1));
    Some(Decoded::number(kind, Some(shape.result), Some(Source::Register(shape.window)), second, None))
}
