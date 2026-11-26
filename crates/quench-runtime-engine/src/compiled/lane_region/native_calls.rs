//! Calls of natives a body knows by identity: the callee register was defined by a field read
//! of a Stable global whose member was a native function when the body was compiled
//! (`GlobalRead::NativeMember`). The call guards that identity, a fact that may vary per
//! execution, and runs the native's operation inline; any other callee or operand takes the
//! call's helper.
//!
//! A method of a string or an array is known the same way: the callee register was defined by
//! a field read the VM resolved to the realm's native method (`Vm::method_native`), and the call
//! guards the callee's identity at run time, as the receiver varies per execution.
//!
//! A Math operation is SSA code on the call's operands: arguments read as unboxed Numbers (each
//! proved once, leaving at the call where one is not a Number), the result defined as a
//! Float64 (or Int32) register. Nothing is written back, boxed or reloaded: the operation runs
//! no guest code and allocates nothing. A builtin of strings, arrays and `Number.isInteger`
//! (`builtin_calls`) runs its staged row, its result a tagged Boolean, Number or string, or an
//! int32.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::tier_surface::Native;
use quench_runtime::staged::call_native::MathCall;
use quench_runtime::vm::number::math_call;
use quench_runtime::vm::global_facts::GlobalRead;
use super::builtin_calls::Builtin;
use super::data_view_calls::DataViewCall;
use quench_runtime::vm::global_facts::scalar_native;

/// A native a call runs inline: a Math function of Numbers that native code lowers
/// (`MathCall::is_row`), computed by its staged row; or a builtin on its staged row (`Builtin`).
#[derive(Clone, Copy)]
pub(crate) enum Inline {
    Math(MathCall),
    Builtin(Builtin),
    /// A scalar native (`global_facts::scalar_native`), called in place by its helper (`Lowering::scalar_native_call`).
    Scalar,
    /// A DataView getter or setter of a fixed-width element, run on the view's bytes (`data_view_calls`).
    DataView(DataViewCall),
    /// Any other native whose in-place body a loop may call and go on (`dispatch_fast::in_place_in_loop`):
    /// called by the scalar natives' helper with the call's receiver (`Lowering::scalar_native_call`).
    InPlace,
}

/// The argument count of a scalar native's binary form, and of the scalar call that takes two.
const BINARY_ARGUMENTS: u16 = 2;

/// The bit of the scalar natives' helper's count that marks an in-place native's call (`Inline::InPlace`).
pub(crate) const IN_PLACE_CALL: u64 = 1 << 32;

impl Inline {
    /// The operation `native` runs inline, if any: the Math row `number::math_call` derives for
    /// native code, else a builtin's row.
    fn of(native: Native) -> Option<Self> {
        math_call(native).filter(|call| call.is_row()).map(Self::Math)
            .or_else(|| Builtin::of(native).map(Self::Builtin))
            .or_else(|| scalar_native(native).then_some(Self::Scalar))
            .or_else(|| DataViewCall::of(native).map(Self::DataView))
            .or_else(|| crate::dispatch_fast::in_place_in_loop(native).then_some(Self::InPlace))
    }

    /// The arguments the inline operation reads; a call passing another count takes the native.
    const fn arguments(self) -> u16 {
        match self { Self::Scalar | Self::InPlace => BINARY_ARGUMENTS, Self::Math(call) => call.arity(), Self::Builtin(builtin) => builtin.arguments(), Self::DataView(call) => call.required() }
    }

    /// Whether a call passing `count` arguments runs the operation inline: a scalar native takes one or two (`parseInt(s)`,
    /// `parseInt(s, 10)`), every other operation exactly `arguments`.
    const fn accepts(self, count: u16) -> bool {
        match self { Self::Scalar => count == 1 || count == BINARY_ARGUMENTS, Self::InPlace => count <= BINARY_ARGUMENTS, Self::DataView(call) => call.accepts(count), _ => count == self.arguments() }
    }

    /// Whether the operation may change an array's elements, which a loop's hoisted views
    /// and the Stable facts its code folded do not survive.
    const fn mutates_arrays(self) -> bool {
        matches!(self, Self::Builtin(builtin) if builtin.mutates_arrays())
    }


    /// Whether the operation appends a value to an array (`push`).
    const fn appends(self) -> bool {
        matches!(self, Self::Builtin(Builtin::Push))
    }

    /// Whether the operation searches an array's elements (`includes`, `indexOf`): its
    /// receiver's view is hoisted around a loop like a store's (`Lowering::element_search`).
    const fn searches(self) -> bool {
        matches!(self, Self::Builtin(Builtin::Includes | Builtin::IndexOf))
    }

