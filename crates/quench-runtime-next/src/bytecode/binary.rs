use super::{
    AtomTable, Constant, DispatchClass, FieldBase, FieldSite, Function, Handler, Instr, MethodSite,
    ObjectSite, Op, Superinstruction, WideInstruction,
};

pub(super) fn write_program(
    program: &super::ResidualProgram,
    path: &std::path::Path,
) -> Result<(), String> {
    let mut out = BinaryWriter::new();
    out.bytes.extend_from_slice(b"RQJ\0\x0d");
    out.u64(super::ResidualProgram::RUNTIME_ABI_FINGERPRINT);
    out.u8(u8::from(program.specialized));
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
        out.u8(u8::from(function.rest));
        out.u8(u8::from(function.is_async));
        out.u8(u8::from(function.is_generator));
        out.u8(u8::from(function.strict));
        out.u16(function.arguments_slot.unwrap_or(u16::MAX));
        out.u16(function.locals);
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
    input.magic(b"RQJ\0\x0d")?;
    let abi = input.u64()?;
    if abi != super::ResidualProgram::RUNTIME_ABI_FINGERPRINT {
        return Err("residual runtime ABI mismatch".into());
    }
    let specialized = match input.u8()? {
        0 => false,
        1 => true,
        _ => return Err("invalid residual specialization mode".into()),
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
        let registers = input.u16()?;
        let dispatch = match input.u8()? {
            0 => DispatchClass::General,
            1 => DispatchClass::Numeric,
            _ => return Err("invalid residual dispatch class".into()),
        };
        let register_root_offset = input.u32()?;
        let code = input.list(|input| {
            let opcode = input.u8()?;
            if usize::from(opcode) >= Op::COUNT {
                return Err("invalid residual opcode".into());
            }
            // SAFETY: `Op` is a contiguous repr(u16) enum generated by `opcodes!`.
            let op = unsafe { std::mem::transmute::<u16, Op>(u16::from(opcode)) };
            let a = input.u16()?;
            let b = input.u16()?;
            let c = input.u16()?;
            let imm = input.u32()?;
            if op == Op::Wide {
                let index = u64::from(a)
                    | (u64::from(b) << 14)
                    | (u64::from(c) << 28)
                    | (u64::from(imm) << 42);
                Instr::wide(index as usize)
                    .ok_or_else(|| String::from("residual wide index exceeds domain"))
            } else {
                Instr::try_new(op, a, b, c, imm)
                    .ok_or_else(|| String::from("residual instruction exceeds packed domain"))
            }
        })?;
        let wide = input.list(|input| {
            let opcode = input.u8()?;
            if usize::from(opcode) >= Op::COUNT || opcode == Op::Wide as u8 {
                return Err("invalid residual wide opcode".into());
            }
            // SAFETY: `Op` is a contiguous repr(u16) enum generated by `opcodes!`.
            let op = unsafe { std::mem::transmute::<u16, Op>(u16::from(opcode)) };
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
            Ok(Handler {
                start,
                end,
                target,
                slot: (slot != u16::MAX).then_some(slot),
            })
        })?;
        Ok(Function {
            parent,
            name,
            params,
            rest,
            is_async,
            is_generator,
            arguments_slot,
            strict,
            locals,
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
