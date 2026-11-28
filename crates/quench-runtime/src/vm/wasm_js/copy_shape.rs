//! The shape of a memory-copy import helper (`Vm::emscripten_memcpy_view_capture`): a
//! three-parameter function whose body is `view.copyWithin(a, b, b + n)` over one captured
//! binding, proved on its bytecode, so any spelling of the helper is recognized and no source
//! text or name decides it.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt};
use super::*;
use crate::bytecode::{Atom, FieldBase, FieldLookup, Operand, OperandKind, Register, RETURN_REGISTER};
use crate::staged::{Arguments, CallOperation, Callee, DecodeContext, Invocation, NumberOperation, Operation, Receiver};
use crate::vm::operations::NumberArithmetic;
#[cfg(not(stencil_build))] #[allow(unused_imports)] use crate::staged::glue_prelude::*;

/// What a register or a frame local holds in a candidate helper body, as the walk proves it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyValue {
    /// Parameter `index` of the function.
    Parameter(usize),
    /// The captured binding the body reads.
    View,
    /// The `copyWithin` method read from the view.
    Method,
    /// Parameter 1 plus parameter 2 (`src + num`).
    ParameterSum,
    /// The result of the body's one call.
    CallResult,
    /// Any other value.
    Other,
}

/// The call's arguments, in order: `dest`, `src` and `src + num`.
const COPY_WITHIN_ARGUMENTS: [BodyValue; 3] = [
    BodyValue::Parameter(0),
    BodyValue::Parameter(1),
    BodyValue::ParameterSum,
];

/// The walk over a candidate body's operations (`copy_within_capture`). It accepts exactly one capture
/// load, one `copyWithin` read of the view, one addition of parameters 1 and 2, one call of the method on
/// the view with the arguments above, and a return of that call's result. Moves and loads of the
/// parameters may appear around them; nothing else may.
struct CopyWithinBody<'a> {
    program: &'a ResidualProgram,
    /// The frame local each of the three parameters arrives in.
    locals: [usize; 3],
    /// What each register holds, `Other` past the end.
    registers: Vec<BodyValue>,
    /// The captured binding as its depth and slot, once its load is seen.
    capture: Option<(u16, u16)>,
    method_read: bool,
    summed: bool,
    /// The register the body's call writes.
    call: Option<Register>,
    /// Whether the body's completion is the call's (a return, or a call that returns).
    returned: bool,
}