    /// Whether the operation reads a flat string's units at an index (`charCodeAt`): its receiver's
    /// facts a loop holds (`loop_strings`).
    const fn reads_string(self) -> bool {
        matches!(self, Self::Builtin(Builtin::CharCodeAt))
    }

}

/// The inline operation of each call of a unit, by PC (`intrinsic_calls`).
/// Each with the function it held and the exact shape of the prototype holding it.
pub(crate) type Intrinsics = Vec<Option<(Value, Inline, u32)>>;

/// Whether a lane body runs a call of `native` inline, which a loop holds natively (`holds_in_loop`). A loop whose calls are all such runs
/// natively in a body (`loop_body::offer_body`).
pub(crate) fn inlines_in_loop(native: Native) -> bool {
    Inline::of(native).is_some()
}

/// Whether a call of `native` with `count` arguments runs inline in a loop that holds it, the lane's test
/// of `intrinsic_calls` (`accepts`) for the region analysis.
pub(crate) fn inlines_in_loop_call(native: Native, count: u16) -> bool {
    Inline::of(native).is_some_and(|inline| inline.accepts(count))
}

/// Whether the call at `pc` of `decoded` is a method call of a native that `inlines_in_loop_call` runs in place:
/// its callee is the register a field read of the realm's native method defined last before it (as
/// `intrinsic_calls` finds it), which `Vm::method_native` answers.
pub(crate) fn native_method_call(machine: &mut Vm, decoded: &[WideInstruction], pc: usize) -> bool {
    let call = decoded[pc];
    if !quench_runtime::staged::meaning::means(call, quench_runtime::staged::meaning::Meaning::Call) { return false; }
    let callee = call.register_b();
    let Some(definition) = (0..pc).rev().find(|at| decoded[*at].written_registers().any(|register| register == callee)) else { return false };
    let Some(quench_runtime::vm::global_facts::GlobalRead::NativeMember { native, .. }) = machine.method_native(&decoded[definition]) else { return false };
    inlines_in_loop_call(native, quench_runtime::bytecode::ImmediateLayout::argument_count(call.imm()))
}

/// Whether the by-name load at `pc` reads a global name held as a scalar native's slot (`GlobalRead::NativeSlot`) or folded
/// to its value: its read is a slot, a live load that leaves at a miss, or none, and it changes nothing a loop holds.
pub(crate) fn scalar_name_read(global_reads: &quench_runtime::vm::global_facts::GlobalReads, pc: usize) -> bool {
    use quench_runtime::vm::global_facts::GlobalRead;
    // A folded read (`GlobalRead::folded_value`) as `Math` in `Math.imul`.
    global_reads.get(&pc).is_some_and(|read| matches!(read, GlobalRead::NativeSlot { .. }) || read.folded_value().is_some())
}

/// Whether the call at `pc` runs inline and changes no array, so the Stable facts its code
/// folded survive it.
pub(crate) fn keeps_arrays(intrinsics: &Intrinsics, pc: usize) -> bool {
    intrinsics.get(pc).copied().flatten().is_some_and(|(_, inline, _)| !inline.mutates_arrays())
}

/// Whether a loop holds the call at `pc` natively: it runs inline, and one that changes an array
/// (`push`, `pop`) reloads the views it may move or shorten (`Lowering::reload_dense_arrays`).
pub(crate) fn holds_in_loop(intrinsics: &Intrinsics, pc: usize) -> bool {
    intrinsics.get(pc).copied().flatten().is_some()
}

/// Whether the call at `pc` changes an array's length (`push`, `pop`).
pub(crate) fn resizes(intrinsics: &Intrinsics, pc: usize) -> bool {
    intrinsics.get(pc).copied().flatten().is_some_and(|(_, inline, _)| inline.mutates_arrays())
}

/// Whether the call at `pc` appends to an array (`appends`): the array's length may grow.
pub(crate) fn appends(intrinsics: &Intrinsics, pc: usize) -> bool {
    intrinsics.get(pc).copied().flatten().is_some_and(|(_, inline, _)| inline.appends())
}

/// Whether the call at `pc` searches an array's elements (`searches`): its receiver's view is
/// hoisted around the loop that holds the call (`loop_arrays`).
pub(crate) fn searches(intrinsics: &Intrinsics, pc: usize) -> bool {
    intrinsics.get(pc).copied().flatten().is_some_and(|(_, inline, _)| inline.searches())
}

