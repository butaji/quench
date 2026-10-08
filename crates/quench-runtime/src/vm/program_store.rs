use crate::Value;
use crate::bytecode::ResidualProgram;
use std::rc::Rc;

struct ProgramEntry {
    residual: Rc<ResidualProgram>,
    wasm_signatures: Option<Rc<crate::wasm::WasmSignatures>>,
    constants: Vec<Value>,
    const_arrays: Vec<Option<Rc<Vec<Value>>>>,
    module_environment: Option<Value>,
    import_meta: Option<Value>,
    module_imports: Vec<(u16, ModuleImport)>,
    module: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum ModuleImport {
    Value(Value),
    Binding(ProgramId, u16, Value),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ProgramId(u32);

impl ProgramId {
    pub(crate) const MAIN: Self = Self(0);

    fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(Self)
    }

    fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }

    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

#[derive(Default)]
pub(crate) struct ProgramStore {
    programs: Vec<ProgramEntry>,
}

impl ProgramStore {
    pub(crate) fn len(&self) -> usize {
        self.programs.len()
    }

    pub(crate) fn reset(&mut self, main: Rc<ResidualProgram>) -> ProgramId {
        self.programs.clear();
        let module = main.is_module();
        let id = self.insert_shared(main).unwrap_or(ProgramId::MAIN);
        if module && let Some(entry) = self.programs.get_mut(id.index()) {
            entry.module = true;
        }
        id
    }

    pub(crate) fn insert(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        self.insert_shared(Rc::new(program))
    }

    pub(crate) fn find_shared(&self, program: &Rc<ResidualProgram>) -> Option<ProgramId> {
        self.programs
            .iter()
            .position(|entry| Rc::ptr_eq(&entry.residual, program))
            .and_then(ProgramId::from_index)
    }

    pub(crate) fn insert_shared(&mut self, program: Rc<ResidualProgram>) -> Option<ProgramId> {
        let id = ProgramId::from_index(self.programs.len())?;
        self.programs.push(ProgramEntry {
            residual: program,
            wasm_signatures: None,
            constants: Vec::new(),
            const_arrays: Vec::new(),
            module_environment: None,
            import_meta: None,
            module_imports: Vec::new(),
            module: false,
        });
        Some(id)
    }

    /// Attach the module's canonical type facts; a code identity cannot acquire
    /// a different signature authority after functions have been registered.
    pub(crate) fn attach_wasm_signatures(
        &mut self,
        id: ProgramId,
        signatures: &Rc<crate::wasm::WasmSignatures>,
    ) -> bool {
        let Some(entry) = self.programs.get_mut(id.index()) else {
            return false;
        };
        if let Some(existing) = &entry.wasm_signatures {
            return Rc::ptr_eq(existing, signatures);
        }
        if entry.residual.validate().is_err()
            || signatures.defined_count() != entry.residual.function_count()
        {
            return false;
        }
        for function in &entry.residual.functions {
            for instruction in &function.code {
                let instruction = if instruction.is_wide() {
                    function.wide[instruction.wide_index()]
                } else {
                    instruction.as_wide()
                };
                let valid = match instruction.op() {
                    crate::bytecode::Op::WasmRefFunc => {
                        signatures.get(instruction.imm() as usize).is_some()
                    }
                    crate::bytecode::Op::WasmIndirectTarget => {
                        signatures.type_signature(instruction.imm()).is_some()
                    }
                    op if crate::wasm::gc::StructConstruction::from_op(op).is_some() => {
                        let mode = crate::wasm::gc::StructConstruction::from_op(op).unwrap();
                        signatures
                            .declarations
                            .struct_fields(instruction.imm())
                            .is_some_and(|fields| {
                                mode.described()
                                    == signatures
                                        .declarations
                                        .descriptor_type(instruction.imm())
                                        .is_some()
                                    && fields.iter().all(|field| match field.element_type {
                                        wasmparser::StorageType::I8
                                        | wasmparser::StorageType::I16 => true,
                                        wasmparser::StorageType::Val(ty) => {
                                            signatures
                                                .declarations
                                                .callable_value_type(ty)
                                                .is_some()
                                                && (!mode.defaulted() || ty.is_defaultable())
                                        }
                                    })
                                    && (mode == crate::wasm::gc::StructConstruction::Default
                                        || usize::from(instruction.register_window().count)
                                            == if mode.defaulted() {
                                                1
                                            } else {
                                                fields.len() + usize::from(mode.described())
                                            })
                            })
                    }
                    crate::bytecode::Op::WasmRefGetDesc => signatures
                        .declarations
                        .descriptor_type(instruction.imm())
                        .is_some(),
                    crate::bytecode::Op::WasmArrayNewData
                    | crate::bytecode::Op::WasmArrayNewElem => {
                        use crate::wasm::gc::{ArraySegmentInput, ArraySegmentKind};
                        instruction.register_window().count == ArraySegmentInput::COUNT
                            && signatures
                                .declarations
                                .array_field(instruction.imm())
                                .is_some_and(|field| {
                                    ArraySegmentKind::from_op(instruction.op())
                                        .unwrap()
                                        .accepts(field.element_type)
                                        && match field.element_type {
                                            wasmparser::StorageType::Val(ty) => signatures
                                                .declarations
                                                .callable_value_type(ty)
                                                .is_some(),
                                            _ => true,
                                        }
                                })
                    }
                    crate::bytecode::Op::WasmArrayNew
                    | crate::bytecode::Op::WasmArrayNewDefault
                    | crate::bytecode::Op::WasmArrayNewFixed => signatures
                        .declarations
                        .array_field(instruction.imm())
                        .is_some_and(|field| {
                            let valid = match field.element_type {
                                wasmparser::StorageType::I8 | wasmparser::StorageType::I16 => true,
                                wasmparser::StorageType::Val(ty) => {
                                    signatures.declarations.callable_value_type(ty).is_some()
                                        && (instruction.op()
                                            != crate::bytecode::Op::WasmArrayNewDefault
                                            || ty.is_defaultable())
                                }
                            };
                            valid
                                && (instruction.op() != crate::bytecode::Op::WasmArrayNewFixed
                                    || usize::from(instruction.register_b())
                                        .checked_add(usize::from(
                                            instruction.register_window().count,
                                        ))
                                        .is_some_and(|end| end <= usize::from(function.registers)))
                        }),
                    crate::bytecode::Op::WasmDescriptorTest
                    | crate::bytecode::Op::WasmDescriptorCast => {
                        crate::wasm::reference::ReferenceTarget::from_tag(instruction.imm())
                            .and_then(|target| target.descriptor_type(&signatures.declarations))
                            .is_some()
                    }
                    crate::bytecode::Op::WasmRefTest | crate::bytecode::Op::WasmRefCast => {
                        crate::wasm::reference::ReferenceTarget::from_tag(instruction.imm())
                            .and_then(|target| target.reference_type())
                            .and_then(|ty| {
                                signatures
                                    .declarations
                                    .callable_value_type(wasmparser::ValType::Ref(ty))
                            })
                            .is_some()
                    }
                    _ => true,
                };
                if !valid {
                    return false;
                }
            }
        }
        entry.wasm_signatures = Some(signatures.clone());
        true
    }

