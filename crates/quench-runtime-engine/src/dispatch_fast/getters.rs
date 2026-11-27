//! Reads that meet an accessor: answered without an activation where the getter's call runs
//! no guest code. The answers and activations are `AccessorCall`'s (`accessor_protocol.rs`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::vm::AccessorForward;
use quench_runtime::vm::accessor_cache::GetterEntryRead;
use quench_runtime::staged::call_getter::{step_getter_call_of, step_getter_invoke_of, GetterAnswer, GetterCall, GetterKind};
use quench_runtime::staged::{Concrete, Step};
use super::lane_frame::LaneControl;

/// The lane's primitives of a getter's answer without its call (`call_getter::step_getter_answer_of`),
/// memoized at the field cache site `memo` names.
#[derive(Clone, Copy)]
pub(crate) struct LaneGetterAnswer<'a> {
    pub(crate) cx: *mut LaneContext,
    pub(crate) memo: GetterMemo<'a>,
}

/// Where a lane getter's answer is memoized: nowhere, a site operand, or a read record's site
/// unless the read is nested (read only where an answer is memoized).
#[derive(Clone, Copy)]
pub(crate) enum GetterMemo<'a> {
    Off,
    Site(u16),
    Read(&'a LaneInstruction),
}

impl GetterAnswer<Concrete> for LaneGetterAnswer<'_> {
    type Target = ScriptCallTarget;
    type Value = Value;

    #[inline(always)]
    fn kind(&mut self, _: &mut Concrete, getter: Value) -> GetterKind {
        match unsafe { vm(self.cx) }.function_record(getter) {
            Some(FunctionRecord { kind: FunctionKind::Native(_), .. }) => GetterKind::Native,
            _ => GetterKind::Script,
        }
    }

    #[inline(always)]
    fn target(&mut self, _: &mut Concrete, getter: Value) -> Option<ScriptCallTarget> {
        script_target_in_lane(self.cx, getter)
    }

    #[inline(always)]
    fn pure_native(&mut self, _: &mut Concrete, getter: Value, base: Value) -> Option<Value> {
        unsafe { vm(self.cx) }.pure_native_getter_value(getter, base)
    }

    /// Where a read answers the call (`CallEntry::answered_by_read`).
    #[inline(always)]
    fn read_answer(&mut self, _: &mut Concrete, target: &ScriptCallTarget, base: Value) -> Option<Value> {
        let cx = self.cx;
        if target.entry == CallEntry::Reads {
            // A getter only the general path has called has not run its read in the lane yet: it
            // takes the kind its first run would.
            // SAFETY: as `call_read_in_lane`; the context's program outlives the lane run.
            let (entry, code) = unsafe { ((*target.view).records(), &*(*cx).code) };
            if unsafe { (*entry).handler } == row_returning_field_first::<NOT_LEAF> as *const () {
                requicken(entry, returning_field_first_kind::<NOT_LEAF>(unsafe { vm(cx) }, code, unsafe { &*entry }));
            }
        // SAFETY: a cached target of the running program.
        } else if let Some(value) = unsafe { constant_body_value(target) } {
            // `return k`: no receiver to bind, nothing to read.
            return Some(value);
        } else if !target.entry.answered_by_read() {
            return None;
        }
        let this = direct_call_this_in_lane(unsafe { vm(cx) }, target, base)?;
        // SAFETY: the running lane's context, a frameless target of its program.
        unsafe { call_read_in_lane(cx, target, ReadArguments::NONE, this) }
    }

    /// Memoizes at the site's accessor entry the forward that proves `base` what the getter
    /// returns for receivers of its layout (`Vm::memoize_getter_answer`, `getter_forward`); a
    /// capture read (`() => binding`) has none.
    #[inline(always)]
    fn memoize(&mut self, _: &mut Concrete, target: &ScriptCallTarget, getter: Value, base: Value) {
        let site = match self.memo {
            _ if target.entry == CallEntry::ReadsCapture => return,
            GetterMemo::Site(site) => site,
            GetterMemo::Read(i) if i.b != quench_runtime::bytecode::FieldBase::NESTED => i.c,
            _ => return,
        };
        memoize_getter(unsafe { vm(self.cx) }, site, getter, base);
    }
}

/// `LaneGetterAnswer::memoize` at the site operand `site`, out of line.
#[inline(never)]
fn memoize_getter(vm: &mut Vm, site: u16, getter: Value, base: Value) {
    let site = vm.field_cache_index(site);
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects. The
    // entry is a cached target of the running program.
    if let Some(entry) = vm.script_call_entry(getter).map(|entry| unsafe { *entry })
        && let Some(answer) = unsafe { getter_forward(vm, &entry, base) }
    {
        vm.memoize_getter_answer(base, site, getter, answer);
    }
}