/// Whether the call at `pc` reads a string's units (`reads_string`): its receiver's flat facts are
/// held around the loop that holds the call (`loop_strings`).
pub(crate) fn reads_string(intrinsics: &Intrinsics, pc: usize) -> bool {
    intrinsics.get(pc).copied().flatten().is_some_and(|(_, inline, _)| inline.reads_string())
}

/// The calls of `unit` that run inline, by PC: a call's callee register was defined by a field
/// read that was a native function when the body was compiled (`GlobalRead::NativeMember`: a
/// member of a Stable global, or a method of the string or array prototype; `GlobalRead::NativeSlot`: a
/// global name holding a scalar native), the call passes that function's argument count, and no direct callee
/// is bound to it. Each with the function it held, whose identity the call guards.
pub(crate) fn intrinsic_calls(unit: &super::super::lane_unit::Unit, global_reads: &quench_runtime::vm::global_facts::GlobalReads,
    direct: impl Fn(usize) -> bool) -> Intrinsics {
    unit.instructions.iter().enumerate().map(|(pc, instruction)| Some((pc, instruction)).filter(|(pc, instruction)| quench_runtime::staged::meaning::means(*instruction, quench_runtime::staged::meaning::Meaning::Call)
        && !direct(*pc) && !instruction.returns_from_frame()
        && !instruction.direct_eval() && !instruction.parameter_eval() && instruction.result_flags() == 0)
        .and_then(|(pc, instruction)| {
            let callee = instruction.register_b();
            let defined = (0..pc).rev().find(|at| unit.instructions[*at].written_registers().any(|register| register == callee))?;
            let (value, native, holder) = match global_reads.get(&defined)? {
                GlobalRead::NativeMember { value, native, holder, .. } | GlobalRead::NativeSlot { value, native, holder, .. } => (*value, *native, *holder),
                _ => return None,
            };
            let inline = Inline::of(native)?;
            inline.accepts(quench_runtime::bytecode::ImmediateLayout::argument_count(instruction.imm()))
                .then_some((value, inline, holder))
        })).collect()
}

