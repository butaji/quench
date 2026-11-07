//! Direct calls of numeric compiled bodies from the lane, and of leaf bodies of the plain
//! `fn(*const f64)` ABI without the shared entry.
use super::{CompiledJsCallResult, CompiledJsEntry, CompiledJsFunction, CompiledJsResult, DirectTierEntry, NativeEntryMode, native_poll_page};
use crate::Value;

pub(super) fn direct_call(
    compiled: &CompiledJsFunction,
    arguments: &[Value],
    interrupt: &crate::ExecutionInterrupt,
) -> Option<CompiledJsCallResult> {
    let int32_identity = compiled.is_int32_identity();
    let requires_int32 = compiled.requires_signed_int32_arguments();
    let int32 = |index| requires_int32 || (int32_identity && index == 0);
    let params = compiled.params().min(arguments.len());
    // The common case, a few parameters, converts into a fixed array on the stack.
    if params <= LEAF_PARAMS {
        let mut numeric = [0.0_f64; LEAF_PARAMS];
        numeric_arguments(arguments, &mut numeric[..params], int32)?;
        return direct_call_numbers(compiled, &numeric[..params], interrupt);
    }
    let mut numeric = vec![0.0_f64; params];
    numeric_arguments(arguments, &mut numeric, int32)?;
    direct_call_numbers(compiled, &numeric, interrupt)
}

#[inline(always)]
fn direct_call_numbers(
    compiled: &CompiledJsFunction,
    numeric_arguments: &[f64],
    interrupt: &crate::ExecutionInterrupt,
) -> Option<CompiledJsCallResult> {
    // Only a body that polls reads the poll page.
    let poll_page = (compiled.native_entry_mode() == NativeEntryMode::Resumable)
        .then(|| native_poll_page(interrupt))
        .flatten();
    match compiled.call(numeric_arguments, interrupt, poll_page)? {
        CompiledJsResult::Number(value) => Some(CompiledJsCallResult::Value(value)),
        CompiledJsResult::Int32Identity { value, .. } => Some(CompiledJsCallResult::Value(Value::number(value))),
        CompiledJsResult::Boolean(value) => {
            Some(CompiledJsCallResult::Value(if value { Value::TRUE } else { Value::FALSE }))
        }
        CompiledJsResult::StringReturnPc(return_pc) => {
            compiled.string_constant_for_return(return_pc).map(CompiledJsCallResult::ReturnConstant)
        }
        CompiledJsResult::StringTransform(_, _) | CompiledJsResult::FieldReject { .. } => None,
    }
}

/// Fills `numbers` with the Numbers `arguments` start with; `None` where one is no Number,
/// or no int32 where `int32(index)` requires one. An integer Value is an int32 Number by
/// representation; any other Number is checked where the body's guards require one.
#[inline(always)]
fn numeric_arguments(arguments: &[Value], numbers: &mut [f64], int32: impl Fn(usize) -> bool) -> Option<()> {
    for (index, (number, argument)) in numbers.iter_mut().zip(arguments).enumerate() {
        *number = match argument.as_int() {
            Some(integer) => f64::from(integer),
            None => {
                let value = argument.as_number()?;
                if int32(index) && !super::facts::is_int32_number(value) { return None; }
                value
            }
        };
    }
    Some(())
}

/// A compiled leaf body of the plain `fn(*const f64)` ABI with at most `LEAF_PARAMS`
/// parameters: what a direct run calls without the shared entry.
#[derive(Clone, Copy)]
pub(super) struct LeafCall {
    kind: LeafKind,
    params: u8,
    /// Per parameter, whether a non-integer Number argument must still be an int32.
    int32_params: u8,
}

#[derive(Clone, Copy)]
enum LeafKind {
    Int32Identity(unsafe extern "C" fn(*const f64) -> f64),
    Boolean(unsafe extern "C" fn(*const f64) -> u32),
}

