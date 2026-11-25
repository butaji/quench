//! The file form of an `AotImage`: magic, build id, fingerprint, the recipe tables and atom
//! texts, the base PC labels, operand records and proven call sites, then per function its base and lane code with
//! relocations. Every number is
//! little-endian; `Words` reads it back and rejects a file that ends early or names an unknown tag.
//!
//! Owner: one pipeline.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{AotFunction, AotImage, CallSite, LaneImage, base::BaseImage, symbols::{HeapValueRecipe, ShapeRecipe}};
use crate::compiled::AHEAD_BUILD_ID;
use quench_runtime::vm::engine_types::WasmRows;
use quench_runtime::vm::jit_backend::relocatable_code::{RelocatableCode, Relocation};
use cranelift_codegen::binemit::Reloc;

const MAGIC: &[u8; 8] = b"QAOT\0\0\0\x04";
/// The memory slot of Wasm rows whose entry reads no memory.
const NO_SLOT: u32 = u32::MAX;
/// The shape prototype a shape with none names.
const NO_PROTOTYPE: u32 = u32::MAX;
const RECIPE_INTRINSIC: u8 = 0;
const RECIPE_GLOBAL: u8 = 1;
const RECIPE_STRING: u8 = 2;
const RECIPE_PROTOTYPE_OF: u8 = 3;
const RECIPE_CLOSURE_OF: u8 = 4;
const RELOC_ABS8: u8 = 0;
const RELOC_X86_PC_REL4: u8 = 1;
const RELOC_X86_CALL_PC_REL4: u8 = 2;
const RELOC_X86_CALL_PLT_REL4: u8 = 3;
const RELOC_ARM64_CALL: u8 = 4;
/// A relocation kind `RelocatableCode` never holds; the reader rejects it.
const RELOC_UNPATCHABLE: u8 = u8::MAX;

/// Serializes `image` (`from_bytes` reads it back).
pub(crate) fn to_bytes(image: &AotImage) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend(image.build_id.to_le_bytes());
    out.extend(image.fingerprint.to_le_bytes());
    put_recipes(&mut out, image);
    put_sites(&mut out, image);
    put_count(&mut out, image.functions.len());
    for function in &image.functions {
        put_function(&mut out, function);
    }
    put_wasm(&mut out, image.wasm.as_ref());
    put_regexps(&mut out, &image.regexps);
    out
}

/// The image `to_bytes` wrote; an error names what the file lacks or from which build it is.
pub(crate) fn from_bytes(bytes: &[u8]) -> Result<AotImage, String> {
    let mut words = Words(bytes.strip_prefix(MAGIC.as_slice()).ok_or("not an AOT image file")?);
    let build_id = words.quad()?;
    if build_id != AHEAD_BUILD_ID {
        return Err("images from another quench-node build".into());
    }
    let fingerprint = words.quad()?;
    let (shapes, heap_values, atoms) = read_recipes(&mut words)?;
    let labels = (0..words.half()?).map(|_| Ok((words.half()?, words.half()?))).collect::<Result<_, String>>()?;
    let calls = (0..words.half()?).map(|_| read_call(&mut words)).collect::<Result<_, String>>()?;
    let count = words.half()?;
    let functions = (0..count).map(|_| read_function(&mut words)).collect::<Result<_, String>>()?;
    let wasm = read_wasm(&mut words)?;
    let regexps = read_regexps(&mut words)?;
    Ok(AotImage { fingerprint, build_id, functions, shapes, heap_values, atoms, labels, calls, wasm, regexps })
}

fn put_count(out: &mut Vec<u8>, count: usize) {
    out.extend((count as u32).to_le_bytes());
}

fn put_recipes(out: &mut Vec<u8>, image: &AotImage) {
    put_count(out, image.shapes.len());
    for shape in &image.shapes {
        put_shape(out, shape);
    }
    put_count(out, image.heap_values.len());
    for value in &image.heap_values {
        put_heap_value(out, value);
    }
    put_count(out, image.atoms.len());
    for atom in &image.atoms {
        put_count(out, atom.len());
        out.extend(atom.as_bytes());
    }
}

