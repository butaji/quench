use super::{
    AtomTable, Constant, DispatchClass, EvalBinding, EvalSite, FieldBase, FieldSite, Function,
    Handler, Instr, MethodSite, ModuleImportBinding, ModuleImportName, ModuleImportNameKind,
    ModuleLinkPlan, ModuleReexport, ModuleReexportKind, ModuleRequest, ModuleRequestPhase,
    ObjectSite, Op, Superinstruction, WideInstruction,
};

const RESIDUAL_MAGIC: &[u8; 5] = b"RQJ\0\x1b";
const OPTIONAL_STRING_NONE: u8 = 0;
const OPTIONAL_STRING_SOME: u8 = 1;
const MODULE_LINK_PLAN_NONE: u8 = 0;
const MODULE_LINK_PLAN_SOME: u8 = 1;

pub(super) fn write_program(
    program: &super::ResidualProgram,
    path: &std::path::Path,
) -> Result<(), String> {
    let mut out = BinaryWriter::new();
    out.bytes.extend_from_slice(RESIDUAL_MAGIC);
    out.u64(super::ResidualProgram::RUNTIME_ABI_FINGERPRINT);
    out.u8(u8::from(program.specialized));
    out.u8(u8::from(program.module));
    out.string(&program.source_name);
    out.u32(program.module_requests.len() as u32);
    for request in &program.module_requests {
        out.string(&request.source);
        out.u8(request.phase.binary_tag());
        write_optional_string(&mut out, request.module_type.as_deref());
    }
    out.u32(program.module_imports.len() as u32);
    for import in &program.module_imports {
        out.string(&import.source);
        out.u8(import.phase.binary_tag());
        write_optional_string(&mut out, import.module_type.as_deref());
        match &import.imported {
            ModuleImportName::Namespace => out.u8(import.imported.binary_tag()),
            ModuleImportName::Named(name) => {
                out.u8(import.imported.binary_tag());
                out.string(name);
            }
        }
        out.string(&import.local);
    }
    match &program.module_link_plan {
        None => out.u8(MODULE_LINK_PLAN_NONE),
        Some(plan) => {
            out.u8(MODULE_LINK_PLAN_SOME);
            out.u32(plan.locals.len() as u32);
            for (local, exported) in &plan.locals {
                out.string(local);
                out.string(exported);
            }
            out.u32(plan.hoisted_functions.len() as u32);
            for (binding, function) in &plan.hoisted_functions {
                out.string(binding);
                out.string(function);
            }
            out.u32(plan.reexports.len() as u32);
            for reexport in &plan.reexports {
                out.u8(reexport.kind().binary_tag());
                match reexport {
                    ModuleReexport::Named {
                        source,
                        imported,
                        exported,
                    } => {
                        out.string(source);
                        out.string(imported);
                        out.string(exported);
                    }
                    ModuleReexport::Star { source } => out.string(source),
                    ModuleReexport::Namespace { source, exported } => {
                        out.string(source);
                        out.string(exported);
                    }
                }
            }
        }
    }
    out.strings(&program.atoms);
    out.u32(program.constants.len() as u32);
    for value in &program.constants {
        match value {
            Constant::Number(value) => {
                out.u8(0);
                out.u64(value.to_bits());
            }
            Constant::String(value) => {
                out.u8(1);
                out.string(value);
            }
            Constant::StringUnits(value) => {
                out.u8(7);
                out.u16s(value);
            }
            Constant::BigInt(value) => {
                out.u8(6);
                out.string(value);
            }
            Constant::Boolean(value) => {
                out.u8(if *value { 2 } else { 3 });
            }
            Constant::Null => out.u8(4),
            Constant::Undefined => out.u8(5),
        }
    }
    out.u32(program.functions.len() as u32);
    for function in &program.functions {
        out.option_u32(function.parent);
        out.option_u32(function.name);
        out.u16(function.params);
        out.u16(function.length);
        out.u32(function.parameter_end_pc);
        out.u32(function.parameter_atoms.len() as u32);
        for atom in &function.parameter_atoms {
            out.u32(*atom);
        }
        out.u8(u8::from(function.rest));
        out.u8(u8::from(function.is_async));
        out.u8(u8::from(function.is_generator));
        out.u8(u8::from(function.is_class_constructor));
        out.u8(u8::from(function.derived_constructor));
        out.u32(function.super_home_atom.unwrap_or(u32::MAX));
        out.u8(u8::from(function.constructible));
        out.u8(u8::from(function.class_field_initializer));
        out.u8(u8::from(function.parameter_eval_arguments_error));
        out.u8(u8::from(function.strict));
        out.u16(function.arguments_slot.unwrap_or(u16::MAX));
        out.u16(function.locals);
        out.u32(function.local_atoms.len() as u32);
        for atom in &function.local_atoms {
            out.u32(*atom);
        }
        out.u32(function.lexical_atoms.len() as u32);
        for atom in &function.lexical_atoms {
            out.u32(*atom);
        }
        out.u32(function.global_lexical_atoms.len() as u32);
        for atom in &function.global_lexical_atoms {
            out.u32(*atom);
        }
        out.u32(function.global_var_atoms.len() as u32);
        for atom in &function.global_var_atoms {
            out.u32(*atom);
        }
        out.u32(function.global_function_atoms.len() as u32);
        for atom in &function.global_function_atoms {
            out.u32(*atom);
        }
        out.u32(function.global_immutable_atoms.len() as u32);
        for atom in &function.global_immutable_atoms {
            out.u32(*atom);
        }
        out.u32(function.eval_sites.len() as u32);
        for site in &function.eval_sites {
            out.u32(site.resume_pc);
            out.u32(site.lexical_bindings.len() as u32);
            for binding in &site.lexical_bindings {
                out.u32(binding.atom);
                out.u16(binding.slot);
                out.u8(u8::from(binding.immutable));
            }
        }
        out.u16(function.registers);
        out.u8(function.dispatch as u8);
        out.u32(function.register_root_offset);
        out.u32(function.code.len() as u32);
        for instruction in &function.code {
            out.u8(instruction.op() as u8);
            out.u16(instruction.a());
            out.u16(instruction.b());
            out.u16(instruction.c());
            out.u32(instruction.imm());
        }
        out.u32(function.wide.len() as u32);
        for instruction in &function.wide {
            out.u8(instruction.op() as u8);
            out.u16(instruction.a());
            out.u16(instruction.b());
            out.u16(instruction.c());
            out.u32(instruction.imm());
        }
        out.u32(function.handlers.len() as u32);
        for handler in &function.handlers {
            out.u32(handler.start);
            out.u32(handler.end);
            out.u32(handler.target);
            out.u16(handler.slot.unwrap_or(u16::MAX));
            out.u32(handler.return_target.unwrap_or(u32::MAX));
            out.u16(handler.return_slot.unwrap_or(u16::MAX));
            out.u16(handler.with_depth);
        }
    }
    out.u16(program.cache_sites);
    out.u32(program.method_sites.len() as u32);
    for site in &program.method_sites {
        out.u32(site.atom);
        out.u16(site.cache);
        out.u32(site.argument_start);
        out.u16(site.argument_count);
        out.optional_pair(site.receiver_path);
    }
    out.u16s(&program.method_arguments);
    out.u32(program.field_sites.len() as u32);
    for site in &program.field_sites {
        out.u16(site.base.0);
        out.pair(site.first);
        out.optional_pair(site.second);
        out.optional_pair(site.sink);
    }
    out.u32(program.object_sites.len() as u32);
    for site in &program.object_sites {
        out.u32(site.atoms[0]);
        out.u32(site.atoms[1]);
    }
    out.u32(program.superinstructions.len() as u32);
    for site in &program.superinstructions {
        for instruction in site.code {
            out.u8(instruction.op() as u8);
            out.u16(instruction.a());
            out.u16(instruction.b());
            out.u16(instruction.c());
            out.u32(instruction.imm());
        }
    }
    out.u32(program.register_roots.len() as u32);
    for value in &program.register_roots {
        out.u64(*value);
    }
    std::fs::write(path, out.bytes).map_err(|error| error.to_string())
}

