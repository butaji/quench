use std::rc::Rc;

use crate::value::{PromiseData, Value};

const INLINE_CALL_ARGUMENTS: usize = 4;

/// Owned call arguments with a small inline representation and one spill
/// representation. This is the single physical storage fact used by ordinary
/// and tail continuations; consumers continue to see a `&[Value]`.
#[derive(Debug)]
pub struct CallArguments {
    storage: CallArgumentStorage,
}

#[derive(Debug)]
enum CallArgumentStorage {
    Inline {
        values: [std::mem::MaybeUninit<Value>; INLINE_CALL_ARGUMENTS],
        len: usize,
    },
    Heap(Vec<Value>),
}

impl CallArguments {
    pub fn new() -> Self {
        Self {
            storage: CallArgumentStorage::Inline {
                values: [const { std::mem::MaybeUninit::uninit() }; INLINE_CALL_ARGUMENTS],
                len: 0,
            },
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        if capacity <= INLINE_CALL_ARGUMENTS {
            Self::new()
        } else {
            Self {
                storage: CallArgumentStorage::Heap(Vec::with_capacity(capacity)),
            }
        }
    }

    pub fn push(&mut self, value: Value) {
        match &mut self.storage {
            CallArgumentStorage::Inline { values, len } if *len < INLINE_CALL_ARGUMENTS => {
                values[*len].write(value);
                *len += 1;
            }
            CallArgumentStorage::Inline { values, len } => {
                let mut heap = Vec::with_capacity((*len + 1).max(INLINE_CALL_ARGUMENTS + 1));
                for value in values.iter_mut().take(*len) {
                    heap.push(unsafe { value.assume_init_read() });
                }
                heap.push(value);
                self.storage = CallArgumentStorage::Heap(heap);
            }
            CallArgumentStorage::Heap(values) => values.push(value),
        }
    }

    pub fn extend(&mut self, values: impl IntoIterator<Item = Value>) {
        for value in values {
            self.push(value);
        }
    }

    pub fn into_vec(self) -> Vec<Value> {
        let storage = unsafe { std::ptr::read(&self.storage) };
        std::mem::forget(self);
        match storage {
            CallArgumentStorage::Inline { values, len } => values
                .into_iter()
                .take(len)
                .map(|value| unsafe { value.assume_init() })
                .collect(),
            CallArgumentStorage::Heap(values) => values,
        }
    }
}

impl Default for CallArguments {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Vec<Value>> for CallArguments {
    fn from(values: Vec<Value>) -> Self {
        if values.len() > INLINE_CALL_ARGUMENTS {
            return Self {
                storage: CallArgumentStorage::Heap(values),
            };
        }
        let mut arguments = Self::new();
        arguments.extend(values);
        arguments
    }
}

impl Clone for CallArguments {
    fn clone(&self) -> Self {
        self.iter().cloned().collect()
    }
}

impl PartialEq for CallArguments {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl std::iter::FromIterator<Value> for CallArguments {
    fn from_iter<T: IntoIterator<Item = Value>>(iter: T) -> Self {
        let mut arguments = Self::new();
        arguments.extend(iter);
        arguments
    }
}

impl std::ops::Deref for CallArguments {
    type Target = [Value];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl CallArguments {
    fn as_slice(&self) -> &[Value] {
        match &self.storage {
            CallArgumentStorage::Inline { values, len } => unsafe {
                std::slice::from_raw_parts(values.as_ptr().cast::<Value>(), *len)
            },
            CallArgumentStorage::Heap(values) => values,
        }
    }
}

impl Drop for CallArguments {
    fn drop(&mut self) {
        if let CallArgumentStorage::Inline { values, len } = &mut self.storage {
            for value in values.iter_mut().take(*len) {
                unsafe { value.assume_init_drop() };
            }
        }
    }
}

/// State required to resume a caller after a non-tail call completes.
///
/// `caller_code` and `caller_pc` are compact integer return addresses.  The
/// isolate's code store is the sole owner of instructions; continuations never
/// retain an instruction slice or AST/IR allocation.  The address is valid
/// only while the originating machine/store is alive.
#[derive(Debug, Clone, PartialEq)]
pub struct CallContinuation {
    pub callee: Value,
    pub receiver: Value,
    pub arguments: CallArguments,
    pub caller_code: crate::identity::CodeId,
    pub caller_pc: u32,
    pub caller_registers: crate::register_file::RegisterFile,
    pub caller_environment: crate::identity::EnvironmentRef,
    pub destination: u16,
    pub guards: ContinuationGuards,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContinuationGuards {
    /// Guard state captured at the call boundary (strict-eval, with-scope,
    /// private-environment, and host-context bits are intentionally opaque to
    /// the completion transport).
    pub flags: u32,
}

impl ContinuationGuards {
    pub const fn new(flags: u32) -> Self {
        Self { flags }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct TailCallRequest {
    pub callee: Value,
    pub receiver: Value,
    pub arguments: CallArguments,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Completion {
    Normal,
    Return(Value),
    /// A call that must be executed before the current caller can continue.
    Call(CallContinuation),
    TailCall(TailCallRequest),
    Throw(Value),
    Break {
        label: Option<String>,
        value: Option<Value>,
    },
    Continue {
        label: Option<String>,
        value: Option<Value>,
    },
    Suspend(Rc<PromiseData>),
    /// A structured fragment suspended after recording its exact continuation
    /// point.  The point is internal VM state; outward async APIs still expose
    /// only the promise through `into_vm_error`.
    SuspendAt(Rc<PromiseData>, crate::continuation::SuspensionPoint),
    Yield(Value),
    /// A generator yield carrying the exact structured continuation that
    /// produced it. This keeps loop/try state in the completion rather than
    /// reconstructing it from source-shaped fragments on resume.
    YieldAt(Value, crate::continuation::SuspensionPoint),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LoopTransition {
    Continue(Option<Value>),
    Break(Option<Value>),
    Propagate(Completion),
}

impl Completion {
    pub(crate) fn visit_values(&self, mut visit: impl FnMut(&Value)) {
        match self {
            Self::Normal => {}
            Self::Return(value)
            | Self::Throw(value)
            | Self::Yield(value)
            | Self::YieldAt(value, _) => visit(value),
            Self::Break { value, .. } | Self::Continue { value, .. } => {
                value.iter().for_each(&mut visit)
            }
            Self::Call(call) => visit_call_values(call, &mut visit),
            Self::TailCall(call) => visit_tail_call_values(call, &mut visit),
            Self::Suspend(promise) | Self::SuspendAt(promise, _) => {
                visit(&Value::Promise(Rc::clone(promise)))
            }
        }
    }

    pub(crate) fn into_loop_transition(self, label: &Option<String>) -> LoopTransition {
        match self {
            Self::Normal => LoopTransition::Continue(None),
            Self::Continue {
                label: target,
                value,
            } if target == *label || target.is_none() => LoopTransition::Continue(value),
            Self::Break {
                label: target,
                value,
            } if target == *label || target.is_none() => LoopTransition::Break(value),
            completion => LoopTransition::Propagate(completion),
        }
    }

    pub(crate) fn update_empty(self, value: Value) -> Self {
        match self {
            Self::Continue { label, value: None } => Self::Continue {
                label,
                value: Some(value),
            },
            Self::Break { label, value: None } => Self::Break {
                label,
                value: Some(value),
            },
            other => other,
        }
    }

    pub(crate) fn is_suspension(&self) -> bool {
        matches!(
            self,
            Self::Suspend(_) | Self::SuspendAt(_, _) | Self::Yield(_) | Self::YieldAt(_, _)
        )
    }

    pub(crate) fn suspension_point(&self) -> Option<&crate::continuation::SuspensionPoint> {
        match self {
            Self::SuspendAt(_, point) => Some(point),
            Self::YieldAt(_, point) => Some(point),
            _ => None,
        }
    }

    pub(crate) fn from_vm_error(
        error: crate::execute::VmError,
    ) -> Result<Self, crate::execute::VmError> {
        use crate::execute::VmError;
        Ok(match error {
            VmError::Thrown(value) => Self::Throw(value),
            VmError::Break(label) => Self::Break { label, value: None },
            VmError::Continue(label) => Self::Continue { label, value: None },
            VmError::Suspended(promise) => Self::Suspend(promise),
            VmError::NotCallable => {
                let VmError::Thrown(value) = crate::execute::not_callable() else {
                    return Err(VmError::NotCallable);
                };
                Self::Throw(value)
            }
            error => return Err(error),
        })
    }

    pub fn into_vm_error(self) -> Result<Value, crate::execute::VmError> {
        use crate::execute::VmError;
        match self {
            Self::Normal => Err(VmError::MissingReturn),
            Self::Return(value) => Ok(value),
            Self::Call(_) => Err(VmError::EvalError("Unconsumed call completion".to_string())),
            Self::TailCall(_) => Err(VmError::EvalError(
                "Unconsumed tail-call completion".to_string(),
            )),
            Self::Throw(value) => Err(VmError::Thrown(value)),
            Self::Break { label, .. } => Err(VmError::Break(label)),
            Self::Continue { label, .. } => Err(VmError::Continue(label)),
            Self::Suspend(promise) => Err(VmError::Suspended(promise)),
            Self::SuspendAt(promise, _) => Err(VmError::Suspended(promise)),
            Self::Yield(_) => Err(VmError::EvalError(
                "Unconsumed yield completion".to_string(),
            )),
            Self::YieldAt(_, _) => Err(VmError::EvalError(
                "Unconsumed yield completion".to_string(),
            )),
        }
    }
}

fn visit_call_values(call: &CallContinuation, visit: &mut impl FnMut(&Value)) {
    visit(&call.callee);
    visit(&call.receiver);
    call.arguments.iter().for_each(&mut *visit);
    call.caller_registers.visit_values(|value| visit(&value));
}

fn visit_tail_call_values(call: &TailCallRequest, visit: &mut impl FnMut(&Value)) {
    visit(&call.callee);
    visit(&call.receiver);
    call.arguments.iter().for_each(visit);
}

#[cfg(test)]
mod call_argument_tests {
    use super::CallArguments;
    use crate::value::Value;

    #[test]
    fn inline_arguments_round_trip_without_spill() {
        let mut arguments = CallArguments::new();
        arguments.push(Value::Number(1.0));
        arguments.push(Value::Boolean(true));
        assert_eq!(
            arguments.as_slice(),
            &[Value::Number(1.0), Value::Boolean(true)]
        );
        assert_eq!(arguments.clone().into_vec(), arguments.as_slice());
    }

    #[test]
    fn spill_arguments_preserve_order_and_values() {
        let values = (0..6)
            .map(|value| Value::Number(value as f64))
            .collect::<Vec<_>>();
        let arguments: CallArguments = values.clone().into();
        assert_eq!(arguments.as_slice(), values.as_slice());
        assert_eq!(arguments.clone().into_vec(), values);
    }
}
