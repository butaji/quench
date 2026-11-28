//! The call a lane body's helper runs, read through its decoded form (`staged::Invocation`), never
//! through the record layout of one opcode: where its callee, receiver and arguments are is data
//! the decode gave, so a register call, a known call, a method call and a construction run the
//! same helper. The running activation's registers, `this` and function come from the running
//! body, which a native callee's frame makes the innermost (`native_frames::in_innermost_frame`).
#[allow(unused_imports)] use quench_runtime::staged::{InvocationFormsExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::field_cache::FieldCacheProbe;
use super::*;
use quench_runtime::bytecode::Register;
use quench_runtime::staged::protocol::fill_call;
use quench_runtime::staged::{Arguments, Callee, Concrete, DecodeContext, FunctionId, Invocation, Passed, Receiver};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// Values a call gathers from its register list, kept inline up to what a call usually passes.
pub(crate) type Gathered = SmallVec<[Value; GATHERED_INLINE_VALUES]>;

/// The values a call passes: a window's, in place, or a list's, gathered.
pub(crate) enum Values<'a> {
    Window(&'a [Value]),
    Gathered(Gathered),
}

impl std::ops::Deref for Values<'_> {
    type Target = [Value];

    fn deref(&self) -> &[Value] {
        match self {
            Self::Window(values) => values,
            Self::Gathered(values) => values,
        }
    }
}

/// A call record of the running body, decoded once.
#[derive(Clone, Copy)]
pub(crate) struct BodyCall<'a> {
    pub(crate) site: Invocation,
    pub(crate) registers: Registers,
    pub(crate) ip: Ip,
    /// The running activation's `this`; the deleted marker where the body reads a lexical one.
    this: Value,
    code: &'a ResidualProgram,
    /// The registers of a method call's argument list; none for a window.
    list: &'a [Register],
}

impl<'a> BodyCall<'a> {
    /// The call at `call_pc`, a record of the running body's function, with the running
    /// activation's registers and `this`. `None` for a form no row runs (`Invocation::of`).
    ///
    /// # Safety
    /// `cx` is the running lane's context and `call_pc` a record of the running body's function.
    #[inline(always)]
    pub(crate) unsafe fn at(cx: *mut LaneContext, call_pc: u64) -> Option<Self> {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        // SAFETY: the context names the running activation, which a native callee's frame is while its helper
        // runs (`native_frames::in_innermost_frame`).
        let (registers, base, function, this) = unsafe { ((*cx).registers, (*cx).base, running_function_in_lane(cx), (*cx).registers.this()) };
        // The form of this record, decoded once per run: the run's code fixes it.
        let site = match unsafe { (*cx).body_site } {
            Some((decoded_code, decoded_function, decoded_pc, site))
                if std::ptr::eq(decoded_code, code) && decoded_function == function && decoded_pc == call_pc => site,
            _ => {
                let body = code.function(function as usize);
                let site = Invocation::of(body.widened(body.code[call_pc as usize]), DecodeContext::of(code))?;
                unsafe { (*cx).body_site = Some((std::ptr::from_ref(code), function, call_pc, site)) };
                site
            }
        };
        let list = match site.arguments {
            Arguments::List(list) => list.registers(&code.method_arguments),
            Arguments::Window { .. } => &[],
        };
        // SAFETY: `call_pc` is a record of the function whose records start at `base`.
        Some(Self { site, registers, ip: unsafe { base.add(call_pc as usize) }, this, code, list })
    }

    /// How many arguments the call passes.
    #[inline]
    pub(crate) fn argc(&self) -> u16 {
        self.site.arguments.count()
    }