pub(super) fn read_program(path: &std::path::Path) -> Result<super::ResidualProgram, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let mut input = BinaryReader::new(&bytes);
    input.magic(RESIDUAL_MAGIC)?;
    let abi = input.u64()?;
    if abi != super::ResidualProgram::RUNTIME_ABI_FINGERPRINT {
        return Err("residual runtime ABI mismatch".into());
    }
    let specialized = match input.u8()? {
        0 => false,
        1 => true,
        _ => return Err("invalid residual specialization mode".into()),
    };
    let module = match input.u8()? {
        0 => false,
        1 => true,
        _ => return Err("invalid residual source goal".into()),
    };
    let source_name = input.string()?;
    let module_requests = input.list(|input| {
        let source = input.string()?;
        let phase = ModuleRequestPhase::from_binary_tag(input.u8()?)
            .ok_or_else(|| "invalid residual module request phase".to_string())?;
        let module_type = read_optional_string(input)?;
        Ok(ModuleRequest {
            source,
            phase,
            module_type,
        })
    })?;
    let module_imports = input.list(|input| {
        let source = input.string()?;
        let phase = ModuleRequestPhase::from_binary_tag(input.u8()?)
            .ok_or_else(|| "invalid residual module import phase".to_string())?;
        let module_type = read_optional_string(input)?;
        let imported = match ModuleImportNameKind::from_binary_tag(input.u8()?)
            .ok_or_else(|| "invalid residual module import name".to_string())?
        {
            ModuleImportNameKind::Namespace => ModuleImportName::Namespace,
            ModuleImportNameKind::Named => ModuleImportName::Named(input.string()?),
        };
        let local = input.string()?;
        Ok(ModuleImportBinding {
            source,
            phase,
            module_type,
            imported,
            local,
        })
    })?;
    let module_link_plan = match input.u8()? {
        MODULE_LINK_PLAN_NONE => None,
        MODULE_LINK_PLAN_SOME => Some(ModuleLinkPlan {
            locals: input.list(|input| Ok((input.string()?, input.string()?)))?,
            hoisted_functions: input.list(|input| Ok((input.string()?, input.string()?)))?,
            reexports: input.list(|input| {
                let kind = ModuleReexportKind::from_binary_tag(input.u8()?)
                    .ok_or_else(|| "invalid residual module re-export kind".to_string())?;
                Ok(match kind {
                    ModuleReexportKind::Named => ModuleReexport::Named {
                        source: input.string()?,
                        imported: input.string()?,
                        exported: input.string()?,
                    },
                    ModuleReexportKind::Star => ModuleReexport::Star {
                        source: input.string()?,
                    },
                    ModuleReexportKind::Namespace => ModuleReexport::Namespace {
                        source: input.string()?,
                        exported: input.string()?,
                    },
                })
            })?,
        }),
        _ => return Err("invalid residual module link plan tag".into()),
    };
    let atoms = input.strings()?;
    let constants = input.list(|input| match input.u8()? {
        0 => Ok(Constant::Number(f64::from_bits(input.u64()?))),
        1 => Ok(Constant::String(input.string()?)),
        7 => Ok(Constant::StringUnits(input.u16s()?)),
        6 => Ok(Constant::BigInt(input.string()?)),
        2 => Ok(Constant::Boolean(true)),
        3 => Ok(Constant::Boolean(false)),
        4 => Ok(Constant::Null),
        5 => Ok(Constant::Undefined),
        _ => Err("invalid residual constant".into()),
    })?;
    let functions = input.list(|input| {
        let parent = input.option_u32()?;
        let name = input.option_u32()?;
        let params = input.u16()?;
        let length = input.u16()?;
        let parameter_end_pc = input.u32()?;
        let parameter_atoms = input.list(|input| input.u32())?;
        let rest = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid residual rest flag".into()),
        };
        let is_async = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid async function flag".into()),
        };
        let is_generator = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid generator function flag".into()),
        };
        let is_class_constructor = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid class constructor flag".into()),
        };
        let derived_constructor = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid derived constructor flag".into()),
        };
        let super_home_atom = match input.u32()? {
            u32::MAX => None,
            atom => Some(atom),
        };
        let constructible = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid constructible function flag".into()),
        };
        let class_field_initializer = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid class field initializer flag".into()),
        };
        let parameter_eval_arguments_error = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid parameter eval flag".into()),
        };
        let strict = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid strict function flag".into()),
        };
        let arguments_slot = match input.u16()? {
            u16::MAX => None,
            slot => Some(slot),
        };
        let locals = input.u16()?;
        let local_atoms = input.list(|input| input.u32())?;
        let lexical_atoms = input.list(|input| input.u32())?;
        let global_lexical_atoms = input.list(|input| input.u32())?;
        let global_var_atoms = input.list(|input| input.u32())?;
        let global_function_atoms = input.list(|input| input.u32())?;
        let global_immutable_atoms = input.list(|input| input.u32())?;
        let eval_sites = input.list(|input| {
            let resume_pc = input.u32()?;
            let lexical_bindings = input.list(|input| {
                Ok(EvalBinding {
                    atom: input.u32()?,
                    slot: input.u16()?,
                    immutable: match input.u8()? {
                        0 => false,
                        1 => true,
                        _ => return Err("invalid eval binding mutability flag".into()),
                    },
                })
            })?;
            Ok(EvalSite {
                resume_pc,
                lexical_bindings,
            })
        })?;
        let registers = input.u16()?;
        let dispatch = match input.u8()? {
            0 => DispatchClass::General,
            1 => DispatchClass::Numeric,
            _ => return Err("invalid residual dispatch class".into()),
        };
        let register_root_offset = input.u32()?;
        let code = input.list(|input| {
            let opcode = input.u8()?;
            let op = Op::from_index(usize::from(opcode))
                .ok_or_else(|| String::from("invalid residual opcode"))?;
            let a = input.u16()?;
            let b = input.u16()?;
            let c = input.u16()?;
            let imm = input.u32()?;
            if op == Op::Wide {
                Instr::wide_from_fields(a, b, c, imm)
                    .ok_or_else(|| String::from("residual wide index exceeds domain"))
            } else {
                Instr::try_new(op, a, b, c, imm)
                    .ok_or_else(|| String::from("residual instruction exceeds packed domain"))
            }
        })?;
        let wide = input.list(|input| {
            let opcode = input.u8()?;
            let op = Op::from_index(usize::from(opcode))
                .filter(|op| *op != Op::Wide)
                .ok_or_else(|| String::from("invalid residual wide opcode"))?;
            Ok(WideInstruction::new(
                op,
                input.u16()?,
                input.u16()?,
                input.u16()?,
                input.u32()?,
            ))
        })?;
        if !super::local_loads_in_bounds(&code, &wide, locals) {
            return Err("invalid residual local load".into());
        }
        let handlers = input.list(|input| {
            let start = input.u32()?;
            let end = input.u32()?;
            let target = input.u32()?;
            let slot = input.u16()?;
            let return_target = input.u32()?;
            let return_slot = input.u16()?;
            let with_depth = input.u16()?;
            Ok(Handler {
                start,
                end,
                target,
                slot: (slot != u16::MAX).then_some(slot),
                return_target: (return_target != u32::MAX).then_some(return_target),
                return_slot: (return_slot != u16::MAX).then_some(return_slot),
                with_depth,
            })
        })?;
        Ok(Function {
            parent,
            name,
            params,
            length,
            parameter_end_pc,
            parameter_atoms,
            rest,
            is_async,
            is_generator,
            is_class_constructor,
            derived_constructor,
            super_home_atom,
            constructible,
            class_field_initializer,
            parameter_eval_arguments_error,
            arguments_slot,
            strict,
            locals,
            local_atoms,
            lexical_atoms,
            global_lexical_atoms,
            global_var_atoms,
            global_function_atoms,
            global_immutable_atoms,
            eval_sites,
            code,
            wide,
            registers,
            dispatch,
            handlers,
            register_root_offset,
        })
    })?;
    let cache_sites = input.u16()?;
    let method_sites = input.list(|input| {
        Ok(MethodSite {
            atom: input.u32()?,
            cache: input.u16()?,
            argument_start: input.u32()?,
            argument_count: input.u16()?,
            receiver_path: input.optional_pair()?,
        })
    })?;
    let method_arguments = input.u16s()?;
    let field_sites = input.list(|input| {
        Ok(FieldSite {
            base: FieldBase(input.u16()?),
            first: input.pair()?,
            second: input.optional_pair()?,
            sink: input.optional_pair()?,
        })
    })?;
    let object_sites = input.list(|input| {
        Ok(ObjectSite {
            atoms: [input.u32()?, input.u32()?],
        })
    })?;
    let superinstructions = input.list(|input| {
        let mut code = [Instr::new(Op::Nop, 0, 0, 0, 0); 4];
        for instruction in &mut code {
            let opcode = input.u8()?;
            if usize::from(opcode) >= Op::COUNT {
                return Err("invalid residual superinstruction opcode".into());
            }
            // SAFETY: same contiguous repr(u16) invariant as normal code.
            let op = unsafe { std::mem::transmute::<u16, Op>(u16::from(opcode)) };
            *instruction =
                Instr::try_new(op, input.u16()?, input.u16()?, input.u16()?, input.u32()?)
                    .ok_or_else(|| {
                        String::from("residual superinstruction exceeds packed domain")
                    })?;
        }
        if code.map(|instruction| instruction.op())
            != [Op::MakeConstArray, Op::Binary, Op::Binary, Op::MakeObject2]
        {
            return Err("invalid residual superinstruction pattern".into());
        }
        Ok(Superinstruction { code })
    })?;
    let register_roots = input.list(|input| input.u64())?;
    input.finish()?;
    let program = super::ResidualProgram {
        specialized,
        module,
        module_requests,
        module_imports,
        module_link_plan,
        source_name,
        atoms,
        constants,
        functions,
        cache_sites,
        method_sites,
        method_arguments,
        field_sites,
        object_sites,
        superinstructions,
        register_roots,
    };
    program.validate()?;
    Ok(program)
}