impl Lowering<'_, '_> {
    /// The call at `pc` where its callee is a known native that runs inline (`intrinsic_calls`),
    /// run as SSA code: guarded once on the callee's identity (the call's interpreter step runs
    /// where it differs) and on each operand its operation reads (a Number, a flat string, a
    /// dense array). False where the call is not one.
    pub(crate) fn intrinsic_call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(&Some((callee, inline, holder))) = self.intrinsics.get(pc) else { return false };
        // A scalar native whose callee identity and arguments the loop's entry proved (`hoist_scalar_calls`): the call's
        // value is the one the entry computed, its callee being the same function on each iteration.
        if let Some(hoisted) = self.scalar_values.get(&pc).copied() && hoisted.fixed {
            let value = self.builder.use_var(hoisted.value);
            self.set_tagged(state, instruction.result_register(), value);
            return true;
        }
        // An element search scans for an int32 word (`element_search`): a search value not proved
        // int32 takes the call's generic path, which compares it by the array's own steps.
        if let Inline::Builtin(Builtin::Includes | Builtin::IndexOf) = inline
            && !state.values[usize::from(quench_runtime::bytecode::ImmediateLayout::call_window_base(instruction.imm()))].value.ty.is_subtype(Type::INT32) { return false; }
        // The callee's identity is held by its holder, a dependency that retires this code
        // before the function can die and its id be reused.
        if !self.holds_holders() { return false; }
        // An in-place native reads its receiver: a register's value, or none for a plain call.
        let receiver = match quench_runtime::staged::Invocation::of(instruction, quench_runtime::staged::DecodeContext::EMPTY).map(|call| call.receiver) {
            _ if !matches!(inline, Inline::InPlace) => None,
            Some(quench_runtime::staged::Receiver::Register(register)) => Some(register),
            Some(quench_runtime::staged::Receiver::Undefined) => None,
            _ => return false,
        };
        let exit = self.guard_exit_for(state, pc as u32, ExitFact::Callee);
        let held = self.tagged(state, instruction.register_b());
        // An in-place native's helper checks its callee itself, so a name several prototypes hold (`has`) takes each.
        if !matches!(inline, Inline::InPlace) {
            self.hold_holder_any(state, exit, holder);
            let same = self.is_identity(held, callee);
            emit_js_guard(&mut self.builder, same, exit);
        }
        match inline {
            Inline::Builtin(builtin) => self.builtin_call(state, pc, instruction, builtin, exit),
            Inline::Scalar => self.scalar_native_call(state, pc, instruction, held, None, exit),
            Inline::InPlace => self.scalar_native_call(state, pc, instruction, held, Some(receiver), exit),
            Inline::Math(call) => self.math_call(state, instruction, call, exit),
            Inline::DataView(call) => self.data_view_call(state, instruction, call, exit),
        }
        true
    }

    /// The scalar native (`Inline::Scalar`) whose callee `held` the call guarded: its helper runs the native's in-place
    /// case on the arguments (`lane_allocations::scalar_native_in_lane`), which reads the native from the callee. The call
    /// leaves at `exit`, before it changes anything, where that case does not complete (an object argument, an error, a
    /// due collection).
    fn scalar_native_call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, held: ir::Value, receiver: Option<Option<Register>>, exit: ir::Block) {
        // The value the loop's entry computed (`hoist_scalar_calls`), where the callee's identity guard at this call holds.
        if let Some(hoisted) = self.scalar_values.get(&pc).copied() {
            let value = self.builder.use_var(hoisted.value);
            self.set_tagged(state, instruction.result_register(), value);
            return;
        }
        let argc = quench_runtime::bytecode::ImmediateLayout::argument_count(instruction.imm());
        let window = quench_runtime::bytecode::ImmediateLayout::call_window_base(instruction.imm());
        let undefined = self.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
        let count = self.raw_const(Lane::I64, i64::from(argc));
        let first = if argc >= 1 { self.tagged(state, window) } else { undefined };
        let second = if argc == 2 { self.tagged(state, window + 1) } else { undefined };
        // An in-place native's receiver (`Inline::InPlace`), and its flag to the helper in the count's high bit.
        let (this, count) = match receiver {
            Some(register) => {
                let this = register.map_or(undefined, |register| self.tagged(state, register));
                (this, self.raw_const(Lane::I64, i64::from(argc) | IN_PLACE_CALL as i64))
            }
            None => (undefined, count),
        };
        let value = self.helper_call(LaneHelper::ScalarNative, &[self.payload, held, count, this, first, second]);
        let done = self.raw_compare_imm(Relation::NotEqual, value, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, done, exit);
        // A Number or other primitive argument may allocate its text, which a body's proofs of object layouts do not survive.
        self.allocated(state);
        self.set_tagged(state, instruction.result_register(), value);
    }

    /// The Math call `call` of Numbers, its result an unboxed Number (an int32 for `Math.imul` and `Math.clz32`).
    fn math_call(&mut self, state: &mut State, instruction: WideInstruction, call: MathCall, exit: ir::Block) {
        let window = quench_runtime::bytecode::ImmediateLayout::call_window_base(instruction.imm());
        let int32 = quench_runtime::staged::call_native::math_operands(call) == quench_runtime::staged::numeric::Domain::Int32;
        let operands: Vec<ir::Value> = (window..window + call.arity()).map(|register| {
            let number = self.number(state, register, exit);
            if int32 { self.number_to_int32(number, exit) } else { number }
        }).collect();
        // The row's one definition (`call_native::math_of_operands`), which the interpreters' value-word step runs.
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        let (domain, word) = quench_runtime::staged::call_native::math_of_operands(&mut stage, call, &operands).expect("an inline Math call is a staged row");
        match domain {
            quench_runtime::staged::numeric::Domain::Int32 => self.set_int(state, instruction.result_register(), word),
            _ => self.set_number(state, instruction.result_register(), word),
        }
    }
}

/// A scalar native call a loop's entry computed once (`Lowering::hoist_scalar_calls`): the variable holding its result, and
/// whether its callee register is no write of the loop's, so the entry's identity guard holds on every iteration (`fixed`).
/// Else the call keeps its identity guard per iteration, and takes the held result where that guard passes.
#[derive(Clone, Copy)]
pub(crate) struct HoistedScalar {
    pub(crate) value: Variable,
    pub(crate) fixed: bool,
}

/// An argument of a hoisted scalar call, as the loop's entry reads it: a register no instruction of the loop writes, or a
/// constant its loading instruction names.
#[derive(Clone, Copy)]
enum ScalarArgument {
    Register(Register),
    Constant(Value),
}