/// What the getter `target` returns for `object` and every receiver of its layout, as a
/// forward (getters-as-calls.md, trivial accessors), recognized from its body:
/// - `return k` of a primitive constant that is no heap value (`constant_body_value`);
/// - `return this` (`CallEntry::ReadsThis`) of a getter that binds `object` itself;
/// - `return this.f` (`CallEntry::Reads`) whose returning `GetField` of `this` hits an own or
///   holder entry: that entry, which proves the receiver's shape itself;
/// - `return this.#f`: the private field's own slot its site recorded for `object`'s exact
///   shape under the getter's home (`Vm::site_private_field_entry`): the getter's identity
///   fixes its environment and so its home, and the shape fixes where the field is.
///
/// None of these can throw or run guest code for a receiver the forward admits: the data
/// field reads have no getter to call, so the call's absence is not observable.
///
/// # Safety
/// `target` is a cached target of the running program.
unsafe fn getter_forward(vm: &Vm, target: &ScriptCallTarget, object: Value) -> Option<AccessorForward> {
    if let Some(value) = unsafe { constant_body_value(target) } {
        return (!value.is_heap()).then_some(AccessorForward::Constant(value));
    }
    if direct_call_this_in_lane(vm, target, object) != Some(object) {
        return None;
    }
    match target.entry {
        CallEntry::ReadsThis => Some(AccessorForward::Receiver),
        // SAFETY: a `Reads` target of the running program holds its returning record first.
        CallEntry::Reads => match unsafe { LeafRead::of(target) }? {
            LeafRead::Field { base, kind: RETURNING_FIELD_OWN | RETURNING_FIELD_HOLDER, cache } if base == quench_runtime::bytecode::FieldBase::THIS.0 => {
                Some(AccessorForward::Field(vm.field_cache_entry(cache)))
            }
            LeafRead::Private { base, site, environment, .. } if base == quench_runtime::bytecode::FieldBase::THIS.0 => {
                let answer = vm.site_private_field_entry(site, environment)?;
                let receiver = vm.heap_reads().object_record(object)?;
                (answer.receiver == receiver.exact_shape()).then_some(AccessorForward::Field(answer))
            }
            _ => None,
        },
        _ => None,
    }
}

/// The constant the body of `target` returns where it is `LoadConst` and `Return` of it
/// (`specialize_constant_returns`'s `return_constant_miss` row, a plain leaf's form included):
/// a call of it is that constant, whatever its receiver and arguments.
///
/// # Safety
/// `target` is a cached target of the running program.
#[inline(always)]
unsafe fn constant_body_value(target: &ScriptCallTarget) -> Option<Value> {
    if !target.entry.activates() {
        return None;
    }
    // SAFETY: the program store keeps a derived view alive with its program; its first
    // record is the body's first instruction.
    let first = unsafe { &*(*target.view).records() };
    let constant = first.handler == return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(CONSTANT_KIND, NOT_LEAF)) as Handler as *const ()
        || first.handler == return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(CONSTANT_KIND, PLAIN_LEAF)) as Handler as *const ();
    constant.then(|| first.constant())
}

/// The lane's primitives of a getter's call for the read record `i` at `ip` (`step_getter_call_of`).
struct LaneGetter<'a> {
    answer: LaneGetterAnswer<'a>,
    r: Registers,
    ip: Ip,
    i: &'a LaneInstruction,
}

impl<'a> GetterCall<Concrete> for LaneGetter<'a> {
    type Answer = LaneGetterAnswer<'a>;
    type Out = Result<(Registers, Ip), Ip>;
    #[inline(always)]
    fn answers(&mut self) -> &mut LaneGetterAnswer<'a> {
        &mut self.answer
    }

    /// The registers are the context's again: the answer's read may have made the activations frames.
    #[inline(always)]
    fn answer(&mut self, _: &mut Concrete, value: Value) -> Self::Out {
        let r = unsafe { (*self.answer.cx).registers };
        r.set(self.i.a, value);
        Ok((r, following(self.ip)))
    }

    #[inline(always)]
    fn call_native(&mut self, _: &mut Concrete, getter: Value, base: Value) -> Self::Out {
        call_native_getter(self.answer.cx, self.ip, self.i, getter, base)
    }

    #[inline(always)]
    fn call(&mut self, _: &mut Concrete, getter: Value, base: Value, target: Option<ScriptCallTarget>) -> Self::Out {
        activate_getter_in_lane(self.answer.cx, self.r, self.ip, getter, base, target)
    }
}