/// The base PC labels, then the call sites.
fn put_sites(out: &mut Vec<u8>, image: &AotImage) {
    put_count(out, image.labels.len());
    for (function, pc) in &image.labels {
        out.extend(function.to_le_bytes());
        out.extend(pc.to_le_bytes());
    }
    put_count(out, image.calls.len());
    for call in &image.calls {
        put_call(out, call);
    }
}

/// A proven call site: its site and callee, and its record's words the plan fixed (the packed header word with
/// its argc, the proven arguments, the registers it names, its argument places); the run-time words are the
/// loader's (`read_call`).
fn put_call(out: &mut Vec<u8>, call: &CallSite) {
    let record = &call.record;
    [call.function, call.pc, call.callee].iter().for_each(|half| out.extend(half.to_le_bytes()));
    out.extend(record.packed.to_le_bytes());
    out.extend(record.proven_args.to_le_bytes());
    [record.callee_register, record.this_register, record.args_register, record.result_register].iter().for_each(|register| out.extend(register.to_le_bytes()));
    out.extend(record.argument_places);
}

/// The call site `put_call` wrote, its record bound to no closure (`super::unbound_callee`).
fn read_call(words: &mut Words<'_>) -> Result<CallSite, String> {
    let (function, pc, callee) = (words.half()?, words.half()?, words.half()?);
    let packed = words.quad()?;
    let proven_args = words.quad()?;
    let [callee_register, this_register, args_register, result_register] = [words.pair()?, words.pair()?, words.pair()?, words.pair()?];
    let argument_places = words.take()?;
    let record = quench_bjit::call_record::CallRecord { callee: super::unbound_callee(), packed, proven_args, callee_register, this_register,
        args_register, result_register, argument_places, ..Default::default() };
    Ok(CallSite { function, pc, callee, record })
}

fn put_shape(out: &mut Vec<u8>, shape: &ShapeRecipe) {
    out.extend(shape.prototype.unwrap_or(NO_PROTOTYPE).to_le_bytes());
    put_count(out, shape.fields.len());
    for (atom, attributes) in shape.fields.iter() {
        out.extend(atom.to_le_bytes());
        out.push(*attributes);
    }
}

fn put_heap_value(out: &mut Vec<u8>, value: &HeapValueRecipe) {
    let (tag, key) = match *value {
        HeapValueRecipe::Intrinsic(key) => (RECIPE_INTRINSIC, key),
        HeapValueRecipe::Global(key) => (RECIPE_GLOBAL, key),
        HeapValueRecipe::String(key) => (RECIPE_STRING, key),
        HeapValueRecipe::PrototypeOf(key) => (RECIPE_PROTOTYPE_OF, key),
        HeapValueRecipe::ClosureOf(key) => (RECIPE_CLOSURE_OF, key),
    };
    out.push(tag);
    out.extend(key.to_le_bytes());
}

fn put_function(out: &mut Vec<u8>, function: &AotFunction) {
    out.extend(function.id.to_le_bytes());
    put_base(out, &function.base);
    match &function.lane {
        Some(lane) => {
            out.push(1);
            out.push(u8::from(lane.fault_poll));
            put_code(out, &lane.code);
        }
        None => out.push(0),
    }
}

fn put_base(out: &mut Vec<u8>, base: &BaseImage) {
    put_code(out, &base.code);
    put_halves(out, &base.pc_offsets);
    put_halves(out, &base.generic);
    out.extend(base.scratch.to_le_bytes());
}

fn put_halves(out: &mut Vec<u8>, values: &[u32]) {
    put_count(out, values.len());
    for value in values {
        out.extend(value.to_le_bytes());
    }
}


