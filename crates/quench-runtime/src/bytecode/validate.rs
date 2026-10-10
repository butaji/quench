use super::control_flow::instruction_at;
use super::{
    FieldBase, FieldLayout, ImmediateLayout, ImmediateRole, InstructionField, Op, Operand,
    OperandKind, REGISTER_MASK, Register, ResidualProgram,
};
use rustc_hash::FxHashSet;

fn register_in_bounds(register: u16, limit: u16, flags: u16) -> bool {
    register & !(REGISTER_MASK | flags) == 0 && register & REGISTER_MASK < limit
}

fn field_base_in_bounds(base: u16, limit: u16) -> bool {
    base == FieldBase::THIS.0 || base == FieldBase::NESTED || base < REGISTER_MASK && base < limit
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
    functions: &[super::Function],
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
            parent.and_then(|id| functions.get(id as usize))
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

fn cache_in_bounds(cache: u16, caches: u16) -> bool {
    cache < caches
}

fn register_window_in_bounds(base: u16, count: u32, registers: u16) -> bool {
    u32::from(base)
        .checked_add(count)
        .is_some_and(|end| end <= u32::from(registers))
}

fn promoted_local_layout_is_valid(
    function: &super::Function,
    atoms: &super::AtomTable,
) -> bool {
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
                Op::LoadName | Op::LoadNameCall | Op::LoadNameTypeof | Op::StoreName | Op::DeleteName
            ) && promoted.iter().any(|entry| {
                function.local_atoms.get(usize::from(entry.local)) == Some(&instruction.imm())
            })
        });
    let eligible_function = function.parent.is_some()
        && !function.is_async
        && !function.is_generator
        && !function.is_class_constructor
        && !function.derived_constructor
        && !function.class_field_initializer
        && function.simple_parameters
        && !function.rest
        && function.arguments_slot.is_none()
        && !function.inherited_with_scope
        && function.binding_sites.is_empty()
        && !has_dynamic_local_resolution
        && !function.code.iter().any(|instruction| {
            matches!(instruction.op(), Op::MakeClosure | Op::ResolveName | Op::CallDirectEvalArray)
                || instruction.op() == Op::Call && instruction.direct_eval()
        })
        && !function.wide.iter().any(|instruction| {
            matches!(instruction.op(), Op::MakeClosure | Op::ResolveName | Op::CallDirectEvalArray)
                || instruction.op() == Op::Call && ImmediateLayout::direct_eval(instruction.imm())
        });
    eligible_function
        && promoted.iter().enumerate().all(|(index, entry)| {
            entry.local < function.params
                && entry.local < function.locals
                && entry.register == index as u16
                && entry.register < function.registers
                && function.local_atoms.get(usize::from(entry.local)).is_some_and(|atom| {
                    !function.lexical_atoms.contains(atom)
                        && usize::try_from(*atom).is_ok_and(|atom| atom < atoms.len())
                        && &atoms[*atom as usize] != "arguments"
                        && !atoms[*atom as usize].starts_with('\0')
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
    let local_slot = (instruction.op().immediate_role() == ImmediateRole::LocalSlot)
        .then(|| instruction.local_slot())
        .and_then(|slot| u16::try_from(slot).ok());
    local_slot.is_some_and(|slot| function.promoted_register(slot).is_some())
        || instruction
            .numeric_local_target()
            .is_some_and(|slot| function.promoted_register(slot).is_some())
        || InstructionField::ALL.iter().copied().any(|field| {
            matches!(
                instruction.op().field_layout(field),
                FieldLayout::Operand | FieldLayout::NumericIndexOperand
            ) && Operand(instruction.field_value(field))
                .kind()
                .is_some_and(|kind| kind == OperandKind::Local)
                && function
                    .promoted_register(Operand(instruction.field_value(field)).payload())
                    .is_some()
        })
}

fn instruction_writes_promoted_register(
    instruction: super::WideInstruction,
    function: &super::Function,
) -> bool {
    InstructionField::ALL.iter().copied().any(|field| {
        let layout = instruction.op().field_layout(field);
        let register = match layout {
            FieldLayout::ResultRegister if instruction.numeric_local_target().is_none() => {
                Some(instruction.result_register())
            }
            FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister => {
                Some(instruction.field_value(field))
            }
            FieldLayout::OptionalRegister => instruction.optional_register_b(),
            _ => None,
        };
        register.is_some_and(|register| {
            function
                .local_registers
                .iter()
                .any(|entry| entry.register == register)
        })
    })
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
            FieldLayout::OptionalRegister => instruction
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
                    field_base_in_bounds(base.0, bounds.registers)
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
        super::ImmediateRole::WasmMemoryStoreOperator => {
            crate::wasm::memory::MemoryStore::from_tag(instruction.imm()).is_some()
        }
        super::ImmediateRole::WasmI32BinaryOperator => {
            crate::wasm::integer::I32BinaryOperator::from_tag(instruction.imm()).is_some()
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
        super::ImmediateRole::WasmStructFieldIndex => true, // The live struct owns field bounds.
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
        }
        super::ImmediateRole::FunctionNamePrefix => {
            instruction.function_name_prefix() <= super::FUNCTION_NAME_PREFIX_SETTER
        }
        super::ImmediateRole::PropertyDefinitionMode => {
            instruction.property_definition_mode().is_some()
        }
        super::ImmediateRole::ArrayLength => instruction.array_length() <= super::MAX_ARRAY_LENGTH,
        super::ImmediateRole::MethodSiteIndex => {
            (instruction.method_site_index() as usize) < bounds.method_sites
        }
        super::ImmediateRole::ObjectSiteIndex => {
            (instruction.object_site_index() as usize) < bounds.object_sites
        }
        super::ImmediateRole::RegExpLiteralSiteIndex => {
            instruction.regexp_literal_site_index() < bounds.regexp_literal_sites
        }
        super::ImmediateRole::SuperinstructionIndex => {
            (instruction.superinstruction_index() as usize) < bounds.superinstructions
        }
        super::ImmediateRole::JumpTarget => instruction.jump_target() < bounds.code_len,
        super::ImmediateRole::FieldLookup
        | super::ImmediateRole::WasmSignatureIndex
        | super::ImmediateRole::WasmFunctionIndex
        | super::ImmediateRole::LayoutEncoded
        | super::ImmediateRole::TemplateSiteIndex
        | super::ImmediateRole::Unused
        | super::ImmediateRole::WideInstructionIndex => true,
    }
}

fn object_site_instruction_valid(
    instruction: super::WideInstruction,
    object_sites: &[super::ObjectSite],
    registers: u16,
) -> bool {
    match instruction.op() {
        super::Op::MakeObject2 => object_sites
            .get(instruction.object_site_index())
            .is_some_and(|site| site.atoms.len() == super::INLINE_OBJECT_SITE_ATOMS),
        super::Op::MakeObjectLiteral => {
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
                    u16::from(window.base),
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

impl ResidualProgram {
    /// Validate all cross-table references before a VM can observe the program.
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.functions.is_empty() && self.kind != super::ProgramKind::Wasm {
            return Err("program has no entry function".into());
        }
        if self.register_roots.len() > u32::MAX as usize {
            return Err("register root table is too large".into());
        }
        let selective_capture_scope_unsafe = selective_capture_scope_unsafe(&self.functions);
        for (index, function) in self.functions.iter().enumerate() {
            if function.code.is_empty() || !super::control_flow::is_bounded(function) {
                return Err(format!("function {index} can fall off its code"));
            }
            if function.registers > REGISTER_MASK {
                return Err(format!("function {index} has too many registers"));
            }
            if function
                .self_binding_slot
                .is_some_and(|slot| slot >= function.locals)
            {
                return Err(format!("function {index} has an invalid self-binding slot"));
            }
            if let Some(slot) = function.self_binding_slot
                && !function.name_bindings.iter().any(|binding| {
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
            if function.is_arrow && (function.constructible || function.is_class_constructor) {
                return Err(format!("function {index} has invalid arrow-function facts"));
            }
            if function
                .global_var_atoms
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
            if let Some(captured) = &function.selective_capture_slots
                && (index == 0
                    || function.parent.is_none()
                    || captured.iter().any(|slot| *slot >= function.locals)
                    || captured.windows(2).any(|pair| pair[0] >= pair[1]))
            {
                return Err(format!(
                    "function {index} has an invalid selective capture layout"
                ));
            }
            if function.selective_capture_slots.is_some()
                && (selective_capture_scope_unsafe[index]
                    || !selective_capture_layout_is_eligible(function, index, &self.atoms))
            {
                return Err(format!(
                    "function {index} has an ineligible selective capture layout"
                ));
            }
            if !promoted_local_layout_is_valid(function, &self.atoms) {
                return Err(format!(
                    "function {index} has an invalid promoted-local layout"
                ));
            }
            let plain_local_slots = function.plain_local_slots(|atom| {
                usize::try_from(atom)
                    .ok()
                    .filter(|atom| *atom < self.atoms.len())
                    .map(|atom| &self.atoms[atom])
            });
            if let Some(initializer) = function.instance_initializer {
                let valid = self
                    .functions
                    .get(initializer as usize)
                    .is_some_and(|plan| {
                        function.derived_constructor
                            && initializer as usize != index
                            && plan.parent == function.parent
                            && plan.class_field_initializer
                            && !plan.constructible
                            && plan.instance_initializer.is_none()
                    });
                if !valid {
                    return Err(format!(
                        "function {index} has an invalid instance initializer"
                    ));
                }
            }
            if function
                .name_bindings
                .windows(2)
                .any(|pair| pair[0].atom >= pair[1].atom)
            {
                return Err(format!(
                    "function {index} has duplicate or unsorted name bindings"
                ));
            }
            for binding in &function.name_bindings {
                if !eval_binding_is_valid(binding, function, &self.functions, self.atoms.len()) {
                    return Err(format!("function {index} has an invalid name binding"));
                }
            }
            let code_len = function.code.len() as u32;
            if function
                .source_positions
                .windows(2)
                .any(|pair| pair[0].pc >= pair[1].pc)
                || function.source_positions.iter().any(|position| {
                    position.pc >= code_len || position.line == 0 || position.column == 0
                })
            {
                return Err(format!("function {index} has invalid source positions"));
            }
            if function
                .binding_sites
                .windows(2)
                .any(|pair| pair[0].resume_pc >= pair[1].resume_pc)
                || function.binding_sites.iter().any(|site| {
                    site.resume_pc == 0
                        || site.resume_pc > code_len
                        || site
                            .bindings
                            .windows(2)
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

            if function.environment_clones.iter().any(|slots| {
                slots.iter().any(|slot| *slot >= function.locals)
                    || slots.windows(2).any(|pair| pair[0] >= pair[1])
            }) {
                return Err(format!(
                    "function {index} has invalid environment clone slots"
                ));
            }
            let bounds = ValidationBounds {
                registers: function.registers,
                locals: function.locals,
                functions: self.functions.len(),
                constants: self.constants.len(),
                environment_clones: function.environment_clones.len(),
                atoms: self.atoms.len(),
                field_sites: self.field_sites.len(),
                cache_sites: self.cache_sites,
                method_sites: self.method_sites.len(),
                object_sites: self.object_sites.len(),
                regexp_literal_sites: self.regexp_literal_sites.len(),
                superinstructions: self.superinstructions.len(),
                code_len,
            };
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
            if function.register_root_offset != u32::MAX {
                let root_end = function
                    .register_root_offset
                    .checked_add(code_len.saturating_add(1))
                    .ok_or_else(|| format!("function {index} root map overflows"))?;
                if root_end as usize > self.register_roots.len() {
                    return Err(format!("function {index} root map is out of bounds"));
                }
                if function.registers <= 64 {
                    let mask = if function.registers == 0 {
                        0
                    } else {
                        u64::MAX >> (64 - u32::from(function.registers))
                    };
                    let start = function.register_root_offset as usize;
                    let end = root_end as usize;
                    if self.register_roots[start..end]
                        .iter()
                        .any(|roots| roots & !mask != 0)
                    {
                        return Err(format!("function {index} root map has an invalid register"));
                    }
                }
            }
            for packed in &function.code {
                let Some(instruction) = instruction_at(function, *packed) else {
                    return Err(format!("function {index} wide instruction is invalid"));
                };
                if instruction.op().is_wide_marker() {
                    return Err(format!("function {index} contains nested wide instruction"));
                }
                if instruction_uses_promoted_local(instruction, function) {
                    return Err(format!(
                        "function {index} accesses a promoted binding through its local slot"
                    ));
                }
                if instruction_writes_promoted_register(instruction, function) {
                    return Err(format!(
                        "function {index} writes a promoted binding outside frame entry"
                    ));
                }
                if matches!(
                    instruction.op(),
                    super::Op::LoadLocalPlain | super::Op::StoreLocalPlain
                ) && !plain_local_slots
                    .get(instruction.local_slot())
                    .copied()
                    .unwrap_or(false)
                {
                    return Err(format!(
                        "function {index} has an unproven plain-local operation"
                    ));
                }
                if let Some(captured) = &function.selective_capture_slots {
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
                    let slot_is_captured = local_slot.is_some_and(|slot| {
                        u16::try_from(slot).is_ok_and(|slot| captured.binary_search(&slot).is_ok())
                    });
                    let environment_op = matches!(
                        instruction.op(),
                        super::Op::LoadEnvLocal | super::Op::StoreEnvLocal
                    );
                    if environment_op && !slot_is_captured {
                        return Err(format!(
                            "function {index} has an environment-local operation for an uncaptured slot"
                        ));
                    }
                    if instruction
                        .numeric_local_target()
                        .is_some_and(|target| captured.binary_search(&target).is_ok())
                        || function.dispatch == super::DispatchClass::Numeric
                            && ((instruction.op() == super::Op::StoreLocal && slot_is_captured)
                                || instruction.op() == super::Op::LoadLocal
                                    && instruction.numeric_local_store_target().is_some()
                                    && slot_is_captured)
                        || function.dispatch == super::DispatchClass::Numeric
                            && instruction.op() == super::Op::GetIndex
                            && [instruction.operand_b(), instruction.operand_c()]
                                .into_iter()
                                .any(|operand| {
                                    operand.kind() == Some(OperandKind::Local)
                                        && captured.binary_search(&operand.payload()).is_ok()
                                })
                    {
                        return Err(format!(
                            "function {index} has a numeric local fast path for an environment-owned slot"
                        ));
                    }
                }
                if instruction.op() == super::Op::Binary
                    && instruction.numeric_local_target().is_some_and(|slot| {
                        function.dispatch != super::DispatchClass::Numeric
                            && !plain_local_slots
                                .get(usize::from(slot))
                                .copied()
                                .unwrap_or(false)
                    })
                {
                    return Err(format!(
                        "function {index} has an unproven plain-local binary target"
                    ));
                }
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
                    || !object_site_instruction_valid(
                        instruction,
                        &self.object_sites,
                        function.registers,
                    )
                {
                    return Err(format!(
                        "function {index} {:?} has an out-of-domain operand",
                        instruction.op()
                    ));
                }
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
            }
            for handler in &function.handlers {
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
                if handler.slot.is_some_and(|slot| slot >= function.locals) {
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
            }
        }
        self.validate_selective_capture_owners()?;
        for site in &self.method_sites {
            if !atom_in_bounds(site.atom, self.atoms.len())
                || !cache_in_bounds(site.cache, self.cache_sites)
                || (site.argument_start as usize)
                    .checked_add(site.argument_count as usize)
                    .is_none_or(|end| end > self.method_arguments.len())
                || site.receiver_path.is_some_and(|(atom, cache)| {
                    !atom_in_bounds(atom, self.atoms.len())
                        || !cache_in_bounds(cache, self.cache_sites)
                })
            {
                return Err("invalid method site".into());
            }
        }
        for site in &self.field_sites {
            if !field_base_in_bounds(site.base.0, REGISTER_MASK)
                || !atom_in_bounds(site.first.0, self.atoms.len())
                || !cache_in_bounds(site.first.1, self.cache_sites)
                || site.second.is_some_and(|(atom, cache)| {
                    !atom_in_bounds(atom, self.atoms.len())
                        || !cache_in_bounds(cache, self.cache_sites)
                })
                || site.sink.is_some_and(|(atom, cache)| {
                    !atom_in_bounds(atom, self.atoms.len())
                        || !cache_in_bounds(cache, self.cache_sites)
                })
            {
                return Err("invalid field site".into());
            }
        }
        for site in &self.object_sites {
            if site
                .atoms
                .iter()
                .any(|atom| !atom_in_bounds(*atom, self.atoms.len()))
            {
                return Err("invalid object site".into());
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

fn selective_capture_scope_unsafe(functions: &[super::Function]) -> Vec<bool> {
    let mut inherited_dynamic_scope = vec![false; functions.len()];
    for (id, function) in functions.iter().enumerate() {
        if let Some(parent) = function.parent.map(|parent| parent as usize) {
            inherited_dynamic_scope[id] =
                inherited_dynamic_scope[parent] || !functions[parent].binding_sites.is_empty();
        }
    }
    let mut unsafe_layout = vec![false; functions.len()];
    for (id, function) in functions.iter().enumerate() {
        if !has_dynamic_scope_access(function) && !inherited_dynamic_scope[id] {
            continue;
        }
        let mut current = Some(id);
        while let Some(owner) = current {
            unsafe_layout[owner] = true;
            current = functions[owner].parent.map(|parent| parent as usize);
        }
    }
    unsafe_layout
}

fn has_dynamic_scope_access(function: &super::Function) -> bool {
    function.inherited_with_scope
        || !function.binding_sites.is_empty()
        || function
            .code
            .iter()
            .filter_map(|packed| instruction_at(function, *packed))
            .chain(function.wide.iter().copied())
            .any(|instruction| {
                matches!(
                    instruction.op(),
                    super::Op::ResolveName | super::Op::CallDirectEvalArray
                ) || (instruction.op() == super::Op::Call
                    && (super::ImmediateLayout::direct_eval(instruction.imm())
                        || super::ImmediateLayout::parameter_eval(instruction.imm())))
            })
}

fn selective_capture_layout_is_eligible(
    function: &super::Function,
    index: usize,
    atoms: &super::AtomTable,
) -> bool {
    let has_closure = function
        .code
        .iter()
        .filter_map(|packed| instruction_at(function, *packed))
        .chain(function.wide.iter().copied())
        .any(|instruction| instruction.op() == super::Op::MakeClosure);
    let local_atoms_are_static = function.local_atoms.len() == usize::from(function.locals)
        && function.local_atoms.iter().all(|atom| {
            usize::try_from(*atom)
                .ok()
                .filter(|atom| *atom < atoms.len())
                .is_some_and(|atom| !atoms[atom].starts_with('\0'))
        });
    let has_unsupported_local_lifecycle = function
        .code
        .iter()
        .filter_map(|packed| instruction_at(function, *packed))
        .chain(function.wide.iter().copied())
        .any(|instruction| {
            matches!(
                instruction.op(),
                super::Op::InitializeTdz | super::Op::CloneEnv
            )
        });
    let captured = function
        .selective_capture_slots
        .as_deref()
        .unwrap_or_default();
    let mapped_parameter_is_captured =
        function.arguments_are_mapped() && captured.iter().any(|slot| *slot < function.params);
    let mapped_arguments_are_captured = !mapped_parameter_is_captured
        || function
            .arguments_slot
            .is_some_and(|slot| captured.binary_search(&slot).is_ok());

    index != 0
        && function.parent.is_some()
        && has_closure
        && !function.is_async
        && !function.is_generator
        && !function.is_class_constructor
        && !function.derived_constructor
        && !function.class_field_initializer
        && function.simple_parameters
        && function.self_binding_slot.is_none()
        && function.environment_clones.is_empty()
        && function.lexical_atoms.is_empty()
        && local_atoms_are_static
        && !has_unsupported_local_lifecycle
        && mapped_arguments_are_captured
}

#[cfg(test)]
mod tests {
    use crate::bytecode::{AtomTable, DispatchClass, Function, Instr, Op, ResidualProgram};

    #[test]
    fn binding_site_domains_are_validated() {
        let program = crate::Engine::specialize(
            "{let value = 1; with ({}) {value; value = 2; typeof value; delete value;}}",
            "binding-sites.js",
        )
        .unwrap();
        assert!(program.validate().is_ok());
        assert!(program.functions[0].binding_sites.len() >= 2);
        for pc in [0, u32::MAX] {
            let mut invalid = program.clone();
            invalid.functions[0].binding_sites[0].resume_pc = pc;
            assert!(invalid.validate().is_err());
        }
        let mut invalid = program.clone();
        invalid.functions[0].binding_sites.reverse();
        assert!(invalid.validate().is_err());
        let mut invalid = program.clone();
        invalid.functions[0].binding_sites[0].bindings[0].atom = u32::MAX;
        assert!(invalid.validate().is_err());
        let mut invalid = program.clone();
        invalid.functions[0].binding_sites[0].bindings[0].location =
            super::super::EvalBindingLocation::Local(u16::MAX);
        assert!(invalid.validate().is_err());
        let mut invalid = program.clone();
        let binding = invalid.functions[0].binding_sites[0].bindings[0];
        invalid.functions[0].binding_sites[0].bindings.push(binding);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn name_binding_domains_are_validated() {
        use crate::bytecode::EvalBindingLocation;
        let program = crate::Engine::specialize(
            "function outer() { const captured = 1; return function inner() { eval('0'); return captured; }; }",
            "name-bindings.js",
        ).unwrap();
        assert!(program.validate().is_ok());
        let owner = program
            .functions
            .iter()
            .position(|function| function.name_bindings.len() >= 2)
            .unwrap();
        for location in [
            EvalBindingLocation::Local(u16::MAX),
            EvalBindingLocation::Capture {
                depth: u16::MAX,
                slot: 0,
            },
            EvalBindingLocation::Capture {
                depth: 0,
                slot: u16::MAX,
            },
        ] {
            let mut invalid = program.clone();
            invalid.functions[owner].name_bindings[0].location = location;
            assert!(invalid.validate().is_err());
        }
        let mut invalid = program.clone();
        invalid.functions[owner].name_bindings.reverse();
        assert!(invalid.validate().is_err());
        let mut invalid = program.clone();
        invalid.functions[owner].name_bindings[1] = invalid.functions[owner].name_bindings[0];
        assert!(invalid.validate().is_err());
        let mut invalid = program;
        invalid.functions[owner].name_bindings[0].atom = u32::MAX;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn environment_clone_domains_are_validated() {
        let program = crate::Engine::specialize(
            "var readers = []; for (let i = 0; i < 2; i++) readers.push(() => i);",
            "clones.js",
        )
        .unwrap();
        assert!(program.validate().is_ok());
        for slots in [vec![u16::MAX], vec![0, 0], vec![1, 0]] {
            let mut invalid = program.clone();
            invalid.functions[0].environment_clones[0] = slots;
            assert!(invalid.validate().is_err());
        }
        let mut invalid = program;
        invalid.functions[0].environment_clones.clear();
        assert!(invalid.validate().is_err());
    }

    fn function(code: Vec<Instr>, registers: u16, root: u32) -> Function {
        Function {
            parent: None,
            name: None,
            is_arrow: false,
            self_binding_slot: None,
            source_text: None,
            params: 0,
            length: 0,
            parameter_end_pc: 0,
            parameter_atoms: vec![],
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            instance_initializer: None,
            super_home_atom: None,
            constructible: true,
            class_field_initializer: false,
            parameter_eval_arguments_error: false,
            arguments_slot: None,
            simple_parameters: true,
            strict: false,
            locals: 0,
            local_atoms: vec![],
            environment_atoms: vec![],
            selective_capture_slots: None,
            local_registers: Vec::new(),
            inherited_with_scope: false,
            lexical_atoms: vec![],
            global_lexical_atoms: vec![],
            global_var_atoms: vec![],
            global_function_atoms: vec![],
            global_annex_b_var_atoms: vec![],
            global_immutable_atoms: vec![],
            name_bindings: vec![],
            binding_sites: vec![],
            source_positions: vec![],
            environment_clones: vec![],
            code,
            wide: vec![],
            registers,
            dispatch: DispatchClass::General,
            decoded: Default::default(),
            plain_locals: Default::default(),
            handlers: vec![],
            register_root_offset: root,
        }
    }

    fn program(function: Function, roots: Vec<u64>) -> ResidualProgram {
        ResidualProgram {
            specialized: true,
            kind: crate::bytecode::ProgramKind::Script,
            module_requests: Vec::new(),
            module_imports: Vec::new(),
            module_link_plan: None,
            source_name: String::new(),
            atoms: AtomTable::default(),
            constants: vec![],
            functions: vec![function],
            cache_sites: 0,
            method_sites: vec![],
            method_arguments: vec![],
            field_sites: vec![],
            object_sites: vec![],
            regexp_literal_sites: vec![],
            superinstructions: vec![],
            register_roots: roots,
        }
    }

    #[test]
    fn unmapped_functions_are_valid_and_mapped_roots_cover_each_pc() {
        let unmapped = program(
            function(vec![Instr::new(Op::Return, 0, 0, 0, 0)], 1, u32::MAX),
            vec![],
        );
        assert!(unmapped.validate().is_ok());

        let mapped = program(
            function(vec![Instr::new(Op::Return, 0, 0, 0, 0)], 1, 0),
            vec![1, 0],
        );
        assert!(mapped.validate().is_ok());
    }

    #[test]
    fn root_maps_reject_out_of_range_register_bits() {
        let invalid = program(
            function(vec![Instr::new(Op::Return, 0, 0, 0, 0)], 1, 0),
            vec![2, 0],
        );
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn table_and_operand_references_are_checked_before_execution() {
        for (op, minimum) in [
            (
                Op::WasmExceptionNew,
                crate::wasm::tag::ExceptionInput::MIN_COUNT,
            ),
            (
                Op::WasmStructNewDesc,
                u16::from(crate::wasm::gc::StructConstruction::Described.described()),
            ),
            (
                Op::WasmStructNewDefaultDesc,
                u16::from(crate::wasm::gc::StructConstruction::DefaultDescribed.described()),
            ),
        ] {
            for count in [0, minimum] {
                let constructor = program(
                    function(
                        vec![
                            Instr::new(op, 0, 0, count, 0),
                            Instr::new(Op::Return, 0, 0, 0, 0),
                        ],
                        1,
                        u32::MAX,
                    ),
                    vec![],
                );
                assert_eq!(constructor.validate().is_ok(), count != 0);
            }
        }
        for operator in [
            crate::wasm::atomic::AtomicOperator::I32AtomicLoad,
            crate::wasm::atomic::AtomicOperator::I64AtomicStore,
            crate::wasm::atomic::AtomicOperator::I32AtomicRmwAdd,
            crate::wasm::atomic::AtomicOperator::I64AtomicRmw32CmpxchgU,
            crate::wasm::atomic::AtomicOperator::MemoryAtomicNotify,
            crate::wasm::atomic::AtomicOperator::MemoryAtomicWait32,
            crate::wasm::atomic::AtomicOperator::MemoryAtomicWait64,
        ] {
            let expected = operator.input_count();
            for count in [0, expected - 1, expected, expected + 1] {
                let access = program(
                    function(
                        vec![
                            Instr::new(Op::WasmAtomicAccess, 0, 0, count, operator as u32),
                            Instr::new(Op::Return, 0, 0, 0, 0),
                        ],
                        expected + 1,
                        u32::MAX,
                    ),
                    vec![],
                );
                assert_eq!(access.validate().is_ok(), count == expected);
            }
        }
        let invalid_atomic = program(
            function(
                vec![
                    Instr::new(Op::WasmAtomicAccess, 0, 0, 2, u32::from(u8::MAX)),
                    Instr::new(Op::Return, 0, 0, 0, 0),
                ],
                4,
                u32::MAX,
            ),
            vec![],
        );
        assert!(invalid_atomic.validate().is_err());
        let invalid_constant = program(
            function(vec![Instr::new(Op::LoadConst, 0, 0, 0, 0)], 1, u32::MAX),
            vec![],
        );
        assert!(invalid_constant.validate().is_err());

        let mut invalid_shuffle = program(
            function(
                vec![
                    Instr::new(Op::WasmSimdShuffle, 0, 0, 0, 0),
                    Instr::new(Op::Return, 0, 0, 0, 0),
                ],
                1,
                u32::MAX,
            ),
            vec![],
        );
        invalid_shuffle
            .constants
            .push(super::super::Constant::WasmV128(
                [u8::MAX; crate::wasm::V128_BYTES],
            ));
        assert!(invalid_shuffle.validate().is_err());
        invalid_shuffle.constants[0] =
            super::super::Constant::WasmV128([0; crate::wasm::V128_BYTES]);
        assert!(invalid_shuffle.validate().is_ok());
        let mut invalid_selector = program(
            function(
                vec![
                    Instr::new(Op::WasmSimd, 0, 0, 0, u32::from(u8::MAX)),
                    Instr::new(Op::Return, 0, 0, 0, 0),
                ],
                1,
                u32::MAX,
            ),
            vec![],
        );
        assert!(invalid_selector.validate().is_err());
        invalid_selector.functions[0].code[0] =
            Instr::new(Op::WasmI31, 0, 0, 0, u32::from(u8::MAX));
        assert!(invalid_selector.validate().is_err());
        invalid_selector.functions[0].code[0] = Instr::new(
            Op::WasmI31,
            0,
            0,
            0,
            crate::wasm::i31::I31Operator::New as u32,
        );
        assert!(invalid_selector.validate().is_ok());
        invalid_selector.functions[0].code[0] =
            Instr::new(Op::WasmRefAsNonNull, 0, 0, 0, u32::from(u8::MAX));
        assert!(invalid_selector.validate().is_err());
        for check in [
            crate::wasm::reference::NonNullCheck::Reference,
            crate::wasm::reference::NonNullCheck::Function,
        ] {
            invalid_selector.functions[0].code[0] =
                Instr::new(Op::WasmRefAsNonNull, 0, 0, 0, check as u32);
            assert!(invalid_selector.validate().is_ok());
        }
        invalid_selector.functions[0].code[0] =
            Instr::new(Op::WasmRefCast, 0, 0, 0, crate::wasm::reference::EXACT);
        assert!(invalid_selector.validate().is_err());
        invalid_selector.functions[0].code[0] = Instr::new(
            Op::WasmRefCast,
            0,
            0,
            0,
            crate::wasm::reference::ReferenceTarget::from_type(wasmparser::RefType::I31REF)
                .unwrap()
                .tag(),
        );
        assert!(invalid_selector.validate().is_ok());
        invalid_selector.functions[0].code[0] = Instr::new(Op::WasmSimd, 0, 0, 0, 0);
        assert!(invalid_selector.validate().is_ok());

        let invalid_field = program(
            function(vec![Instr::new(Op::GetField, 0, 0, 0, 0)], 1, u32::MAX),
            vec![],
        );
        assert!(invalid_field.validate().is_err());

        let invalid_call = program(
            function(
                vec![Instr::new(
                    Op::Call,
                    0,
                    0,
                    0,
                    super::super::ImmediateLayout::call_immediate(1, 1, false, false),
                )],
                1,
                u32::MAX,
            ),
            vec![],
        );
        assert!(invalid_call.validate().is_err());
    }

    #[test]
    fn reachable_control_flow_cannot_fall_off_code() {
        let fallthrough = program(
            function(vec![Instr::new(Op::Nop, 0, 0, 0, 0)], 1, u32::MAX),
            vec![],
        );
        assert!(fallthrough.validate().is_err());

        let branch_fallthrough = program(
            function(vec![Instr::new(Op::JumpFalse, 0, 0, 0, 0)], 1, u32::MAX),
            vec![],
        );
        assert!(branch_fallthrough.validate().is_err());

        let loop_forever = program(
            function(vec![Instr::new(Op::Jump, 0, 0, 0, 0)], 1, u32::MAX),
            vec![],
        );
        assert!(loop_forever.validate().is_ok());
    }
}