/// A lane read of `base` at the record `i` (at `ip`) that met `getter`: the staged getter call
/// (`call_getter::step_getter_call_of`). Only a read that writes its register is answered in
/// place: a returning read (`RETURN_REGISTER`) completes its activation, which only the getter's
/// activation does. `Err` after a native call that raised leaves the error in `Vm::lane_raised`.
/// A nested read's answer is not memoized.
pub(crate) fn lane_read_getter(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    getter: Value,
    base: Value,
) -> Result<(Registers, Ip), Ip> {
    let writes = i.a & quench_runtime::bytecode::RETURN_REGISTER == 0;
    let mut call = LaneGetter { answer: LaneGetterAnswer { cx, memo: GetterMemo::Read(i) }, r, ip, i };
    step_getter_call_of(&mut Concrete::pure(), &mut call, getter, base, writes)
}

/// `LaneGetter::call_native`: the native getter called as a lane `Call` of it with no arguments;
/// out of line, so script getters keep their code.
#[cold]
#[inline(never)]
fn call_native_getter(cx: *mut LaneContext, ip: Ip, i: &LaneInstruction, getter: Value, base: Value) -> Result<(Registers, Ip), Ip> {
    let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = unsafe { vm(cx) }.function_record(getter) else {
        unreachable!("`kind` named the getter native");
    };
    match native_complete_in_lane(cx, ip, i, call_native_in_lane(cx, ip, *native, getter, base, &[])) {
        Resumed::At(r, next, _) => Ok((r, next)),
        Resumed::Exit(next) => Err(next),
    }
}

/// What a getter site's `GetField` record does after one walk of its site's accessor entries
/// (`lane_getter_step`).
pub(crate) enum GetterStep {
    /// A trivial getter's forward (`Vm::getter_entry_read`): the read's value, no call.
    Answer(Value),
    /// The getter's read or call (`lane_read_getter`), found without the fact table's probe
    /// and the second walk `getter_activation_in_lane` makes.
    Read(Result<(Registers, Ip), Ip>),
    /// No entry proves the base: `getter_activation_in_lane`'s lookups decide.
    Miss,
}

/// `lane_get_field_getter`'s step for a `GetField` record `i` (at `ip`) of a register or
/// `this`: the entry of the base's layout gives the forward, which only a read that writes its
/// register takes in place (a returning read completes its activation, which only the call
/// does), or the getter, which a lane allowed script calls reads or calls as
/// `getter_activation_in_lane` would. A nested read, and any base no entry proves, is `Miss`.
#[inline(always)]
pub(crate) fn lane_getter_step(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> GetterStep {
    if i.b == quench_runtime::bytecode::FieldBase::NESTED {
        return GetterStep::Miss;
    }
    // SAFETY: the running lane's context and registers.
    let Some(base) = (unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) }) else {
        return GetterStep::Miss;
    };
    let vm = unsafe { vm(cx) };
    let reads = i.a & quench_runtime::bytecode::RETURN_REGISTER == 0;
    match vm.getter_entry_read(base, vm.field_cache_index(i.c)) {
        GetterEntryRead::Answer(value, _) if reads => GetterStep::Answer(value),
        // `getter_activation_in_lane` declines a lane without script calls before any lookup.
        GetterEntryRead::Answer(_, getter) | GetterEntryRead::Call(getter) if unsafe { (*cx).script_calls } => {
            GetterStep::Read(lane_read_getter(cx, r, ip, i, getter, base))
        }
        _ => GetterStep::Miss,
    }
}

/// A script getter's call through the one call row (`step_getter_invoke_of`, getters-as-calls.md) over
/// the read's frame (`lane_frame::getter_frame`): `target` the read resolved, `base` bound as any call's
/// receiver. `None`, which changes nothing, for a call that is not plain: the kept activation takes it.
#[inline(always)]
pub(crate) fn invoke_getter(cx: *mut LaneContext, r: Registers, ip: Ip, target: &ScriptCallTarget, base: Value) -> Option<(Registers, Ip)> {
    let function = u16::try_from(target.function).ok()?;
    let mut frame = super::lane_frame::getter_frame(cx, r, ip, target, base, getter_activation);
    // SAFETY: `ip` is the read record of the running view.
    let result = unsafe { (*ip).a };
    match step_getter_invoke_of(&mut Concrete::pure(), &mut frame, (), function, result) {
        Step::Control(LaneControl::Resumed(Resumed::At(registers, next, _))) => Some((registers, next)),
        _ => None,
    }
}