impl Lowering<'_, '_> {
    /// The scalar native calls of the loop at `header` whose arguments the loop does not write, computed once on its entering
    /// edge (`HoistedScalar`), so an iteration takes their result instead of the helper call. Each call is one the entry
    /// can run before the loop: a pure native on primitive arguments, whose exit (an object argument, an error, a due
    /// collection) leaves at the header before the loop ran, as each iteration would have left at the call.
    pub(crate) fn hoist_scalar_calls(&mut self, state: &mut State, header: usize) {
        if self.generic.contains(&header) || !self.holds_holders() { return; }
        let Some(end) = self.loop_end(header) else { return };
        if !self.enters_only_at_header(header, end) { return; }
        let calls: Vec<usize> = (header..=end).filter(|&pc| matches!(self.intrinsics.get(pc), Some(Some((_, Inline::Scalar, _))))
            && !self.scalar_values.contains_key(&pc) && !self.hoisted.contains_key(&pc) && !self.generic.contains(&pc)).collect();
        for pc in calls {
            self.hoist_scalar_call(state, header, end, pc);
        }
    }

    /// The scalar native call at `pc` in the loop `header..=end` (`hoist_scalar_calls`), where its callee is held and its
    /// arguments are loop-invariant; else nothing is hoisted.
    fn hoist_scalar_call(&mut self, state: &mut State, header: usize, end: usize, pc: usize) -> Option<()> {
        let instruction = self.unit.instructions[pc];
        let argc = quench_runtime::bytecode::ImmediateLayout::argument_count(instruction.imm());
        let window = quench_runtime::bytecode::ImmediateLayout::call_window_base(instruction.imm());
        let callee = instruction.register_b();
        let Some(Some((identity, _, holder))) = self.intrinsics.get(pc).copied() else { return None };
        if argc == 0 { return None; }
        let writes = |at: usize| self.unit.instructions[at].written_registers().any(|written| written == callee);
        let loop_writes: Vec<usize> = (header..=end).filter(|&at| writes(at)).collect();
        let defined = (0..pc).rev().find(|&at| writes(at))?;
        // A callee the loop writes is its one by-name read of the native (`defined`), read on the entering edge: else the
        // callee is no loop write and the entry's guard holds throughout.
        let fixed = loop_writes.is_empty();
        if !fixed && (loop_writes != [defined] || defined < header || !matches!(self.global_reads.get(&defined), Some(GlobalRead::NativeSlot { .. }))) {
            return None;
        }
        let arguments: Vec<ScalarArgument> = (0..argc).map(|index| self.scalar_argument(header, end, pc, window + index)).collect::<Option<_>>()?;
        let exit = self.guard_exit_for(state, header as u32, ExitFact::Callee);
        self.hold_holder_any(state, exit, holder);
        let entry = if fixed { self.tagged(state, callee) } else { self.hoist_native_read(state, header, defined)? };
        let same = self.is_identity(entry, identity);
        emit_js_guard(&mut self.builder, same, exit);
        let mut values = Vec::with_capacity(usize::from(argc));
        for argument in arguments {
            values.push(match argument {
                ScalarArgument::Register(register) => self.tagged(state, register),
                ScalarArgument::Constant(value) => self.raw_const(Lane::I64, value.to_bits() as i64),
            });
        }
        let undefined = self.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
        let count = self.raw_const(Lane::I64, i64::from(argc));
        let second = if argc == 2 { values[1] } else { undefined };
        let value = self.helper_call(LaneHelper::ScalarNative, &[self.payload, entry, count, undefined, values[0], second]);
        let done = self.raw_compare_imm(Relation::NotEqual, value, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, done, exit);
        // As the call does in the loop, a primitive argument may allocate its text.
        self.allocated(state);
        let variable = self.builder.declare_var(ir::types::I64);
        self.builder.def_var(variable, value);
        self.scalar_values.insert(pc, HoistedScalar { value: variable, fixed });
        Some(())
    }

    /// The argument the window register `register` holds at the call at `pc` in the loop `header..=end`: the source of the
    /// one copy or constant load that writes it, in the call's own block, where the loop writes that source nowhere.
    fn scalar_argument(&self, header: usize, end: usize, pc: usize, register: Register) -> Option<ScalarArgument> {
        let at = (header..pc).rev().find(|&at| self.unit.instructions[at].written_registers().any(|written| written == register))?;
        if ((at + 1)..pc).any(|later| self.blocks.get(later).is_some_and(|block| block.is_some())) { return None; }
        let instruction = self.unit.instructions[at];
        if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst) {
            let constant = self.program.constants.get(instruction.constant_index())?;
            return super::super::facts::immediate(constant).map(ScalarArgument::Constant);
        }
        let Some(quench_runtime::staged::Operation::<(), ()>::Move { source, .. }) = quench_runtime::staged::Operation::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY) else { return None };
        (header..=end).all(|at| !self.unit.instructions[at].written_registers().any(|written| written == source)).then_some(ScalarArgument::Register(source))
    }
}