impl CopyWithinBody<'_> {
    fn value(&self, register: Register) -> BodyValue {
        self.registers.get(usize::from(register)).copied().unwrap_or(BodyValue::Other)
    }

    fn define(&mut self, register: Register, value: BodyValue) {
        let index = usize::from(register);
        if index >= self.registers.len() {
            self.registers.resize(index + 1, BodyValue::Other);
        }
        self.registers[index] = value;
    }

    fn local(&self, slot: u16) -> BodyValue {
        self.locals
            .iter()
            .position(|&local| local == usize::from(slot))
            .map_or(BodyValue::Other, BodyValue::Parameter)
    }

    fn operand(&self, operand: Operand) -> BodyValue {
        match operand.kind() {
            Some(OperandKind::Register) => self.value(operand.payload()),
            Some(OperandKind::Local) => self.local(operand.payload()),
            _ => BodyValue::Other,
        }
    }

    /// Walks one operation; `false` where the body holds any other operation.
    fn step(&mut self, operation: Operation<(), ()>, flags: Register) -> bool {
        if self.returned {
            return matches!(operation, Operation::Nop);
        }
        match operation {
            Operation::Call(CallOperation::Site(call)) => self.invoke(call, flags),
            Operation::Return { source } if flags == 0 => self.ret(source),
            _ => flags == 0 && self.data(operation),
        }
    }

    /// The value operations of the body: parameter and capture loads, the view's method read, the sum.
    fn data(&mut self, operation: Operation<(), ()>) -> bool {
        match operation {
            Operation::Nop => true,
            Operation::Move { destination, source } => {
                let value = self.value(source);
                self.define(destination, value);
                true
            }
            Operation::LoadLocalPlain { destination, slot }
            | Operation::LoadLocalChecked { destination, slot, .. } => self.load_local(destination, slot),
            Operation::LoadCapture { destination, depth, slot, root: false } => {
                self.load_capture(destination, depth, slot)
            }
            Operation::GetField {
                lookup: FieldLookup::Atom { atom, base, .. },
                destination,
                ..
            } => self.read_method(destination, atom, base),
            Operation::Number(NumberOperation::Arithmetic {
                destination,
                left,
                right,
                operation: NumberArithmetic::Add,
                ..
            }) => self.sum(destination, left, right),
            _ => false,
        }
    }

    fn load_local(&mut self, destination: Register, slot: u16) -> bool {
        match self.local(slot) {
            value @ BodyValue::Parameter(_) => {
                self.define(destination, value);
                true
            }
            _ => false,
        }
    }

    fn load_capture(&mut self, destination: Register, depth: u16, slot: u16) -> bool {
        if self.capture.is_some() {
            return false;
        }
        self.capture = Some((depth, slot));
        self.define(destination, BodyValue::View);
        true
    }

    fn read_method(&mut self, destination: Register, atom: Atom, base: FieldBase) -> bool {
        let on_view = base.register_index().is_some_and(|register| self.value(register) == BodyValue::View);
        let named_copy_within = self.program.atoms.get(atom as usize) == Some("copyWithin");
        if self.method_read || !on_view || !named_copy_within {
            return false;
        }
        self.method_read = true;
        self.define(destination, BodyValue::Method);
        true
    }

    fn sum(&mut self, destination: Register, left: Operand, right: Operand) -> bool {
        let adds_parameters = self.operand(left) == BodyValue::Parameter(1)
            && self.operand(right) == BodyValue::Parameter(2);
        if self.summed || !adds_parameters {
            return false;
        }
        self.summed = true;
        self.define(destination, BodyValue::ParameterSum);
        true
    }

    /// The body's one call: `view.copyWithin` on the view, with the arguments in order. Its flags are the
    /// completion flags only: a call that returns makes the body return.
    fn invoke(&mut self, call: Invocation, flags: Register) -> bool {
        let (Callee::Register(callee), Receiver::Register(receiver), Arguments::Window { first, count }) =
            (call.callee, call.receiver, call.arguments)
        else {
            return false;
        };
        let on_method = self.value(callee) == BodyValue::Method && self.value(receiver) == BodyValue::View;
        let arguments = COPY_WITHIN_ARGUMENTS.iter().enumerate().all(|(index, expected)| {
            u16::try_from(index)
                .ok()
                .and_then(|offset| first.checked_add(offset))
                .is_some_and(|register| self.value(register) == *expected)
        });
        if self.call.is_some()
            || call.forwards
            || usize::from(count) != COPY_WITHIN_ARGUMENTS.len()
            || !on_method
            || !arguments
            || !matches!(flags, 0 | RETURN_REGISTER)
        {
            return false;
        }
        self.call = Some(call.result);
        self.returned = flags != 0;
        self.define(call.result, BodyValue::CallResult);
        true
    }

    fn ret(&mut self, source: Register) -> bool {
        if self.call.is_none() || self.value(source) != BodyValue::CallResult {
            return false;
        }
        self.returned = true;
        true
    }
}

/// The captured binding `(depth, slot)` that the body of `function` reads as its view, when the body is
/// `(p0, p1, p2) => V.copyWithin(p0, p1, p1 + p2)` over that one capture. Parameters are proved by the
/// local each arrives in, or by the register it is promoted to; names play no part.
fn copy_within_capture(program: &ResidualProgram, function: &crate::bytecode::Function) -> Option<(u16, u16)> {
    let atoms: [Atom; 3] = function.parameter_atoms.get(..COPY_WITHIN_ARGUMENTS.len())?.try_into().ok()?;
    let [first, second, third] = atoms.map(|atom| function.local_slot(atom));
    let locals = [first?, second?, third?];
    let mut body = CopyWithinBody {
        program,
        locals,
        registers: Vec::new(),
        capture: None,
        method_read: false,
        summed: false,
        call: None,
        returned: false,
    };
    for (index, &local) in locals.iter().enumerate() {
        let promoted = u16::try_from(local).ok().and_then(|slot| function.promoted_register(slot));
        if let Some(register) = promoted {
            body.define(register, BodyValue::Parameter(index));
        }
    }
    for instruction in function.instructions() {
        let (operation, flags) = Operation::<(), ()>::decode_completed(instruction, DecodeContext::EMPTY)?;
        if !body.step(operation, flags) {
            return None;
        }
    }
    body.capture.filter(|_| body.returned)
}