/// The getter's hand-off past the frame's admission (`CallActivation`): a receiver-binding leaf runs as a
/// leaf (`leaf_getter`), else the plain header is pushed at `depth` with no values. Its entry is counted.
///
/// # Safety
/// As `plain_call_activation`, `record` the read record at `ip`.
#[inline(always)]
unsafe fn getter_activation(context: *mut (), r: Registers, ip: Ip, _record: *const LaneInstruction, target: *const ScriptCallTarget,
    depth: usize, this: Value) -> Resumed {
    let cx = context.cast::<LaneContext>();
    let (vm, target) = unsafe { (vm(cx), &*target) };
    vm.programs.record_js_jit_lane_entry(target.program(), target.function);
    if leaf_binds_this(target.entry.leaf_form())
        && let Some((registers, base)) = unsafe { leaf_getter(cx, r, ip, target, this) }
    {
        return Resumed::At(registers, base, Carried::script(cx));
    }
    // No values: every parameter `undefined` (a dangling, aligned pointer, never read for a count of 0).
    let none: &[Value] = &[];
    let callee_registers = fill_window_in_lane(vm, target, none.as_ptr(), 0);
    let (registers, base) = enter_plain_window(cx, vm, r, ip, target, callee_registers, depth, 0, this, false);
    Resumed::At(registers, base, Carried::script(cx))
}

/// A receiver-binding leaf getter `target` (`THIS_LEAF`, `CONTEXT_LEAF`) run as a leaf call with no
/// values, the read record at `ip` its call: its return writes the record's `a` and resumes at
/// `following(ip)`, as the plain return does (the other forms rebind `this` from the record's `c`, a
/// `GetField`'s site). `None` where the cache does not hold the target's entry in place (the leaf names
/// its cache entry, which must stay put while it runs).
///
/// # Safety
/// `cx` is the running lane's context, with no leaf running; `target` passed the frame's plain
/// admission (running program, no `with` scope, room by `plain_room`); `this` is its binding.
#[inline(always)]
unsafe fn leaf_getter(cx: *mut LaneContext, r: Registers, ip: Ip, target: &ScriptCallTarget, this: Value) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    debug_assert!(unsafe { (*cx).leaf.call }.is_null(), "a getter's read materializes a running leaf first");
    let form = target.entry.leaf_form();
    // SAFETY: as `Vm::script_call_entry`; read in place before anything touches the cache, and
    // nothing in the leaf resolves a target or collects.
    let entry = unsafe { &*vm.script_call_entry(target.callee)? };
    if entry.entry != target.entry || entry.function != target.function || !std::ptr::eq(entry.view, target.view) {
        return None;
    }
    // SAFETY: as `lane_run_leaf`'s window: `plain_room` proved the admitted room, which the
    // stack's length covers only once the leaf is materialized; no collection runs before.
    unsafe {
        let window = Registers(vm.lane_stack.values.top());
        Some(lane_enter_leaf(cx, AdmittedWindow::of(entry, std::ptr::null(), 0), window, ip, r, entry, form, 0, this, None, false))
    }
}

script_handler! {
    /// The generated getter read (`GETTER_THIS`, `GETTER_REGISTER`) past its hint: the read the
    /// getter's answer makes (`step_getter_answer_of`), whose getter's own field becomes the
    /// hint (`AccessorCall::hold_getter_hint`). A getter the answer declines takes `get_field_getter_miss`.
    #[inline(never)]
    pub(crate) fn row_get_field_getter_refresh<>(cx, r, ip, i, view) {
        let base = unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) };
        let Some(value) = base.and_then(|base| unsafe { AccessorCall::of(cx).hold_getter_hint(ip, i, base) }) else {
            requicken(ip, protocol_handler::<{ quench_runtime::bytecode::FIELD_READ as u16 }, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::GET_FIELD_GETTER as u16 }>());
            sibling!(get_field_getter_miss, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The `FieldOperand::shape` of a record whose hint is a getter's own-field return
/// (`Vm::getter_hint_operands`), as `HOLDER_HINT` no object's exact shape.
const GETTER_HINT: u32 = quench_runtime::vm::NO_FIELD_RECEIVER - 3;

/// Makes the own field `slot` of `base`, which the record's getter returns, the hint of the
/// `GetField` record at `ip`, in place of the one it holds; whether the record holds it.
pub(crate) fn set_getter_hint(vm: &mut Vm, ip: Ip, atom: Atom, base: Value, slot: u16) -> bool {
    if holds_loop_site(ip) {
        return false;
    }
    // SAFETY: `ip` is a `GetField` record of a live view.
    let i = unsafe { &*ip };
    let owned = (unsafe { i.operand.field.shape } == GETTER_HINT).then(|| i.call_link.get());
    let Some(index) = vm.hold_getter_hint(owned, atom, base, slot, NO_CALL_LINK) else {
        return false;
    };
    set_record_hint(ip, GETTER_HINT, index);
    true
}
