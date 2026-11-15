#[path = "activation_layout.rs"] mod activation_layout;
use super::control_flow::instruction_at;
use super::{
    FieldBase, FieldLayout, ImmediateLayout, InstructionField, Op, Operand, OperandKind,
    REGISTER_MASK, Register, ResidualProgram,
};
use rustc_hash::FxHashSet;
use crate::bitset::BitSet;

fn register_in_bounds(register: u16, limit: u16, flags: u16) -> bool {
    register & !(REGISTER_MASK | flags) == 0 && register & REGISTER_MASK < limit
}

fn field_base_in_bounds(base: u16, limit: u16, locals: u16) -> bool {
    let base = FieldBase(base);
    base == FieldBase::THIS
        || base.0 == FieldBase::NESTED
        || base.local_index().is_some_and(|local| local < locals)
        || base.register_index().is_some_and(|register| register < limit)
}

fn operand_in_bounds(
    operand: u16,
    registers: u16,
    locals: u16,
    constants: usize,
    fields: usize,
) -> bool {
    let operand = Operand(operand);
    match operand.kind() {
        Some(OperandKind::Register) => register_in_bounds(operand.payload(), registers, 0),
        Some(OperandKind::Constant) => usize::from(operand.payload()) < constants,
        Some(OperandKind::Field) => usize::from(operand.payload()) < fields,
        Some(OperandKind::Local) => operand.payload() < locals,
        None => false,
    }
}

fn is_numeric_index_operand(operand: Operand) -> bool {
    matches!(
        operand.kind(),
        Some(OperandKind::Register | OperandKind::Local)
    )
}

fn numeric_index_operand_in_bounds(operand: u16, bounds: ValidationBounds) -> bool {
    is_numeric_index_operand(Operand(operand))
        && operand_in_bounds(
            operand,
            bounds.registers,
            bounds.locals,
            bounds.constants,
            bounds.field_sites,
        )
}

fn atom_in_bounds(atom: u32, atoms: usize) -> bool {
    (atom as usize) < atoms
}

fn eval_binding_is_valid(
    binding: &super::EvalBinding,
    function: &super::Function,
    functions: &[super::FunctionSlot],
    atoms: usize,
) -> bool {
    let target = match binding.location {
        super::EvalBindingLocation::Local(_) => Some(function),
        super::EvalBindingLocation::Capture { depth, .. } => {
            let mut parent = function.parent;
            for _ in 0..depth {
                parent = parent
                    .and_then(|id| functions.get(id as usize))
                    .and_then(|function| function.parent);
            }
            parent.and_then(|id| functions.get(id as usize).map(|slot| &**slot))
        }
    };
    let slot = match binding.location {
        super::EvalBindingLocation::Local(slot)
        | super::EvalBindingLocation::Capture { slot, .. } => slot,
    };
    atom_in_bounds(binding.atom, atoms)
        && target.is_some_and(|target| {
            slot < target.locals && usize::from(binding.with_depth) <= function.code.len()
        })
}

/// Every name-binding list is sorted with one entry per atom, and each binding names a slot
/// of the function it resolves to.
fn name_bindings_are_valid(
    function: &super::Function,
    functions: &[super::FunctionSlot],
    atoms: usize,
    checked_captured: &mut rustc_hash::FxHashSet<(usize, u16, Option<u32>)>,
) -> bool {
    let bindings = &function.name_bindings;
    if !strictly_ascending(bindings.direct().iter().map(|binding| binding.atom))
        || !strictly_ascending(bindings.hidden().iter().copied())
        || !bindings
            .hidden()
            .iter()
            .all(|atom| atom_in_bounds(*atom, atoms))
        || !bindings
            .direct()
            .iter()
            .all(|binding| eval_binding_is_valid(binding, function, functions, atoms))
    {
        return false;
    }
    bindings.captured_levels().all(|(depth, level)| {
        let key = (
            level as *const super::CapturedNames as usize,
            depth,
            function.parent,
        );
        checked_captured.contains(&key)
            || (strictly_ascending(level.bindings_at(depth).map(|binding| binding.atom))
                && level
                    .bindings_at(depth)
                    .all(|binding| eval_binding_is_valid(&binding, function, functions, atoms))
                && checked_captured.insert(key))
    })
}

fn strictly_ascending(atoms: impl IntoIterator<Item = super::Atom>) -> bool {
    let mut previous = None;
    atoms.into_iter().all(|atom| {
        previous
            .replace(atom)
            .is_none_or(|previous| previous < atom)
    })
}

fn cache_in_bounds(cache: u16, caches: u16) -> bool {
    cache < caches
}

fn register_window_in_bounds(base: u16, count: u32, registers: u16) -> bool {
    u32::from(base)
        .checked_add(count)
        .is_some_and(|end| end <= u32::from(registers))
}

