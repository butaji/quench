use super::program_store::ProgramId;
use crate::Value;
use std::collections::VecDeque;

/// The only state that crosses an activation boundary. The value is kept as a
/// child of the suspension's owner and is consumed by the resumer according
/// to its completion kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Completion {
    GeneratorStart,
    Return(Value),
    // Current thrown resumes enter the shared frame error boundary directly.
    #[allow(dead_code)]
    Throw(Value),
    Yield(Value),
    Await(Value),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NativeActivation {
    pub callable: Value,
    /// What the activation runs on and keeps alive besides its callable: an in-place
    /// generator step's generator (`Vm::try_in_loop_generator_step`), `undefined` for others.
    pub receiver: Value,
    pub frame_depth: usize,
    pub boundary: NativeCallBoundary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeCallBoundary {
    Forward,
    Opaque,
}

/// The origin of an activation, including the identity exposed to JavaScript.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CallContext {
    Function(Value),
    DirectEval(Value),
    IndirectEval(Value),
    Internal,
    Constructor(Value),
}

/// The single completion mode of a pushed call. Cross-program restoration and
/// a caller's terminal-return flag are independent of the value this call yields.
pub(crate) enum CallCompletion {
    Value,
    Constructor(Value),
    Promise(crate::heap::RootId),
    Generator(super::generator::GeneratorResume),
    /// A lane's array iteration whose callback became this frame: its return steps and
    /// finishes the iteration (`Vm::finish_array_step`), which yields the method's value.
    ArrayStep(Box<super::array_indexed::ArrayStepCall>),
}

impl CallCompletion {
    #[inline(always)]
    pub(super) fn constructor_this(&self) -> Option<Value> {
        match self { Self::Constructor(this) => Some(*this), _ => None }
    }

    #[inline(always)]
    pub(super) fn is_settling(&self) -> bool { matches!(self, Self::Promise(_)) }

    #[inline(always)]
    pub(super) fn is_generator(&self) -> bool { matches!(self, Self::Generator(_)) }

    /// Whether the call's value is made on the general path from what the callee returns:
    /// an async callee's promise settles there, an array step's iteration finishes there.
    #[inline(always)]
    pub(super) fn completes_on_general_path(&self) -> bool { matches!(self, Self::Promise(_) | Self::ArrayStep(_)) }

    #[inline(always)]
    pub(super) fn take_array_step(&mut self) -> Option<Box<super::array_indexed::ArrayStepCall>> {
        if !matches!(self, Self::ArrayStep(_)) { return None; }
        let Self::ArrayStep(step) = std::mem::replace(self, Self::Value) else { unreachable!() };
        Some(step)
    }

    #[inline(always)]
    pub(super) fn take_promise(&mut self) -> Option<crate::heap::RootId> {
        let Self::Promise(root) = self else { return None; };
        let root = *root;
        *self = Self::Value;
        Some(root)
    }

    #[inline(always)]
    pub(super) fn take_generator(&mut self) -> Option<super::generator::GeneratorResume> {
        if !self.is_generator() { return None; }
        let Self::Generator(resume) = std::mem::replace(self, Self::Value) else { unreachable!() };
        Some(resume)
    }
}

/// A pending constructor entry belongs only to its actual callee.
#[derive(Clone, Copy)]
pub(super) struct ConstructionTarget {
    pub(super) callee: Value,
    pub(super) new_target: Value,
}

impl CallContext {
    pub(super) fn constructed(self) -> Self {
        match self {
            Self::Function(value) | Self::Constructor(value) => Self::Constructor(value),
            other => other,
        }
    }

    pub(super) fn is_constructor(self) -> bool {
        matches!(self, Self::Constructor(_))
    }

    pub(super) fn user_function(id: u32, callable: Value) -> Self {
        if id == super::ROOT_FUNCTION_ID {
            Self::Internal
        } else {
            Self::Function(callable)
        }
    }

    pub(super) fn callee(self) -> Option<Value> {
        match self {
            Self::Function(value)
            | Self::Constructor(value)
            | Self::DirectEval(value)
            | Self::IndirectEval(value) => Some(value),
            Self::Internal => None,
        }
    }

    pub(super) fn callable(self) -> Option<Value> {
        match self {
            Self::Function(value) | Self::Constructor(value) => Some(value),
            Self::DirectEval(_) | Self::IndirectEval(_) | Self::Internal => None,
        }
    }
}

/// A suspended activation: its frame, kept whole with the buffers it ran with, and what
/// its resumer reads. Suspending moves the frame in and resuming moves it back out
/// (`resume`), so neither copies frame state nor gives up the frame's storage.
#[derive(Clone)]
pub(crate) struct Continuation {
    pub frame: Box<super::Frame>,
    pub completion: Completion,
    pub resume_register: Option<u16>,
    pub promise: Value,
    /// The registers live at the suspension, when liveness covers the function and every
    /// other register was cleared there (`Vm::suspend_activation`): only these can name a
    /// cell, so the GC traces them alone. `None` traces every register.
    pub(super) live_registers: Option<crate::compile::liveness::RegisterSet>,
}