impl Vm {
    /// Recognizes the Emscripten memcpy import helper by its bytecode, whatever its spelling: a user
    /// function of exactly three parameters whose body is `(p0, p1, p2) => V.copyWithin(p0, p1, p1 + p2)`
    /// over one captured binding `V` (`copy_within_capture`). The binding is named by its capture's depth
    /// and slot in the owner's environment, never by name. The capture location is retained; the view itself
    /// is deliberately re-read on every call because Emscripten replaces it after `WebAssembly.Memory.grow()`.
    #[quench_macros::test_visible]
    pub(in crate::vm) fn emscripten_memcpy_view_capture(
        &self,
        function: Value,
        signature: &crate::WasmSignature,
    ) -> Option<(Value, usize)> {
        use crate::WasmType::I32;

        if signature.params.as_slice() != [I32, I32, I32] {
            return None;
        }
        let FunctionRecord {
            kind: FunctionKind::User(program_id, function_id),
            env,
            ..
        } = self.heap.function(function)?
        else {
            return None;
        };
        let program = self.programs.get(*program_id)?;
        let metadata = program.function_get(*function_id as usize)?;
        if metadata.params != 3
            || metadata.inherited_with_scope
            || !metadata.binding_sites.is_empty()
        {
            return None;
        }
        let (depth, slot) = copy_within_capture(&program, metadata)?;
        let owner_environment = self.environment_capture_env(*env, false, depth)?;
        let view_atom = self.captured_binding_atom(owner_environment, slot)?;
        // Verify that the statically resolved binding still names this slot in a
        // plain lexical environment. Captured with/eval scopes can shadow a static
        // outer declaration even though their parent chain also contains that slot.
        let mut environment = *env;
        loop {
            let crate::heap::EnvironmentRecord {
                parent,
                function: owner_function,
                slots,
                scope,
            } = self.heap.environment(environment)?;
            if scope.root_eval_scope()
                || scope.binding_site_pc().is_some()
                || !scope.with_objects().is_empty()
                || self
                    .heap
                    .environment_bindings(environment)?
                    .iter()
                    .any(|(atom, _)| *atom == view_atom)
            {
                return None;
            }
            if environment == owner_environment {
                let owner_program_id = super::program_store::ProgramId::from_raw(scope.program()?);
                let owner_program = self.programs.get(owner_program_id)?;
                let owner_metadata = owner_program.function_get(*owner_function as usize)?;
                let owns_atom = if *owner_function == super::ROOT_FUNCTION_ID {
                    owner_metadata.declares_global_lexical(view_atom)
                        || (owner_program.root_variables_are_local()
                            && owner_metadata.global_var_atoms().contains(&view_atom))
                } else {
                    owner_metadata.declares_environment_atom(view_atom)
                };
                if !owns_atom
                    || self.local_binding_slot(&owner_program, *owner_function, view_atom)?
                        != usize::from(slot)
                    || usize::from(slot) >= slots.len()
                {
                    return None;
                }
                return Some((environment, usize::from(slot)));
            }
            environment = *parent;
        }
    }

    /// The atom of the binding in local `slot` of the function that owns `environment`: the name a
    /// capture of that slot reads, which the environment walk then verifies.
    fn captured_binding_atom(&self, environment: Value, slot: u16) -> Option<Atom> {
        let crate::heap::EnvironmentRecord {
            function, scope, ..
        } = self.heap.environment(environment)?;
        let program = self
            .programs
            .get(super::program_store::ProgramId::from_raw(scope.program()?))?;
        program
            .function_get(*function as usize)?
            .local_atoms
            .get(usize::from(slot))
            .copied()
    }
}