    /// The record's hint of the script call target that answered it last.
    #[inline]
    pub(crate) fn link(&self) -> &'a std::cell::Cell<u16> {
        // SAFETY: `ip` is a record of the running view.
        unsafe { &(*self.ip).call_link }
    }

    /// The receiver the call binds: a register, the activation's `this` or the field of it a cached
    /// read gives, or `undefined`. `None` where the helper cannot read it in place: a lexical `this`,
    /// a field the caches do not hold, or a construction's receiver, which its plan allocates.
    pub(crate) fn receiver(&self, vm: &mut Vm) -> Option<Value> {
        match self.site.receiver {
            Receiver::Register(register) => Some(self.registers.get(register)),
            Receiver::Undefined => Some(Value::UNDEFINED),
            Receiver::This { path } => {
                let this = Some(self.this).filter(|this| !this.is_deleted())?;
                let Some((_, cache)) = path else { return Some(this) };
                match vm.cached_field_hit(self.code, this, cache) {
                    FieldCacheProbe::Hit(value) => Some(value),
                    FieldCacheProbe::OrdinaryMiss | FieldCacheProbe::Ineligible => None,
                }
            }
            Receiver::Allocated | Receiver::Base => None,
        }
    }

    /// The callee: a register's value, a method the site's caches load from `receiver`
    /// (`Vm::method_cache_load`), or the function a known call names, which its site linked. `None`
    /// where the method is not cached or the site linked no such function.
    #[inline(always)]
    pub(crate) fn callee(&self, vm: &mut Vm, receiver: Value) -> Option<Value> {
        match self.site.callee {
            Callee::Register(register) => Some(self.registers.get(register)),
            Callee::Method { atom, site } => vm.method_cache_load(self.code, site as usize, atom, receiver),
            Callee::Known(function) => known_callee(vm, self.link(), function),
        }
    }

    /// Parameter `k` of a plain activation as this call passes it: argument `k`, or `undefined`
    /// where it is not passed.
    #[inline(always)]
    pub(crate) fn argument(&self) -> impl Fn(u16) -> Value + '_ {
        #[inline(always)] move |parameter| match self.site.arguments {
            Arguments::Window { first, count } if parameter < count => self.registers.get(first + parameter),
            Arguments::Window { .. } => Value::UNDEFINED,
            Arguments::List(_) => self.list.get(usize::from(parameter)).map_or(Value::UNDEFINED, |&register| self.registers.get(register)),
        }
    }

    /// The values the call passes.
    ///
    /// # Safety
    /// The caller's registers do not move while the values are read.
    pub(crate) unsafe fn values(&self) -> Values<'a> {
        match self.site.arguments {
            // SAFETY: the window is registers of the running function.
            Arguments::Window { first, count } => Values::Window(unsafe { std::slice::from_raw_parts(self.registers.0.add(usize::from(first)), usize::from(count)) }),
            Arguments::List(_) => Values::Gathered(Gathered::from_iter(self.list.iter().map(|&register| self.registers.get(register)))),
        }
    }

    /// The arguments as the keeping activation takes them: the window in place, or the values of a
    /// list gathered into `gathered`, which then outlives the activation.
    pub(crate) fn kept(&self, gathered: &mut Gathered) -> ActivationArguments {
        match self.site.arguments {
            Arguments::Window { first, .. } => ActivationArguments::Window(first),
            Arguments::List(_) => {
                gathered.extend(self.list.iter().map(|&register| self.registers.get(register)));
                ActivationArguments::Kept(gathered.as_ptr(), gathered.len())
            }
        }
    }

    /// The plain activation of `target` this call makes: its window of registers filled with the
    /// arguments, then `undefined`. The call passes at most `target`'s parameters and the lane stack
    /// has room (`activation_room_in_lane`).
    pub(crate) fn push_plain(&self, cx: *mut LaneContext, target: &ScriptCallTarget, this: Value, constructs: bool) -> Registers {
        match self.site.arguments {
            Arguments::Window { first, count } => {
                // SAFETY: the record's window of the registers holds its values.
                let passed = unsafe { self.registers.0.add(usize::from(first)).cast_const() };
                push_passed_in_lane::<false, false>(cx, self.registers, self.ip, target, passed, count, this, constructs).0
            }
            Arguments::List(_) => self.push_listed(cx, target, this, constructs),
        }
    }

    /// `push_plain` of a list: the registers it names are read in the window's fill
    /// (`staged::protocol::fill_call`, the row the native tiers emit).
    fn push_listed(&self, cx: *mut LaneContext, target: &ScriptCallTarget, this: Value, constructs: bool) -> Registers {
        let stack = &mut unsafe { vm(cx) }.lane_stack;
        // SAFETY: capacity holds the window's admitted room (`ScriptCallTarget::room`); the listed
        // registers are the caller's, outside it; each slot is written before the length covers it.
        let window = unsafe {
            let top = stack.values.top();
            let listed = Passed::List { file: self.registers.0 as u64, registers: self.list };
            fill_call(&mut Concrete::pure(), top as u64, &[], listed, target.registers);
            stack.values.set_top(top.add(usize::from(target.registers) + quench_runtime::vm::lane_values::CONTEXT));
            Registers(top)
        };
        enter_window_in_lane::<false, false>(cx, self.registers, self.ip, target, window, self.argc(), this, constructs, None).0
    }
}

/// The function a known call names (`Callee::Known`), the callee its site linked: the cached
/// entry the record's hint names, where that is a function of the running program.
fn known_callee(vm: &Vm, link: &std::cell::Cell<u16>, function: FunctionId) -> Option<Value> {
    let entry = vm.lane_stack.script_targets.entries.get(usize::from(link.get()))?;
    (entry.function == u32::from(function) && entry.program() == vm.active_program).then_some(entry.callee)
}