fn promoted_local_layout_is_valid(function: &super::Function, atoms: &super::AtomTable) -> bool {
    let promoted = &function.local_registers;
    if promoted.is_empty() {
        return true;
    }
    let has_dynamic_local_resolution = function
        .code
        .iter()
        .filter_map(|instruction| instruction_at(function, *instruction))
        .chain(function.wide.iter().copied())
        .any(|instruction| {
            matches!(
                instruction.op(),
                Op::LoadName
                    | Op::LoadNameCall
                    | Op::LoadNameTypeof
                    | Op::ResolveName
                    | Op::StoreName
                    | Op::DeleteName
            ) && promoted.iter().any(|entry| {
                function.local_atoms.get(usize::from(entry.local)) == Some(&instruction.imm())
            })
        });
    let eligible_function = function.parent.is_some()
        // An async body that never suspends runs to completion as an ordinary one does
        // (`Compiler::promoted_local_plan`'s `async_body`).
        && !(function.is_async && function.suspends())
        && !function.is_generator
        && !function.derived_constructor
        // An arrow inside an initializer owns its locals (`compile/arrow.rs`).
        && !(function.class_initializer && !function.is_arrow)
        // A rest parameter is promoted where the list binds its names directly
        // (`FunctionCompiler::binds_parameters_directly`), each its own slot; a list binding
        // its names apart promotes its hidden argument slots.
        && function.arguments_own_their_slots(|atom| atoms.get(atom))
        // Promoted parameters leave their slots, which only an aliasing `arguments` reads.
        && (function.arguments_slot.is_none() || !function.arguments_alias_parameters())
        && !function.inherited_with_scope
        && function.binding_sites.is_empty()
        && !has_dynamic_local_resolution
        && !function.code.iter().any(|instruction| {
            instruction.op() == Op::MakeClosure
                || instruction.op().is_call_with_eval_flags() && instruction.direct_eval()
        })
        && !function.wide.iter().any(|instruction| {
            instruction.op() == Op::MakeClosure
                || instruction.op().is_call_with_eval_flags()
                    && ImmediateLayout::direct_eval(instruction.imm())
        });
    eligible_function
        && promoted.iter().enumerate().all(|(index, entry)| {
            entry.local < function.locals
                && entry.register == index as u16
                && entry.register < function.registers
                && function
                    .local_atoms
                    .get(usize::from(entry.local))
                    .is_some_and(|atom| {
                        !function.lexical_atoms.contains(atom)
                            && usize::try_from(*atom).is_ok_and(|atom| atom < atoms.len())
                            && &atoms[*atom as usize] != "arguments"
                            // A hidden argument slot is a parameter's own (`arguments_own_their_slots`).
                            && (!atoms[*atom as usize].starts_with('\0')
                                || entry.local < function.params
                                    && atoms[*atom as usize].starts_with(super::HIDDEN_PARAMETER_PREFIX))
                            && entry.local != function.self_binding_slot.unwrap_or(u16::MAX)
                    })
                && function
                    .selective_capture_slots
                    .as_ref()
                    .is_none_or(|slots| slots.binary_search(&entry.local).is_err())
                && (index == 0 || promoted[index - 1].local < entry.local)
        })
}

fn instruction_uses_promoted_local(
    instruction: super::WideInstruction,
    function: &super::Function,
) -> bool {
    instruction
        .addressed_local_slots()
        .any(|slot| function.promoted_register(slot).is_some())
}

#[derive(Clone, Copy)]
struct ValidationBounds {
    registers: u16,
    locals: u16,
    functions: usize,
    constants: usize,
    environment_clones: usize,
    atoms: usize,
    field_sites: usize,
    cache_sites: u16,
    method_sites: usize,
    object_sites: usize,
    regexp_literal_sites: usize,
    literal_templates: usize,
    superinstructions: usize,
    code_len: u32,
}

fn field_domains_in_bounds(instruction: super::WideInstruction, bounds: ValidationBounds) -> bool {
    InstructionField::ALL.iter().copied().all(|field| {
        let value = instruction.field_value(field);
        match instruction.op().field_layout(field) {
            FieldLayout::ResultRegister => {
                if let Some(local) = instruction.numeric_local_target() {
                    local < bounds.locals
                } else {
                    register_in_bounds(instruction.result_register(), bounds.registers, 0)
                }
            }
            FieldLayout::Register | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister => {
                register_in_bounds(value, bounds.registers, 0)
            }
            FieldLayout::RegisterWindowBase => {
                let window = instruction.register_window();
                (instruction.op() != super::Op::WasmAtomicAccess
                    || crate::wasm::atomic::AtomicOperator::from_tag(instruction.imm())
                        .is_some_and(|operator| window.count == operator.input_count()))
                    && (instruction.op() != super::Op::WasmExceptionNew
                        || window.count >= crate::wasm::tag::ExceptionInput::MIN_COUNT)
                    && crate::wasm::gc::StructConstruction::from_op(instruction.op())
                        .is_none_or(|mode| window.count >= u16::from(mode.described()))
                    && register_window_in_bounds(value, u32::from(window.count), bounds.registers)
            }
            FieldLayout::RegisterCount => true, // The associated window base owns the bounds check.
            FieldLayout::SignedConstant => true,
            FieldLayout::OptionalRegister | FieldLayout::OptionalReadRegister => instruction
                .optional_register_b()
                .is_none_or(|register| register_in_bounds(register, bounds.registers, 0)),
            FieldLayout::CacheSiteIndex => cache_in_bounds(value, bounds.cache_sites),
            FieldLayout::FieldLookupCacheSiteIndex => true,
            FieldLayout::BooleanFlag => instruction.boolean_field(field).is_some(),
            FieldLayout::FunctionIndex => usize::from(value) < bounds.functions,
            FieldLayout::ElementCount => instruction
                .constant_index()
                .checked_add(usize::from(value))
                .is_some_and(|end| end <= bounds.constants),
            FieldLayout::Operand => operand_in_bounds(
                value,
                bounds.registers,
                bounds.locals,
                bounds.constants,
                bounds.field_sites,
            ),
            FieldLayout::NumericIndexOperand => numeric_index_operand_in_bounds(value, bounds),
            FieldLayout::BinaryOperator => {
                u32::from(value) <= oxc_ast::ast::BinaryOperator::Instanceof as u32
            }
            FieldLayout::FieldBase => match instruction.field_lookup() {
                Some(super::FieldLookup::Site(site)) => site < bounds.field_sites,
                Some(super::FieldLookup::Atom {
                    atom,
                    base,
                    cache_site,
                }) => {
                    field_base_in_bounds(base.0, bounds.registers, bounds.locals)
                        && atom_in_bounds(atom, bounds.atoms)
                        && cache_in_bounds(cache_site, bounds.cache_sites)
                }
                None => false,
            },
            FieldLayout::NumericLocalTarget => {
                instruction.numeric_local_store_fields_valid()
                    && instruction
                        .numeric_local_store_target()
                        .is_none_or(|target| {
                            register_in_bounds(target.register, bounds.registers, 0)
                        })
            }
            FieldLayout::Unused
            | FieldLayout::NumericLocalStoreMarker
            | FieldLayout::ConstructArguments
            | FieldLayout::WideIndexChunk => true,
        }
    })
}