impl std::fmt::Debug for Continuation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Continuation")
            .field("program", &self.frame.program)
            .field("function", &self.frame.function)
            .field("pc", &self.frame.pc)
            .field("completion", &self.completion)
            .field("resume_register", &self.resume_register)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GeneratorRecord {
    pub(crate) continuation: Option<Continuation>,
    pub(crate) realm: Value,
    pub(crate) done: bool,
    pub(crate) running: bool,
    pub(crate) requests: VecDeque<AsyncGeneratorRequest>,
}

impl GeneratorRecord {
    pub(crate) fn extend_roots(&self, out: &mut Vec<Value>) {
        out.push(self.realm);
        if let Some(continuation) = &self.continuation {
            continuation.extend_roots(out);
        }
        out.extend(self.requests.iter().flat_map(|request| [request.promise, request.value]));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsyncGeneratorOperation {
    Next,
    Return,
    Throw,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AsyncGeneratorRequest {
    pub(crate) operation: AsyncGeneratorOperation,
    pub(crate) promise: Value,
    pub(crate) value: Value,
}

impl Continuation {
    /// `frame`, suspended with `completion`; `resume_register` receives the value it resumes
    /// with, and `promise` is an async activation's result.
    pub(super) fn suspend(
        mut frame: Box<super::Frame>,
        completion: Completion,
        resume_register: Option<u16>,
        promise: Value,
    ) -> Self {
        frame.js_jit_interpreter_started = None;
        Self { frame, completion, resume_register, promise, live_registers: None }
    }

    /// The suspended frame, ready to push: it continues after its suspending instruction,
    /// every register holds a value, and the state a run derives for itself (the operation's
    /// field and binding site, the capture cache, a fixed `this`, the `with` base) starts anew.
    pub(super) fn resume(self, with_base: usize) -> Box<super::Frame> {
        let mut frame = self.frame;
        frame.pc_is_resume = true;
        frame.source_field = None;
        frame.binding_site_pc = None;
        frame.initialized_registers = frame.registers.len();
        frame.capture_cache = super::CaptureCache::default();
        frame.with_base = with_base;
        frame.js_jit_interpreter_started = None;
        frame.fixed_this = false;
        frame
    }

    /// The program the suspended activation runs.
    pub(crate) fn program(&self) -> ProgramId {
        self.frame.program
    }

    /// Its edges, appended to `out`: the GC traces every suspended frame this way. Locals and
    /// registers contribute only their heap values, and registers only the live ones where
    /// the suspension knows them (`live_registers`): each other one would cost the mark a
    /// push and a pop to find it names no cell.
    pub(crate) fn extend_roots(&self, out: &mut Vec<Value>) {
        let frame = &self.frame;
        out.extend(frame.context.callee());
        out.extend_from_slice(&frame.original_arguments);
        out.extend_from_slice(&frame.with_objects);
        out.extend([frame.env, frame.this]);
        Value::extend_heap_values(out, &frame.locals);
        out.extend(frame.dynamic_bindings.iter().map(|(_, value)| *value));
        match self.live_registers {
            Some(mut live) => {
                while live != 0 {
                    let register = live.trailing_zeros() as usize;
                    live &= live - 1;
                    if let Some(&value) = frame.registers.get(register)
                        && value.is_heap()
                    {
                        out.push(value);
                    }
                }
            }
            None => Value::extend_heap_values(out, &frame.registers),
        }
        out.extend(match self.completion {
            Completion::GeneratorStart => None,
            Completion::Return(value)
            | Completion::Throw(value)
            | Completion::Yield(value)
            | Completion::Await(value) => Some(value),
        });
        out.push(self.promise);
    }

    pub(crate) fn roots(&self) -> Vec<Value> {
        let mut roots = Vec::new();
        self.extend_roots(&mut roots);
        roots
    }
}

use crate::snapshot::Snap;

/// A suspended activation as a snapshot records it: its frame's suspended state, field by
/// field, then its resumer's; a restored continuation's frame starts its run anew on resume.
impl Snap for Continuation {
    fn snap(&self, writer: &mut crate::snapshot::Writer) -> crate::snapshot::SnapshotResult<()> {
        let super::Frame {
            context, reflected_parameters, original_arguments, program, function, pc, env, this,
            locals, dynamic_bindings, registers, active_iterators, with_objects, captured,
            pc_is_resume: _, source_field: _, binding_site_pc: _, initialized_registers: _,
            capture_cache: _, with_base: _, js_jit_interpreter_started: _, fixed_this: _,
            js_jit_skip_entry_once: _,
        } = &*self.frame;
        context.snap(writer)?;
        reflected_parameters.snap(writer)?;
        original_arguments.snap(writer)?;
        program.snap(writer)?;
        function.snap(writer)?;
        pc.snap(writer)?;
        env.snap(writer)?;
        this.snap(writer)?;
        locals.snap(writer)?;
        dynamic_bindings.snap(writer)?;
        registers.snap(writer)?;
        active_iterators.snap(writer)?;
        with_objects.snap(writer)?;
        self.completion.snap(writer)?;
        captured.snap(writer)?;
        self.resume_register.snap(writer)?;
        self.promise.snap(writer)
    }

    fn unsnap(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        let context = Snap::unsnap(reader)?;
        let reflected_parameters = Snap::unsnap(reader)?;
        let original_arguments = Snap::unsnap(reader)?;
        let program = Snap::unsnap(reader)?;
        let mut frame = Box::new(super::Frame::empty(program, 0));
        frame.context = context;
        frame.reflected_parameters = reflected_parameters;
        frame.original_arguments = original_arguments;
        frame.function = Snap::unsnap(reader)?;
        frame.pc = Snap::unsnap(reader)?;
        frame.env = Snap::unsnap(reader)?;
        frame.this = Snap::unsnap(reader)?;
        frame.locals = Snap::unsnap(reader)?;
        frame.dynamic_bindings = Snap::unsnap(reader)?;
        frame.registers = Snap::unsnap(reader)?;
        frame.active_iterators = Snap::unsnap(reader)?;
        frame.with_objects = Snap::unsnap(reader)?;
        let completion = Snap::unsnap(reader)?;
        frame.captured = Snap::unsnap(reader)?;
        Ok(Self {
            frame,
            completion,
            resume_register: Snap::unsnap(reader)?,
            promise: Snap::unsnap(reader)?,
            live_registers: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_completion_preserves_other_modes_and_consumes_promise_lease_once() {
        let old_fields = std::mem::size_of::<Option<Value>>()
            + std::mem::size_of::<Option<crate::heap::RootId>>()
            + std::mem::size_of::<Option<super::super::generator::GeneratorResume>>();
        assert!(std::mem::size_of::<CallCompletion>() < old_fields);
        eprintln!("call completion: {} bytes; previous independent fields: {old_fields}; step: {}; pending: {}",
            std::mem::size_of::<CallCompletion>(), std::mem::size_of::<super::super::StepResult>(),
            std::mem::size_of::<super::super::dispatch_frame::PendingGeneralCall>());
        let this = Value::number(17.0);
        let mut constructor = CallCompletion::Constructor(this);
        assert!(constructor.take_generator().is_none());
        assert!(constructor.take_promise().is_none());
        assert_eq!(constructor.constructor_this(), Some(this));
        let mut vm = super::super::Vm::new(crate::test_host::TestHost::default());
        let root = vm.heap.root(Value::number(42.0));
        let mut promise = CallCompletion::Promise(root);
        assert!(promise.take_generator().is_none());
        assert!(promise.is_settling());
        assert_eq!(promise.take_promise(), Some(root));
        assert!(matches!(promise, CallCompletion::Value));
        assert!(promise.take_promise().is_none());
        assert_eq!(vm.heap.root_value(root), Some(Value::number(42.0)));
        assert!(vm.heap.release_root(root));
        assert!(vm.heap.root_value(root).is_none());
    }

    #[test]
    fn continuation_roots_include_frame_and_completion_values() {
        let mut frame = Box::new(super::super::Frame::empty(ProgramId::MAIN, 0));
        frame.context = CallContext::Function(Value::heap(9));
        frame.original_arguments = vec![Value::heap(10)];
        frame.function = 3;
        frame.pc = 7;
        frame.env = Value::heap(1);
        frame.this = Value::heap(2);
        frame.locals = vec![Value::heap(4)];
        frame.dynamic_bindings = vec![(0, Value::heap(8))];
        frame.registers = vec![Value::heap(5)];
        frame.with_objects = vec![Value::heap(11)];
        frame.with_base = 3;
        frame.fixed_this = true;
        let continuation =
            Continuation::suspend(frame, Completion::Await(Value::heap(6)), Some(1), Value::heap(7));
        assert_eq!(
            continuation.roots(),
            [
                Value::heap(9),
                Value::heap(10),
                Value::heap(11),
                Value::heap(1),
                Value::heap(2),
                Value::heap(4),
                Value::heap(8),
                Value::heap(5),
                Value::heap(6),
                Value::heap(7)
            ]
        );
        // The resumed frame keeps the suspended state and starts its own run anew.
        let frame = continuation.resume(0);
        assert_eq!(frame.original_arguments, [Value::heap(10)]);
        assert_eq!(frame.with_objects, [Value::heap(11)]);
        assert_eq!((frame.function, frame.pc, frame.with_base), (3, 7, 0));
        assert_eq!(frame.initialized_registers, frame.registers.len());
        assert!(frame.pc_is_resume && !frame.fixed_this);
    }
}
