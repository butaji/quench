use super::program_store::ProgramId;
use crate::Value;

mod frame_record;
mod generator_record;
pub(crate) use frame_record::FrameRecord;
#[cfg(test)]
pub(in crate::vm) use frame_record::{frames_made, note_frame_made};
pub(crate) use generator_record::{GeneratorRecord, GeneratorState};

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

/// What owns the suspended activation a lane resumes with no VM frame beneath it
/// (`JobRoot`): an async function's await state, or a generator's record, async or the native
/// `next` of a sync one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SuspendKind {
    Generator,
    AsyncFunction,
    AsyncGenerator,
}

/// The root of a lane run that resumes a suspended activation (`Vm::run_lane_job`): the
/// object that owns its frame, the await state or the generator, and what kind it is.
#[derive(Clone, Copy, Debug)]
pub(crate) struct JobRoot {
    pub(crate) object: Value,
    pub(crate) kind: SuspendKind,
}

/// How a promise job or a request resumes the activation its owner suspended
/// (`Vm::run_job`).
#[derive(Debug)]
pub(crate) enum JobResume {
    /// With the value its `await` or `yield` evaluates to.
    Sends(Value),
    /// Throwing the error where it suspended, which a handler of its bytecode may catch.
    Throws(super::JsError),
}

impl JobResume {
    /// The value the resumed `await` or `yield` is sent: `undefined` for a throw, which sends none.
    pub(crate) fn sent(&self) -> Value {
        match self {
            Self::Sends(sent) => *sent,
            Self::Throws(_) => Value::UNDEFINED,
        }
    }
    /// The heap value it carries, which the run keeps alive.
    pub(crate) fn value(&self) -> Option<Value> {
        match self {
            Self::Sends(value) => Some(*value),
            Self::Throws(error) => error.thrown_value(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NativeActivation {
    pub callable: Value,
    /// What the activation runs on and keeps alive besides its callable: a windowed
    /// generator step's generator (`Vm::resume_generator_window`), `undefined` for others.
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
    /// A store's setter the lane activated: the assignment yields the value stored, which
    /// the caller's register still holds, whatever the setter returns.
    Assigned,
}

impl CallCompletion {

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

/// A suspended activation: its frame record (`FrameRecord`), which owns the frame state,
/// and what its resumer reads. Suspending moves the frame's state into the record and
/// resuming moves it back out (`FrameRecord::into_frame`), so neither copies it nor gives up its storage.
#[derive(Clone)]
pub(crate) struct Continuation {
    pub frame: Box<FrameRecord>,
    pub completion: Completion,
    pub resume_register: Option<u16>,
    pub promise: Value,
}

impl std::fmt::Debug for Continuation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Continuation")
            .field("frame", &self.frame)
            .field("completion", &self.completion)
            .field("resume_register", &self.resume_register)
            .finish_non_exhaustive()
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
        frame: Box<super::Frame>,
        completion: Completion,
        resume_register: Option<u16>,
        promise: Value,
    ) -> Self {
        Self { frame: FrameRecord::capture(frame), completion, resume_register, promise }
    }

    /// A lane resumes the frame as an activation over its register file, which stays in this
    /// continuation's record: the frame's address, which its box keeps until the run ends,
    /// and the register that takes the value the run resumes with. Every register is live
    /// while it runs.
    pub(crate) fn begin_window(&mut self) -> (*mut FrameRecord, Option<u16>) {
        self.frame.live_registers = None;
        (&raw mut *self.frame, self.resume_register)
    }

    /// The program the suspended activation runs.
    pub(crate) fn program(&self) -> ProgramId {
        self.frame.program
    }

    /// Its edges, appended to `out`: the GC traces every suspended frame this way
    /// (`FrameRecord::extend_roots`).
    pub(crate) fn extend_roots(&self, out: &mut Vec<Value>) {
        self.frame.extend_roots(out);
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

/// A suspended activation as a snapshot records it: its frame record, then its resumer's.
impl Snap for Continuation {
    fn snap(&self, writer: &mut crate::snapshot::Writer) -> crate::snapshot::SnapshotResult<()> {
        self.frame.snap(writer)?;
        self.completion.snap(writer)?;
        self.resume_register.snap(writer)?;
        self.promise.snap(writer)
    }

    fn unsnap(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        Ok(Self {
            frame: Box::new(Snap::unsnap(reader)?),
            completion: Snap::unsnap(reader)?,
            resume_register: Snap::unsnap(reader)?,
            promise: Snap::unsnap(reader)?,
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
        assert!(matches!(constructor, CallCompletion::Constructor(receiver) if receiver == this));
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
        let frame = continuation.frame.into_frame(0);
        assert_eq!(frame.original_arguments, [Value::heap(10)]);
        assert_eq!(frame.with_objects, [Value::heap(11)]);
        assert_eq!((frame.function, frame.pc, frame.with_base), (3, 7, 0));
        assert_eq!(frame.initialized_registers, frame.registers.len());
        assert!(frame.pc_is_resume && !frame.fixed_this);
    }
}