fn put_code(out: &mut Vec<u8>, code: &RelocatableCode) {
    put_count(out, code.code.len());
    out.extend(&*code.code);
    put_count(out, code.relocations.len());
    for relocation in &*code.relocations {
        out.extend((relocation.offset as u32).to_le_bytes());
        out.push(reloc_tag(relocation.kind));
        out.extend(relocation.namespace.to_le_bytes());
        out.extend(relocation.key.to_le_bytes());
        out.extend(relocation.addend.to_le_bytes());
    }
}

fn reloc_tag(kind: Reloc) -> u8 {
    match kind {
        Reloc::Abs8 => RELOC_ABS8,
        Reloc::X86PCRel4 => RELOC_X86_PC_REL4,
        Reloc::X86CallPCRel4 => RELOC_X86_CALL_PC_REL4,
        Reloc::X86CallPLTRel4 => RELOC_X86_CALL_PLT_REL4,
        Reloc::Arm64Call => RELOC_ARM64_CALL,
        _ => RELOC_UNPATCHABLE,
    }
}

fn reloc_of(tag: u8) -> Result<Reloc, String> {
    Ok(match tag {
        RELOC_ABS8 => Reloc::Abs8,
        RELOC_X86_PC_REL4 => Reloc::X86PCRel4,
        RELOC_X86_CALL_PC_REL4 => Reloc::X86CallPCRel4,
        RELOC_X86_CALL_PLT_REL4 => Reloc::X86CallPLTRel4,
        RELOC_ARM64_CALL => Reloc::Arm64Call,
        _ => return Err("an AOT image relocation has an unknown kind".into()),
    })
}

type Recipes = (Vec<ShapeRecipe>, Vec<HeapValueRecipe>, Vec<Box<str>>);

fn read_recipes(words: &mut Words<'_>) -> Result<Recipes, String> {
    let shapes = (0..words.half()?).map(|_| read_shape(words)).collect::<Result<_, String>>()?;
    let heap_values = (0..words.half()?).map(|_| read_heap_value(words)).collect::<Result<_, String>>()?;
    let atoms = (0..words.half()?).map(|_| read_atom(words)).collect::<Result<_, String>>()?;
    Ok((shapes, heap_values, atoms))
}

fn read_shape(words: &mut Words<'_>) -> Result<ShapeRecipe, String> {
    let prototype = words.half()?;
    let fields = (0..words.half()?).map(|_| Ok((words.half()?, words.byte()?))).collect::<Result<_, String>>()?;
    Ok(ShapeRecipe { prototype: (prototype != NO_PROTOTYPE).then_some(prototype), fields })
}

fn read_heap_value(words: &mut Words<'_>) -> Result<HeapValueRecipe, String> {
    let tag = words.byte()?;
    let key = words.half()?;
    Ok(match tag {
        RECIPE_INTRINSIC => HeapValueRecipe::Intrinsic(key),
        RECIPE_GLOBAL => HeapValueRecipe::Global(key),
        RECIPE_STRING => HeapValueRecipe::String(key),
        RECIPE_PROTOTYPE_OF => HeapValueRecipe::PrototypeOf(key),
        RECIPE_CLOSURE_OF => HeapValueRecipe::ClosureOf(key),
        _ => return Err("an AOT image heap value recipe has an unknown kind".into()),
    })
}

fn read_atom(words: &mut Words<'_>) -> Result<Box<str>, String> {
    let length = words.half()? as usize;
    let text = std::str::from_utf8(words.bytes(length)?).map_err(|_| "an AOT image atom is not text")?;
    Ok(text.into())
}

fn read_function(words: &mut Words<'_>) -> Result<AotFunction, String> {
    let id = words.half()?;
    let base = read_base(words)?;
    let lane = match words.byte()? {
        0 => None,
        _ => {
            let fault_poll = words.byte()? != 0;
            Some(LaneImage { code: read_code(words)?, fault_poll })
        }
    };
    Ok(AotFunction { id, base, lane })
}