pub(super) struct BinaryWriter {
    pub(super) bytes: Vec<u8>,
}

#[rustfmt::skip]
impl BinaryWriter {
    pub(super) fn new() -> Self { Self { bytes: Vec::new() } }
    pub(super) fn u8(&mut self, value: u8) { self.bytes.push(value); }
    pub(super) fn u16(&mut self, value: u16) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    pub(super) fn u32(&mut self, value: u32) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    pub(super) fn u64(&mut self, value: u64) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    pub(super) fn string(&mut self, value: &str) { self.u32(value.len() as u32); self.bytes.extend_from_slice(value.as_bytes()); }
    pub(super) fn strings(&mut self, values: &AtomTable) { self.u32(values.len() as u32); for value in values.iter() { self.string(value); } }
    pub(super) fn u16s(&mut self, values: &[u16]) { self.u32(values.len() as u32); for value in values { self.u16(*value); } }
    pub(super) fn option_u32(&mut self, value: Option<u32>) { self.u32(value.unwrap_or(u32::MAX)); }
    pub(super) fn pair(&mut self, value: (u32, u16)) { self.u32(value.0); self.u16(value.1); }
    pub(super) fn optional_pair(&mut self, value: Option<(u32, u16)>) { self.u8(value.is_some() as u8); if let Some(value) = value { self.pair(value); } }
}