fn immediate_domains_in_bounds(
    instruction: super::WideInstruction,
    bounds: ValidationBounds,
) -> bool {
    match instruction.op().immediate_role() {
        super::ImmediateRole::ConstantIndex => instruction.constant_index() < bounds.constants,
        super::ImmediateRole::EnvironmentCloneIndex => {
            instruction.environment_clone_index() < bounds.environment_clones
        }
        super::ImmediateRole::ClosureFunctionIndex => {
            (instruction.closure_function_index() as usize) < bounds.functions
        }
        super::ImmediateRole::LocalSlot => instruction.local_slot() < usize::from(bounds.locals),
        super::ImmediateRole::AtomIndex => atom_in_bounds(instruction.atom_index(), bounds.atoms),
        super::ImmediateRole::BooleanFlag => instruction.boolean_flag().is_some(),
        super::ImmediateRole::ArrayIndex => {
            instruction.array_index() != super::ARRAY_INDEX_SENTINEL
        }
        super::ImmediateRole::BinaryOperator => {
            instruction.binary_operator() <= oxc_ast::ast::BinaryOperator::Instanceof as u32
        }
        super::ImmediateRole::AdditionOperator => {
            instruction.binary_operator() == oxc_ast::ast::BinaryOperator::Addition as u32
        }
        super::ImmediateRole::MultiplicationOperator => {
            instruction.binary_operator() == oxc_ast::ast::BinaryOperator::Multiplication as u32
        }
        super::ImmediateRole::WasmSimdOperator => {
            crate::wasm::simd::SimdOperator::from_selector(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmAtomicOperator => {
            crate::wasm::atomic::AtomicOperator::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmMemoryLoadOperator => {
            crate::wasm::memory::MemoryLoad::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmMemoryOffset => true,
        super::ImmediateRole::WasmInstanceSlot => true,
        super::ImmediateRole::WasmI32Immediate => true,
        super::ImmediateRole::WasmMemoryStoreOperator => {
            crate::wasm::memory::MemoryStore::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmI64BinaryOperator => {
            crate::wasm::integer::I64BinaryOperator::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmScalarConversionOperator => {
            crate::wasm::conversion::ScalarConversionOperator::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmF32BinaryOperator => {
            crate::wasm::float::F32BinaryOperator::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmF32UnaryOperator => {
            crate::wasm::float::F32UnaryOperator::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmF64BinaryOperator => {
            crate::wasm::float::F64BinaryOperator::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmF64UnaryOperator => {
            crate::wasm::float::F64UnaryOperator::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmI64UnaryOperator => {
            crate::wasm::integer::I64UnaryOperator::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmGcField => crate::wasm::gc::GcField::from_tag(instruction.imm()).is_some(),
        super::ImmediateRole::WasmGcTypeIndex | super::ImmediateRole::WasmExceptionFieldIndex => {
            true
        } // The attached graph owns this index domain.
        super::ImmediateRole::WasmNonNullCheck => {
            crate::wasm::reference::NonNullCheck::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmReferenceTarget => {
            crate::wasm::reference::ReferenceTarget::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmExternalConversion => {
            crate::wasm::reference::ExternalConversion::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmI31Operator => {
            crate::wasm::i31::I31Operator::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmI32UnaryOperator => {
            crate::wasm::integer::I32UnaryOperator::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::UnaryOperator => {
            instruction.unary_operator() <= oxc_ast::ast::UnaryOperator::Void as u32
                || matches!(instruction.unary_operator(), super::UNARY_TO_STRING_PRIMITIVE | super::UNARY_ADD_OPERAND_STRING)
                || super::typeof_test(instruction.unary_operator()).is_some()
                || instruction.unary_operator() == super::UNARY_TYPEOF_INDEX
        }
        super::ImmediateRole::FunctionNamePrefix => {
            instruction.function_name_prefix() <= super::FUNCTION_NAME_PREFIX_SETTER
        }
        super::ImmediateRole::PropertyDefinitionMode => {
            instruction.property_definition_mode().is_some()
        }
        super::ImmediateRole::ArrayLength => instruction.array_length() <= super::MAX_ARRAY_LENGTH,
        super::ImmediateRole::MethodSiteIndex => {
            instruction.method_site_index() < bounds.method_sites
        }
        super::ImmediateRole::ObjectSiteIndex => {
            instruction.object_site_index() < bounds.object_sites
        }
        super::ImmediateRole::RegExpLiteralSiteIndex => {
            instruction.regexp_literal_site_index() < bounds.regexp_literal_sites
        }
        super::ImmediateRole::LiteralTemplateIndex => {
            instruction.literal_template_index() < bounds.literal_templates
        }
        super::ImmediateRole::SuperinstructionIndex => {
            instruction.superinstruction_index() < bounds.superinstructions
        }
        super::ImmediateRole::JumpTarget => instruction.jump_target() < bounds.code_len,
        // Control-flow validation bounds the entries and checks that each is a jump.
        super::ImmediateRole::BranchTableCases => true,
        super::ImmediateRole::FieldLookup
        | super::ImmediateRole::WasmSignatureIndex
        | super::ImmediateRole::WasmFunctionIndex
        | super::ImmediateRole::LayoutEncoded
        | super::ImmediateRole::TemplateSiteIndex
        | super::ImmediateRole::Unused
        | super::ImmediateRole::WideInstructionIndex => true,
    }
}

fn packed_layout_domains_in_bounds(
    instruction: super::WideInstruction,
    bounds: ValidationBounds,
) -> bool {
    match instruction.op().immediate_layout() {
        ImmediateLayout::CaptureDepthAndSlot => {
            usize::from(instruction.capture_depth()) < bounds.functions
        }
        ImmediateLayout::CallWindow
        | ImmediateLayout::CallWindowWithEvalFlags
        | ImmediateLayout::SingleArgumentCallWindowWithEvalFlags => {
            let window = instruction.call_window();
            let layout = instruction.op().immediate_layout();
            (layout != ImmediateLayout::SingleArgumentCallWindowWithEvalFlags
                || window.count == super::SINGLE_ARGUMENT_CALL_ARGUMENT_COUNT)
                && register_window_in_bounds(
                    window.base,
                    u32::from(window.count),
                    bounds.registers,
                )
        }
        ImmediateLayout::ConstructCountAndFlags => match instruction.construct_arguments() {
            super::ConstructArguments::Registers(window) => {
                register_window_in_bounds(window.base, u32::from(window.count), bounds.registers)
            }
            super::ConstructArguments::Array(register) => {
                register_in_bounds(register, bounds.registers, 0)
            }
        },
        ImmediateLayout::RegisterPair => {
            let (first, second) = instruction.register_pair();
            register_in_bounds(first, bounds.registers, 0)
                && register_in_bounds(second, bounds.registers, 0)
        }
        ImmediateLayout::Scalar => true,
    }
}

fn object_site_instruction_valid(
    instruction: super::WideInstruction,
    object_sites: &[super::ObjectSite],
    registers: u16,
) -> bool {
    match instruction.op() {
        Op::MakeObject2 => object_sites
            .get(instruction.object_site_index())
            .is_some_and(|site| site.atoms.len() == super::INLINE_OBJECT_SITE_ATOMS),
        Op::MakeObjectLiteral => {
            let Some(site) = object_sites.get(instruction.object_site_index()) else {
                return false;
            };
            let window = instruction.register_window();
            let mut atoms = FxHashSet::default();
            usize::from(window.count) > super::INLINE_OBJECT_SITE_ATOMS
                && site.atoms.len() == usize::from(window.count)
                && site.atoms.iter().all(|atom| atoms.insert(*atom))
                && register_window_in_bounds(window.base, u32::from(window.count), registers)
        }
        _ => true,
    }
}

impl ResidualProgram {
    /// Validate all cross-table references before a VM can observe the program.
    /// `validate`, unless the program is proven valid (`ValidationProof`).
    pub(crate) fn validate_unless_proven(&self) -> Result<(), String> {
        if self.validated.holds() {
            return Ok(());
        }
        self.validate()
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if let Some(source) = &self.source { source.validate_call_sites()?; }
        if self.functions.is_empty() && self.kind != super::ProgramKind::Wasm {
            return Err("program has no entry function".into());
        }
        // A Wasm module whose functions lower on their first calls validates the records it
        // has: lowering a function only to check it would keep every record it saves.
        let lazy_wasm = self.wasm_lazy.is_some();
        // Validation reads every record, and the cross-function checks read their parents'.
        if !lazy_wasm {
            self.compile_released();
        }
        let scope_unsafe = if lazy_wasm {
            BitSet::new(self.functions.len())
        } else {
            selective_capture_scope_unsafe(&self.functions, &self.atoms)
        };
        // Captured name scopes are shared by sibling functions: each is checked once per
        // position in a parent chain.
        let mut checked_captured_names = rustc_hash::FxHashSet::default();
        for (index, function) in self.functions.iter().enumerate() {
            if lazy_wasm && function.record().is_none() {
                continue;
            }
            self.validate_function(index, function, scope_unsafe.contains(index), &mut checked_captured_names)?;
        }
        self.validate_selective_capture_owners()?;
        self.validate_method_sites()?;
        self.validate_field_sites()?;
        self.validate_object_sites()?;
        self.validate_literal_sites()
    }

    /// Checks function `index`: its facts and layouts, its tables, then its code.
    fn validate_function(
        &self,
        index: usize,
        function: &super::Function,
        scope_unsafe: bool,
        checked_captured_names: &mut rustc_hash::FxHashSet<(usize, u16, Option<u32>)>,
    ) -> Result<(), String> {
        let code_len = function.code.len() as u32;
        self.validate_function_facts(index, function, scope_unsafe)?;
        self.validate_function_tables(index, function, code_len, checked_captured_names)?;
        self.validate_function_code(index, function, code_len)
    }

    /// Checks what function `index` claims about itself: its code, registers, derived facts,
    /// bindings, atoms and selective capture layout.
    fn validate_function_facts(&self, index: usize, function: &super::Function, scope_unsafe: bool) -> Result<(), String> {
        self.validate_code_shape(index, function)?;
        self.validate_liveness_claims(index, function)?;
        validate_self_binding(index, function)?;
        self.validate_function_names(index, function)?;
        self.validate_selective_capture(index, function, scope_unsafe)?;
        if !promoted_local_layout_is_valid(function, &self.atoms) {
            return Err(format!(
                "function {index} has an invalid promoted-local layout"
            ));
        }
        Ok(())
    }

    /// Checks that function `index` has code that cannot fall off its end, a register count
    /// within the register mask, and only the facts its code derives.
    fn validate_code_shape(&self, index: usize, function: &super::Function) -> Result<(), String> {
        if function.code.is_empty() || !super::control_flow::is_bounded(function) {
            return Err(format!("function {index} can fall off its code"));
        }
        if function.registers > REGISTER_MASK {
            return Err(format!("function {index} has too many registers"));
        }
        // A call plan decides how an activation is laid out, so a seeded one is the
        // function's own derivation, as is every other fact an encoding keeps.
        if !function.derived_facts_hold(self) {
            return Err(format!("function {index} carries facts its code does not derive"));
        }
        Ok(())
    }

    /// Checks the dead-call-result and entry-liveness claims of function `index` against the
    /// liveness the compiler derives.
    fn validate_liveness_claims(&self, index: usize, function: &super::Function) -> Result<(), String> {
        // A lane lets the callee of a dead result skip building it, so each claim is the
        // compiler's own derivation (`liveness::register_roots`).
        // An activation fills only the registers below the live entry bound, so that bound
        // is the compiler's own derivation too.
        if function.register_roots.dead_call_results().is_empty()
            && function.register_roots.entry_fill(function.registers) >= function.registers
        {
            return Ok(());
        }
        let derived = crate::compile::liveness::register_roots(function, self, &self.field_sites, &self.superinstructions);
        if derived.dead_call_results() != function.register_roots.dead_call_results() {
            return Err(format!("function {index} claims dead call results its liveness does not prove"));
        }
        if derived.entry_fill(function.registers) > function.register_roots.entry_fill(function.registers) {
            return Err(format!("function {index} claims an entry liveness its liveness does not prove"));
        }
        Ok(())
    }

    /// Checks the arrow, global and environment atoms, and the parent of function `index`.
    fn validate_function_names(&self, index: usize, function: &super::Function) -> Result<(), String> {
        if function.is_arrow && (function.constructible || function.is_class_constructor) {
            return Err(format!("function {index} has invalid arrow-function facts"));
        }
        if function
            .global_var_atoms()
            .iter()
            .any(|atom| !atom_in_bounds(*atom, self.atoms.len()))
        {
            return Err(format!("function {index} has an invalid global var atom"));
        }
        if function
            .environment_atoms
            .iter()
            .any(|atom| !atom_in_bounds(*atom, self.atoms.len()))
        {
            return Err(format!("function {index} has an invalid environment atom"));
        }
        if function
            .parent
            .is_some_and(|p| p as usize >= self.functions.len())
        {
            return Err(format!("function {index} has an invalid parent"));
        }
        Ok(())
    }

    /// The module link plan and imports that the module body of function `index` binds, when
    /// it is that body.
    fn module_body(&self, index: usize, function: &super::Function) -> Option<(&super::ModuleLinkPlan, &[super::ModuleImportBinding])> {
        (self.kind == super::ProgramKind::Module && index == 0 && function.parent.is_none())
            .then_some(self.module_link_plan.as_ref())
            .flatten()
            .map(|plan| (plan, &self.module_imports[..]))
    }

    /// Checks the selective capture layout of function `index`, and that the function may
    /// have one.
    fn validate_selective_capture(&self, index: usize, function: &super::Function, scope_unsafe: bool) -> Result<(), String> {
        let module_body = self.module_body(index, function);
        if let Some(captured) = &function.selective_capture_slots
            && ((module_body.is_none() && (index == 0 || function.parent.is_none()))
                || captured.iter().any(|slot| *slot >= function.locals)
                || captured.windows(NEIGHBOURS).any(|pair| pair[0] >= pair[1]))
        {
            return Err(format!(
                "function {index} has an invalid selective capture layout"
            ));
        }
        if function.selective_capture_slots.is_some()
            && (scope_unsafe
                || !selective_capture_layout_is_eligible(function, index, module_body, &self.atoms))
        {
            return Err(format!(
                "function {index} has an ineligible selective capture layout"
            ));
        }
        Ok(())
    }

    /// Checks the instance initializer, name bindings, source positions, binding sites and
    /// environment clones of function `index`.
    fn validate_function_tables(
        &self,
        index: usize,
        function: &super::Function,
        code_len: u32,
        checked_captured_names: &mut rustc_hash::FxHashSet<(usize, u16, Option<u32>)>,
    ) -> Result<(), String> {
        self.validate_instance_initializer(index, function)?;
        if !name_bindings_are_valid(
            function,
            &self.functions,
            self.atoms.len(),
            checked_captured_names,
        ) {
            return Err(format!("function {index} has invalid name bindings"));
        }
        validate_source_positions(index, function, code_len)?;
        self.validate_binding_sites(index, function, code_len)?;
        if !function.environment_clones.valid(function.locals) {
            return Err(format!(
                "function {index} has invalid environment clone slots"
            ));
        }
        Ok(())
    }

    /// Checks that the instance initializer of function `index`, when it has one, is a
    /// class-initializer sibling of the same parent.
    fn validate_instance_initializer(&self, index: usize, function: &super::Function) -> Result<(), String> {
        let Some(initializer) = function.instance_initializer else {
            return Ok(());
        };
        let valid = self
            .functions
            .get(initializer as usize)
            .is_some_and(|plan| {
                function.derived_constructor
                    && initializer as usize != index
                    && plan.parent == function.parent
                    && plan.class_initializer
                    && !plan.constructible
                    && plan.instance_initializer.is_none()
            });
        if !valid {
            return Err(format!(
                "function {index} has an invalid instance initializer"
            ));
        }
        Ok(())
    }

    /// Checks the binding-site metadata of function `index`: resume points in order inside the
    /// code, and bindings sorted by atom and valid.
    fn validate_binding_sites(&self, index: usize, function: &super::Function, code_len: u32) -> Result<(), String> {
        if function
            .binding_sites
            .windows(NEIGHBOURS)
            .any(|pair| pair[0].resume_pc >= pair[1].resume_pc)
            || function.binding_sites.iter().any(|site| {
                site.resume_pc == 0
                    || site.resume_pc > code_len
                    || site
                        .bindings
                        .windows(NEIGHBOURS)
                        .any(|pair| pair[0].atom >= pair[1].atom)
                    || site.bindings.iter().any(|binding| {
                        !eval_binding_is_valid(
                            binding,
                            function,
                            &self.functions,
                            self.atoms.len(),
                        )
                    })
            })
        {
            return Err(format!(
                "function {index} has invalid binding-site metadata"
            ));
        }
        Ok(())
    }

    /// Checks the frame of function `index` and its code: each instruction, then each handler.
    fn validate_function_code(&self, index: usize, function: &super::Function, code_len: u32) -> Result<(), String> {
        let bounds = self.validation_bounds(function, code_len);
        validate_frame_metadata(index, function, code_len)?;
        let plain_local_slots = if self.kind == super::ProgramKind::Wasm {
            crate::bitset::BitSet::from_fn(usize::from(function.locals), |_| true)
        } else {
            function.plain_local_slots(&self.superinstructions)
        };
        for packed in &function.code {
            let Some(instruction) = instruction_at(function, *packed) else {
                return Err(format!("function {index} wide instruction is invalid"));
            };
            self.validate_instruction(index, function, instruction, &plain_local_slots, bounds)?;
        }
        for handler in &function.handlers {
            validate_handler(index, function, handler, code_len)?;
        }
        Ok(())
    }

    /// The table lengths and frame sizes that the operands of function `index` must stay within.
    fn validation_bounds(&self, function: &super::Function, code_len: u32) -> ValidationBounds {
        ValidationBounds {
            registers: function.registers,
            locals: function.locals,
            functions: self.functions.len(),
            constants: self.constants.len(),
            environment_clones: function.environment_clones.len(),
            atoms: self.atoms.len(),
            field_sites: self.field_sites.len(),
            cache_sites: self.cache_sites.get(),
            method_sites: self.method_sites.len(),
            object_sites: self.object_sites.len(),
            regexp_literal_sites: self.regexp_literal_sites.len(),
            literal_templates: self.literal_templates.len(),
            superinstructions: self.superinstructions.len(),
            code_len,
        }
    }

    /// Checks one decoded instruction of function `index`: its local accesses, its capture
    /// accesses, its binary target and its operands.
    fn validate_instruction(
        &self,
        index: usize,
        function: &super::Function,
        instruction: super::WideInstruction,
        plain_local_slots: &crate::bitset::BitSet,
        bounds: ValidationBounds,
    ) -> Result<(), String> {
        validate_local_access(index, function, instruction, plain_local_slots)?;
        validate_capture_access(index, function, instruction)?;
        validate_binary_target(index, function, instruction, plain_local_slots)?;
        self.validate_operand_domains(index, function, instruction, bounds)?;
        self.validate_wasm_constants(index, instruction)
    }

    /// Checks that the Wasm memory offset and SIMD shuffle constants of an instruction are
    /// well formed.
    fn validate_wasm_constants(&self, index: usize, instruction: super::WideInstruction) -> Result<(), String> {
        if instruction.op() == super::Op::WasmMemoryAddress
            && !matches!(
                self.constants.get(instruction.constant_index()),
                Some(super::Constant::WasmBits64(_))
            )
        {
            return Err(format!(
                "function {index} has an invalid Wasm memory offset constant"
            ));
        }
        if instruction.op() == super::Op::WasmSimdShuffle
            && !matches!(self.constants.get(instruction.constant_index()), Some(super::Constant::WasmV128(indices)) if crate::wasm::simd::shuffle_indices_valid(indices))
        {
            return Err(format!("function {index} has invalid SIMD shuffle indices"));
        }
        Ok(())
    }

    /// Whether an atom and a cache of a site are both in bounds.
    fn atom_cache_in_bounds(&self, (atom, cache): (u32, u16)) -> bool {
        atom_in_bounds(atom, self.atoms.len()) && cache_in_bounds(cache, self.cache_sites.get())
    }

    /// Checks the method sites: atoms, caches and argument ranges in bounds.
    fn validate_method_sites(&self) -> Result<(), String> {
        for site in &self.method_sites {
            if !atom_in_bounds(site.atom, self.atoms.len())
                || !cache_in_bounds(site.cache, self.cache_sites.get())
                || (site.argument_start as usize)
                    .checked_add(site.argument_count as usize)
                    .is_none_or(|end| end > self.method_arguments.len())
                || !site.receiver_path.is_none_or(|pair| self.atom_cache_in_bounds(pair))
            {
                return Err("invalid method site".into());
            }
        }
        Ok(())
    }

    /// Checks the field sites: positions, base register, atoms and caches in bounds.
    fn validate_field_sites(&self) -> Result<(), String> {
        for site in &self.field_sites {
            if site.sources.iter().flatten().any(|(line, column)| *line == 0 || *column == 0)
                || !field_base_in_bounds(site.base.0, REGISTER_MASK, 0)
                || !atom_in_bounds(site.first.0, self.atoms.len())
                || !cache_in_bounds(site.first.1, self.cache_sites.get())
                || !site.second.is_none_or(|pair| self.atom_cache_in_bounds(pair))
                || !site.sink.is_none_or(|pair| self.atom_cache_in_bounds(pair))
            {
                return Err("invalid field site".into());
            }
        }
        Ok(())
    }

    /// Checks that every object site names only atoms of the program.
    fn validate_object_sites(&self) -> Result<(), String> {
        for site in &self.object_sites {
            if site
                .atoms
                .iter()
                .any(|atom| !atom_in_bounds(*atom, self.atoms.len()))
            {
                return Err("invalid object site".into());
            }
        }
        Ok(())
    }

    /// Checks the literal templates, the RegExp literal sites and the method argument registers.
    fn validate_literal_sites(&self) -> Result<(), String> {
        for template in &self.literal_templates {
            if !self.literal_template_is_valid(template) {
                return Err("invalid literal template".into());
            }
        }
        for site in &self.regexp_literal_sites {
            if !matches!(
                self.constants.get(site.pattern_constant as usize),
                Some(super::Constant::String(_))
            ) || !matches!(
                self.constants.get(site.flags_constant as usize),
                Some(super::Constant::String(_))
            ) {
                return Err("invalid RegExp literal site".into());
            }
        }
        if self
            .method_arguments
            .iter()
            .any(|register: &Register| *register > REGISTER_MASK)
        {
            return Err("invalid method argument register".into());
        }
        Ok(())
    }

    /// Checks an instruction's result flags, unused operands and operand domains against the
    /// bounds and object sites of its program.
    fn validate_operand_domains(
        &self,
        index: usize,
        function: &super::Function,
        instruction: super::WideInstruction,
        bounds: ValidationBounds,
    ) -> Result<(), String> {
        if !instruction.result_flags_valid() {
            return Err(format!("function {index} result flags are invalid"));
        }
        if !instruction.unused_operands_are_zero() {
            return Err(format!(
                "function {index} {:?} has nonzero unused operands",
                instruction.op()
            ));
        }
        if !field_domains_in_bounds(instruction, bounds)
            || !immediate_domains_in_bounds(instruction, bounds)
            || !packed_layout_domains_in_bounds(instruction, bounds)
            || !object_site_instruction_valid(instruction, &self.object_sites, function.registers)
        {
            return Err(format!(
                "function {index} {:?} has an out-of-domain operand",
                instruction.op()
            ));
        }
        Ok(())
    }

    /// Whether `steps` name only this program's constants and object sites and, run over
    /// a stack, never take more values than it holds and leave exactly one.
    fn literal_template_is_valid(&self, steps: &[super::LiteralStep]) -> bool {
        let mut depth = 0usize;
        for step in steps {
            let (taken, valid) = match *step {
                super::LiteralStep::Constant(index) => {
                    (0, (index as usize) < self.constants.len())
                }
                super::LiteralStep::Int(_) => (0, true),
                super::LiteralStep::Array(count) => (count as usize, true),
                super::LiteralStep::Object(site) => match self.object_sites.get(site as usize) {
                    Some(site) => (site.atoms.len(), true),
                    None => (0, false),
                },
                super::LiteralStep::Json(start) => (0, self.data_literal_text(start).is_some()),
            };
            if !valid || taken > depth {
                return false;
            }
            depth = depth - taken + 1;
        }
        depth == 1
    }

    fn validate_selective_capture_owners(&self) -> Result<(), String> {
        // JavaScript captures are addressed through the function-parent chain.
        // Wasm uses the same opcodes for slots in its separate instance
        // environment, whose layout is validated by the Wasm module loader.
        if self.kind == super::ProgramKind::Wasm {
            return Ok(());
        }
        for (function_id, function) in self.functions.iter().enumerate() {
            for instruction in function
                .code
                .iter()
                .filter_map(|packed| instruction_at(function, *packed))
                .chain(function.wide.iter().copied())
            {
                if !matches!(
                    instruction.op(),
                    super::Op::LoadCapture | super::Op::StoreCapture
                ) {
                    continue;
                }
                let mut owner = function_id;
                let depth = usize::from(instruction.capture_depth()) + 1;
                for _ in 0..depth {
                    let Some(parent) = self.functions[owner].parent else {
                        return Err(format!(
                            "function {function_id} captures outside its ancestor chain"
                        ));
                    };
                    owner = parent as usize;
                }
                if usize::from(instruction.capture_slot())
                    >= usize::from(self.functions[owner].locals)
                {
                    return Err(format!(
                        "function {function_id} captures an out-of-range slot in function {owner}"
                    ));
                }
                if let Some(captured) = &self.functions[owner].selective_capture_slots
                    && captured.binary_search(&instruction.capture_slot()).is_err()
                {
                    return Err(format!(
                        "function {function_id} captures a slot omitted by function {owner}'s selective layout"
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Checks that the self-binding slot of function `index`, when it has one, is a local that the
/// function's own name binds.
fn validate_self_binding(index: usize, function: &super::Function) -> Result<(), String> {
    if function.self_binding_slot.is_some_and(|slot| slot >= function.locals) {
        return Err(format!("function {index} has an invalid self-binding slot"));
    }
    if let Some(slot) = function.self_binding_slot
        && !function.name_bindings.direct().iter().any(|binding| {
            binding.kind == super::LexicalBindingKind::FunctionName
                && matches!(
                    binding.location,
                    super::EvalBindingLocation::Local(binding_slot)
                        if binding_slot == slot
                )
        })
    {
        return Err(format!("function {index} has an unbound self-binding slot"));
    }
    Ok(())
}

/// Checks the source positions of function `index`: ordered by pc within its code, with
/// positive lines and columns.
fn validate_source_positions(index: usize, function: &super::Function, code_len: u32) -> Result<(), String> {
    let positions = function.source_positions.iter();
    if positions
        .clone()
        .zip(positions.clone().skip(1))
        .any(|(first, second)| first.pc >= second.pc)
        || positions.clone().any(|position| {
            position.pc >= code_len || position.line == 0 || position.column == 0
        })
        || positions.clone().last() != function.source_positions.last()
    {
        return Err(format!("function {index} has invalid source positions"));
    }
    Ok(())
}

/// Checks the frame metadata of function `index`: the argument slot, the parameter boundary
/// and the undefined register lie within its frame and code.
fn validate_frame_metadata(index: usize, function: &super::Function, code_len: u32) -> Result<(), String> {
    if function
        .arguments_slot
        .is_some_and(|slot| slot >= function.locals)
        || function.rest && function.simple_parameters
    {
        return Err(format!("function {index} has invalid argument metadata"));
    }
    if function.parameter_end_pc > code_len
        || function.parameter_end_pc != 0 && !function.is_generator
    {
        return Err(format!(
            "function {index} has an invalid parameter boundary"
        ));
    }
    if function
        .undefined_register
        .is_some_and(|register| register >= function.registers)
    {
        return Err(format!(
            "function {index} undefined register is out of bounds"
        ));
    }
    Ok(())
}

/// Checks that an instruction of function `index` is not a nested wide marker, that it does
/// not reach a promoted binding through its local slot, and that its plain-local operations
/// are proven plain.
fn validate_local_access(
    index: usize,
    function: &super::Function,
    instruction: super::WideInstruction,
    plain_local_slots: &crate::bitset::BitSet,
) -> Result<(), String> {
    if instruction.op().is_wide_marker() {
        return Err(format!("function {index} contains nested wide instruction"));
    }
    if instruction_uses_promoted_local(instruction, function) {
        return Err(format!(
            "function {index} accesses a promoted binding through its local slot"
        ));
    }
    if matches!(
        instruction.op(),
        super::Op::LoadLocalPlain | super::Op::StoreLocalPlain
    ) && !plain_local_slots.contains(instruction.local_slot())
    {
        return Err(format!(
            "function {index} has an unproven plain-local operation"
        ));
    }
    Ok(())
}

/// The width of the neighbouring pairs a sorted list is checked by.
const NEIGHBOURS: usize = 2;

/// Whether an instruction addresses a local slot that the selective capture layout captures.
fn addresses_captured_slot(instruction: super::WideInstruction, captured: &[u16]) -> bool {
    let local_slot = matches!(
        instruction.op(),
        super::Op::LoadLocal
            | super::Op::StoreLocal
            | super::Op::LoadLocalPlain
            | super::Op::StoreLocalPlain
            | super::Op::LoadEnvLocal
            | super::Op::StoreEnvLocal
    )
    .then(|| instruction.local_slot());
    local_slot.is_some_and(|slot| {
        u16::try_from(slot).is_ok_and(|slot| captured.binary_search(&slot).is_ok())
    })
}

/// Whether a numeric local fast path of an instruction reaches an environment-owned slot.
fn numeric_path_reaches_capture(
    function: &super::Function,
    instruction: super::WideInstruction,
    captured: &[u16],
    slot_is_captured: bool,
) -> bool {
    let numeric = function.dispatch == super::DispatchClass::Numeric;
    instruction
        .numeric_local_target()
        .is_some_and(|target| captured.binary_search(&target).is_ok())
        || numeric
            && ((instruction.op() == super::Op::StoreLocal && slot_is_captured)
                || instruction.op() == super::Op::LoadLocal
                    && instruction.numeric_local_store_target().is_some()
                    && slot_is_captured)
        || numeric && instruction.op() == super::Op::GetIndex && index_operand_is_captured(instruction, captured)
}

/// Whether an index operand of an instruction is a local the capture layout captures.
fn index_operand_is_captured(instruction: super::WideInstruction, captured: &[u16]) -> bool {
    [instruction.operand_b(), instruction.operand_c()].into_iter().any(|operand| {
        operand.kind() == Some(OperandKind::Local) && captured.binary_search(&operand.payload()).is_ok()
    })
}

/// Checks an instruction of function `index` against its selective capture layout:
/// environment operations reach only captured slots, and no numeric local fast path reaches
/// an environment-owned slot.
fn validate_capture_access(index: usize, function: &super::Function, instruction: super::WideInstruction) -> Result<(), String> {
    let Some(captured) = &function.selective_capture_slots else {
        return Ok(());
    };
    let slot_is_captured = addresses_captured_slot(instruction, captured);
    let environment_op = matches!(
        instruction.op(),
        super::Op::LoadEnvLocal | super::Op::StoreEnvLocal
    );
    if environment_op && !slot_is_captured {
        return Err(format!(
            "function {index} has an environment-local operation for an uncaptured slot"
        ));
    }
    if numeric_path_reaches_capture(function, instruction, captured, slot_is_captured) {
        return Err(format!(
            "function {index} has a numeric local fast path for an environment-owned slot"
        ));
    }
    Ok(())
}

/// Checks that a binary operation of function `index` targets a plain local, unless the
/// function dispatches numerically.
fn validate_binary_target(
    index: usize,
    function: &super::Function,
    instruction: super::WideInstruction,
    plain_local_slots: &crate::bitset::BitSet,
) -> Result<(), String> {
    if instruction.op() == super::Op::Binary
        && instruction.numeric_local_target().is_some_and(|slot| {
            function.dispatch != super::DispatchClass::Numeric
                && !plain_local_slots.contains(usize::from(slot))
        })
    {
        return Err(format!(
            "function {index} has an unproven plain-local binary target"
        ));
    }
    Ok(())
}

/// Checks a handler of function `index`: its range and targets lie in the code, and its slots
/// in the frame.
fn validate_handler(index: usize, function: &super::Function, handler: &super::Handler, code_len: u32) -> Result<(), String> {
    if handler.start > handler.end
        || handler.end > code_len
        || handler.target >= code_len
        || handler
            .return_target
            .is_some_and(|target| target >= code_len)
        || handler.return_target.is_some() != handler.return_slot.is_some()
    {
        return Err(format!("function {index} has an invalid handler"));
    }
    if handler.slot.is_some_and(|slot| match slot {
        super::ThrownValueSlot::Local(slot) => slot >= function.locals,
        super::ThrownValueSlot::Register(register) => register >= function.registers,
    }) {
        return Err(format!("function {index} handler slot is out of bounds"));
    }
    if handler
        .return_slot
        .is_some_and(|slot| slot >= function.locals)
    {
        return Err(format!(
            "function {index} handler return slot is out of bounds"
        ));
    }
    Ok(())
}

fn selective_capture_scope_unsafe(
    functions: &[super::FunctionSlot],
    atoms: &super::AtomTable,
) -> BitSet {
    let is_with_enter =
        |atom: usize| atom < atoms.len() && &atoms[atom] == super::WITH_ENTER_BINDING;
    let inherited_dynamic_scope = inherited_dynamic_scopes(functions);
    let mut unsafe_layout = BitSet::new(functions.len());
    for (id, function) in functions.iter().enumerate() {
        if !has_dynamic_scope_access(function)
            && !inherited_dynamic_scope.contains(id)
            && !function.contains_with_statement(is_with_enter)
        {
            continue;
        }
        let mut current = Some(id);
        while let Some(owner) = current {
            unsafe_layout.insert(owner);
            current = functions[owner].parent.map(|parent| parent as usize);
        }
    }
    unsafe_layout
}

/// The functions that inherit a dynamic scope from an ancestor: one whose parent holds a binding
/// site, or inherits one itself.
fn inherited_dynamic_scopes(functions: &[super::FunctionSlot]) -> BitSet {
    let mut inherited = BitSet::new(functions.len());
    for (id, function) in functions.iter().enumerate() {
        if let Some(parent) = function.parent.map(|parent| parent as usize)
            && (inherited.contains(parent) || !functions[parent].binding_sites.is_empty())
        {
            inherited.insert(id);
        }
    }
    inherited
}

fn has_dynamic_scope_access(function: &super::Function) -> bool {
    function.inherited_with_scope
        || !function.binding_sites.is_empty()
        || function
            .code
            .iter()
            .filter_map(|packed| instruction_at(function, *packed))
            .chain(function.wide.iter().copied())
            .any(super::WideInstruction::resolves_dynamic_scope)
}

fn selective_capture_layout_is_eligible(
    function: &super::Function,
    index: usize,
    module_body: Option<(&super::ModuleLinkPlan, &[super::ModuleImportBinding])>,
    atoms: &super::AtomTable,
) -> bool {
    let has_closure = function
        .code
        .iter()
        .filter_map(|packed| instruction_at(function, *packed))
        .chain(function.wide.iter().copied())
        .any(|instruction| instruction.op() == super::Op::MakeClosure);
    let captured = function
        .selective_capture_slots
        .as_deref()
        .unwrap_or_default();
    // Locals the runtime resolves by name must live in the environment.
    let local_atoms_are_static = function.local_atoms.len() == usize::from(function.locals)
        && function.local_atoms.iter().enumerate().all(|(slot, atom)| {
            usize::try_from(*atom).is_ok_and(|atom| {
                atom < atoms.len()
                    && (!super::local_name_requires_environment(&atoms[atom])
                        || u16::try_from(slot)
                            .is_ok_and(|slot| captured.binary_search(&slot).is_ok()))
            })
        });
    let mapped_parameter_is_captured =
        function.arguments_alias_parameters() && captured.iter().any(|slot| *slot < function.params);
    // A mapped `arguments` object shares the captured parameters, so it lives in the
    // environment with them; a body that keeps no `arguments` binding maps nothing.
    let mapped_arguments_are_captured = !mapped_parameter_is_captured
        || function
            .arguments_slot
            .is_none_or(|slot| captured.binary_search(&slot).is_ok());

    // A module body (`module_body` given: its link plan and imports) splits whether or not it makes closures, keeping
    // its module-scope bindings in the module environment; a nested function splits only
    // around closures.
    let activation_splits = if let Some((plan, imports)) = module_body {
        function
            .module_environment_slots(plan, imports, |atom| {
                usize::try_from(atom).ok().filter(|atom| *atom < atoms.len()).map(|atom| &atoms[atom])
            })
            .all(|slot| captured.binary_search(&slot).is_ok())
    } else {
        index != 0 && function.parent.is_some() && has_closure
    };
    activation_splits
        && !function.is_async
        && !function.is_generator
        && function.construction_is_private()
        // The compiler splits a rest list that binds directly and a list binding its names
        // apart too (`selective_layout_is_safe`).
        && function.arguments_own_their_slots(|atom| atoms.get(atom))
        && local_atoms_are_static
        && mapped_arguments_are_captured
}

#[cfg(test)]
#[path = "validate_tests.rs"]
mod tests;