const LEAF_PARAMS: usize = 8;

impl LeafCall {
    pub(super) fn of(compiled: &CompiledJsFunction) -> Option<Self> {
        let params = compiled.params();
        if params > LEAF_PARAMS { return None; }
        let (kind, int32_params) = match compiled.entry {
            CompiledJsEntry::Int32Identity { entry, .. } => (LeafKind::Int32Identity(entry), 1),
            CompiledJsEntry::Boolean(entry) => (LeafKind::Boolean(entry), 0),
            _ => return None,
        };
        let int32_params = if compiled.requires_signed_int32_arguments() { u8::MAX } else { int32_params };
        Some(Self { kind, params: params as u8, int32_params })
    }

    /// The body's result on `arguments`; `None` where an argument is no Number its guards
    /// admit, as `direct_call`.
    #[inline(always)]
    pub(super) fn run(self, arguments: &[Value]) -> Option<Value> {
        let mut numeric = [0.0_f64; LEAF_PARAMS];
        let params = usize::from(self.params);
        if arguments.len() < params { return None; }
        numeric_arguments(arguments, &mut numeric[..params], |index| self.int32_params >> index & 1 != 0)?;
        // SAFETY: the variant names the ABI the artifact emits (`CompiledJsFunction::from_artifact`);
        // a leaf makes no call, polls nothing and cannot fault.
        Some(match self.kind {
            LeafKind::Int32Identity(entry) => Value::number(unsafe { entry(numeric.as_ptr()) }),
            LeafKind::Boolean(entry) => {
                if unsafe { entry(numeric.as_ptr()) } & super::super::js_compiled::BOOLEAN_REGION_RESULT_MASK != 0 { Value::TRUE } else { Value::FALSE }
            }
        })
    }
}

impl CompiledJsFunction {
    /// A leaf body's result on `arguments`, which hold every input it reads; `None` for
    /// any other body.
    #[inline(always)]
    pub(super) fn call_leaf(&self, arguments: &[f64]) -> Option<CompiledJsResult> {
        // A leaf makes no call, polls nothing and cannot fault: it runs as a direct call,
        // without the shared adapter's context.
        // SAFETY: each artifact kind emits the ABI its variant names (`from_artifact`), and
        // `arguments` holds every input it reads (`call` checked).
        let result = match self.entry {
            CompiledJsEntry::Int32Identity { entry, return_pc } =>
                CompiledJsResult::Int32Identity { value: unsafe { entry(arguments.as_ptr()) }, return_pc },
            CompiledJsEntry::Boolean(entry) => CompiledJsResult::Boolean(
                unsafe { entry(arguments.as_ptr()) } & super::super::js_compiled::BOOLEAN_REGION_RESULT_MASK != 0),
            CompiledJsEntry::StringReturnPc(entry) => CompiledJsResult::StringReturnPc(unsafe { entry(arguments.as_ptr()) }),
            _ => return None,
        };
        #[cfg(test)]
        self.native_entries.set(self.native_entries.get().saturating_add(1));
        Some(result)
    }
}

impl DirectTierEntry {
    /// The leaf run of this entry on `parameters` (`LeafCall`): `None` where it has no leaf
    /// call, else the run's outcome, `Some(None)` where an argument does not admit it or the
    /// tier retired the artifact.
    #[inline(always)]
    pub(in crate::vm) fn run_leaf(&self, parameters: &[Value]) -> Option<Option<(usize, crate::bytecode::Register, Value)>> {
        let leaf = self.leaf?;
        // The artifact lives while the tier holds it; its code then stays mapped.
        if self.retired() { return Some(None); }
        #[cfg(test)]
        if let Some(compiled) = self.compiled.upgrade() {
            compiled.native_entries.set(compiled.native_entries.get().saturating_add(1));
        }
        Some(leaf.run(parameters).map(|value| (self.return_pc, self.return_register, value)))
    }
}