pub(super) struct BinaryReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

#[rustfmt::skip]
impl<'a> BinaryReader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self { Self { bytes, cursor: 0 } }
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self.cursor.checked_add(len).ok_or("residual overflow")?;
        let value = self.bytes.get(self.cursor..end).ok_or("truncated residual")?;
        self.cursor = end;
        Ok(value)
    }
    pub(super) fn magic(&mut self, value: &[u8]) -> Result<(), String> {
        if self.take(value.len())? == value { Ok(()) } else { Err("invalid residual header".into()) }
    }
    pub(super) fn u8(&mut self) -> Result<u8, String> { Ok(self.take(1)?[0]) }
    pub(super) fn u16(&mut self) -> Result<u16, String> { Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap())) }
    pub(super) fn u32(&mut self) -> Result<u32, String> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    pub(super) fn u64(&mut self) -> Result<u64, String> { Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap())) }
    pub(super) fn string(&mut self) -> Result<String, String> {
        let len = self.u32()? as usize;
        String::from_utf8(self.take(len)?.to_vec()).map_err(|_| "invalid residual string".into())
    }
    pub(super) fn strings(&mut self) -> Result<AtomTable, String> {
        let count = self.u32()? as usize;
        let mut text = String::new();
        let mut ends = Vec::with_capacity(count);
        for _ in 0..count {
            let len = self.u32()? as usize;
            let value = std::str::from_utf8(self.take(len)?).map_err(|_| "invalid residual string")?;
            text.push_str(value);
            ends.push(text.len() as u32);
        }
        Ok(AtomTable::new(text, ends))
    }
    pub(super) fn u16s(&mut self) -> Result<Vec<u16>, String> { self.list(|input| input.u16()) }
    pub(super) fn option_u32(&mut self) -> Result<Option<u32>, String> {
        let value = self.u32()?;
        Ok((value != u32::MAX).then_some(value))
    }
    pub(super) fn pair(&mut self) -> Result<(u32, u16), String> { Ok((self.u32()?, self.u16()?)) }
    pub(super) fn optional_pair(&mut self) -> Result<Option<(u32, u16)>, String> {
        match self.u8()? { 0 => Ok(None), 1 => Ok(Some(self.pair()?)), _ => Err("invalid residual option".into()) }
    }
    pub(super) fn list<T>(&mut self, mut read: impl FnMut(&mut Self) -> Result<T, String>) -> Result<Vec<T>, String> {
        let len = self.u32()? as usize;
        (0..len).map(|_| read(self)).collect()
    }
    pub(super) fn finish(self) -> Result<(), String> {
        if self.cursor == self.bytes.len() { Ok(()) } else { Err("trailing residual data".into()) }
    }
}

fn write_optional_string(out: &mut BinaryWriter, value: Option<&str>) {
    match value {
        Some(value) => {
            out.u8(OPTIONAL_STRING_SOME);
            out.string(value);
        }
        None => out.u8(OPTIONAL_STRING_NONE),
    }
}

fn read_optional_string(input: &mut BinaryReader<'_>) -> Result<Option<String>, String> {
    match input.u8()? {
        OPTIONAL_STRING_NONE => Ok(None),
        OPTIONAL_STRING_SOME => input.string().map(Some),
        _ => Err("invalid optional residual string".into()),
    }
}