/// The Wasm rows, behind a byte that says whether there are any: the memory slot (`NO_SLOT` for none),
/// then per function a byte for whether it has rows, and its rows.
fn put_wasm(out: &mut Vec<u8>, wasm: Option<&WasmRows>) {
    let Some(wasm) = wasm else { return out.push(0) };
    out.push(1);
    out.extend(wasm.memory_slot.unwrap_or(NO_SLOT).to_le_bytes());
    put_count(out, wasm.functions.len());
    for rows in &wasm.functions {
        match rows {
            Some(code) => {
                out.push(1);
                put_code(out, code);
            }
            None => out.push(0),
        }
    }
}

/// The RegExp literals' searches: per search its key's source, flags byte and bounds byte, then its code.
fn put_regexps(out: &mut Vec<u8>, regexps: &[quench_runtime::vm::engine_types::AotRegexp]) {
    put_count(out, regexps.len());
    for regexp in regexps {
        let (source, flags, bounds) = &regexp.key;
        put_count(out, source.len());
        out.extend(source.as_bytes());
        out.extend([*flags, u8::from(*bounds)]);
        put_count(out, regexp.code.len());
        out.extend(&regexp.code);
    }
}

fn read_regexps(words: &mut Words<'_>) -> Result<Vec<quench_runtime::vm::engine_types::AotRegexp>, String> {
    (0..words.half()?).map(|_| {
        let source = read_atom(words)?;
        let (flags, bounds) = (words.byte()?, words.byte()? != 0);
        let length = words.half()? as usize;
        Ok(quench_runtime::vm::engine_types::AotRegexp { key: (source, flags, bounds), code: words.bytes(length)?.into() })
    }).collect()
}

fn read_wasm(words: &mut Words<'_>) -> Result<Option<WasmRows>, String> {
    if words.byte()? == 0 {
        return Ok(None);
    }
    let slot = words.half()?;
    let functions = (0..words.half()?).map(|_| match words.byte()? { 0 => Ok(None), _ => read_code(words).map(Some) }).collect::<Result<_, String>>()?;
    Ok(Some(WasmRows { memory_slot: (slot != NO_SLOT).then_some(slot), functions }))
}

fn read_base(words: &mut Words<'_>) -> Result<BaseImage, String> {
    let code = read_code(words)?;
    let pc_offsets = read_halves(words)?;
    let generic = read_halves(words)?;
    let scratch = words.half()?;
    Ok(BaseImage { code, pc_offsets, generic, scratch })
}

fn read_halves(words: &mut Words<'_>) -> Result<Box<[u32]>, String> {
    (0..words.half()?).map(|_| words.half()).collect()
}


fn read_code(words: &mut Words<'_>) -> Result<RelocatableCode, String> {
    let length = words.half()? as usize;
    let code: Box<[u8]> = words.bytes(length)?.into();
    let relocations = (0..words.half()?).map(|_| read_relocation(words)).collect::<Result<_, String>>()?;
    Ok(RelocatableCode { code, relocations })
}

fn read_relocation(words: &mut Words<'_>) -> Result<Relocation, String> {
    let offset = words.half()? as usize;
    let kind = reloc_of(words.byte()?)?;
    Ok(Relocation { offset, kind, namespace: words.half()?, key: words.half()?, addend: words.quad()? as i64 })
}

/// A reader of the file's numbers.
pub(crate) struct Words<'a>(pub(crate) &'a [u8]);

impl<'a> Words<'a> {
    pub(crate) fn bytes(&mut self, count: usize) -> Result<&'a [u8], String> {
        let (head, rest) = self.0.split_at_checked(count).ok_or("an AOT image file ends early")?;
        self.0 = rest;
        Ok(head)
    }
    fn take<const N: usize>(&mut self) -> Result<[u8; N], String> {
        Ok(self.bytes(N)?.try_into().expect("N bytes"))
    }
    pub(crate) fn byte(&mut self) -> Result<u8, String> {
        Ok(u8::from_le_bytes(self.take()?))
    }
    pub(crate) fn pair(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take()?))
    }
    pub(crate) fn half(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    pub(crate) fn quad(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take()?))
    }
}