    pub(crate) fn wasm_function_signature(
        &self,
        id: ProgramId,
        index: u32,
    ) -> Option<&crate::WasmSignature> {
        self.programs
            .get(id.index())?
            .wasm_signatures
            .as_ref()?
            .defined_signature(index)
    }

    pub(crate) fn wasm_signatures(&self, id: ProgramId) -> Option<&crate::wasm::WasmSignatures> {
        self.programs.get(id.index())?.wasm_signatures.as_deref()
    }

    pub(crate) fn insert_module(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        let id = self.insert(program)?;
        self.programs[id.index()].module = true;
        Some(id)
    }

    pub(crate) fn is_module(&self, id: ProgramId) -> bool {
        self.programs
            .get(id.index())
            .is_some_and(|entry| entry.module)
    }

    pub(crate) fn set_module_environment(&mut self, id: ProgramId, environment: Value) {
        if let Some(entry) = self.programs.get_mut(id.index())
            && entry.module
        {
            entry.module_environment = Some(environment);
        }
    }

    pub(crate) fn module_environment(&self, id: ProgramId) -> Option<Value> {
        self.programs.get(id.index())?.module_environment
    }

    pub(crate) fn import_meta(&self, id: ProgramId) -> Option<Value> {
        self.programs.get(id.index())?.import_meta
    }

    pub(crate) fn set_import_meta(&mut self, id: ProgramId, value: Value) {
        if let Some(entry) = self.programs.get_mut(id.index())
            && entry.module
        {
            entry.import_meta = Some(value);
        }
    }

    pub(crate) fn set_module_imports(&mut self, id: ProgramId, imports: Vec<(u16, ModuleImport)>) {
        if let Some(entry) = self.programs.get_mut(id.index()) {
            entry.module_imports = imports;
        }
    }

    pub(crate) fn module_imports(&self, id: ProgramId) -> &[(u16, ModuleImport)] {
        self.programs
            .get(id.index())
            .map_or(&[], |entry| &entry.module_imports)
    }

    pub(crate) fn module_import(&self, id: ProgramId, slot: u16) -> Option<ModuleImport> {
        self.programs.get(id.index()).and_then(|entry| {
            entry
                .module_imports
                .iter()
                .find_map(|(local, import)| (*local == slot).then_some(*import))
        })
    }

    pub(crate) fn get(&self, id: ProgramId) -> Option<Rc<ResidualProgram>> {
        self.programs
            .get(id.index())
            .map(|entry| entry.residual.clone())
    }

    pub(crate) fn set_constants(&mut self, id: ProgramId, constants: Vec<Value>) {
        if let Some(entry) = self.programs.get_mut(id.index()) {
            entry.const_arrays = vec![None; constants.len()];
            entry.constants = constants;
        }
    }

    pub(crate) fn constant(&self, id: ProgramId, index: usize) -> Option<Value> {
        self.programs.get(id.index())?.constants.get(index).copied()
    }

    pub(crate) fn const_array(
        &mut self,
        id: ProgramId,
        start: usize,
        count: usize,
    ) -> Option<Rc<Vec<Value>>> {
        let entry = self.programs.get_mut(id.index())?;
        let end = start.checked_add(count)?;
        let cached = entry.const_arrays.get_mut(start)?;
        if cached.is_none() {
            *cached = Some(Rc::new(entry.constants.get(start..end)?.to_vec()));
        }
        cached.clone()
    }

    pub(crate) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        self.programs.iter().flat_map(|entry| {
            entry
                .constants
                .iter()
                .copied()
                .chain(entry.module_environment)
                .chain(entry.import_meta)
                .chain(
                    entry
                        .module_imports
                        .iter()
                        .filter_map(|(_, import)| match import {
                            ModuleImport::Value(value) => Some(*value),
                            ModuleImport::Binding(_, _, fallback) => Some(*fallback),
                        }),
                )
        })
    }
}
