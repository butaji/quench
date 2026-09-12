//! Instantiated Native module: memories, tables, globals, invoke.

mod build;
mod const_eval;
mod link;
mod registry;

#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::fast::Fast;
use crate::hir::{Export, FuncSig, HeapKind, HirModule, Kind, RefType, Ty};
use crate::interp;
use crate::mir::MirFunc;
use crate::native::{Native, RefVal};
use crate::slot::Slot;
use crate::unwind::{Failure, Trap};

pub const MAX_CALL_DEPTH: usize = 10_240;

#[derive(Clone, Debug, PartialEq)]
pub enum InvokeError {
    Failure(Failure),
    /// The embedder supplied values that do not match the exported function
    /// signature.  WebAssembly invocation is typed even when the host API
    /// performs the check dynamically.
    TypeMismatch,
    Unimplemented,
    MissingExport,
    Unlinkable(&'static str),
}

impl InvokeError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Failure(failure) => failure.message(),
            Self::TypeMismatch => "type mismatch",
            Self::Unimplemented => "unimplemented",
            Self::MissingExport => "unknown export",
            Self::Unlinkable(message) => message,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Func {
    Code(MirFunc),
    Host(FuncSig),
    Unsupported,
    Import { instance: Instance, index: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Memory {
    pub data: Vec<u8>,
    pub min: u64,
    pub max: Option<u64>,
    pub page: u32,
    pub memory64: bool,
    pub shared: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub elems: Vec<RefVal>,
    pub min: u64,
    pub max: Option<u64>,
    pub table64: bool,
    pub refty: RefType,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Global {
    pub value: Slot,
    pub mutable: bool,
    pub ty: Ty,
    pub refty: Option<RefType>,
}

#[derive(Clone, Debug)]
pub struct Instance {
    inner: Rc<Inner>,
}

#[derive(Debug)]
pub(crate) struct Inner {
    id: u32,
    pub types: Box<[FuncSig]>,
    pub funcs: Box<[Func]>,
    pub func_types: Box<[u32]>,
    pub memories: Vec<Rc<RefCell<Memory>>>,
    pub tables: Vec<Rc<RefCell<Table>>>,
    pub globals: Vec<Rc<RefCell<Global>>>,
    pub datas: RefCell<Vec<Option<Box<[u8]>>>>,
    pub elems: RefCell<Vec<Option<Box<[RefVal]>>>>,
    pub tags: Box<[FuncSig]>,
    pub gc_types: Box<[crate::hir::GcType]>,
    pub gc: Rc<RefCell<crate::gc::GcHeap>>,
    pub tag_ids: Box<[u32]>,
    pub exports: HashMap<String, Export>,
    pub start: Option<u32>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        registry::unregister(self.id);
    }
}

impl PartialEq for Instance {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

#[derive(Clone, Debug)]
pub enum ResolvedImport {
    Func { func: Func, sig: FuncSig },
    Memory(Rc<RefCell<Memory>>),
    Table(Rc<RefCell<Table>>),
    Global(Rc<RefCell<Global>>),
    Tag { sig: FuncSig, id: u32 },
}

impl Instance {
    pub fn from_hir(module: HirModule) -> Result<Self, InvokeError> {
        Self::from_hir_imports(module, Vec::new())
    }

    pub fn from_hir_imports(
        module: HirModule,
        resolved: Vec<ResolvedImport>,
    ) -> Result<Self, InvokeError> {
        build::from_hir_imports(module, resolved)
    }

    /// VM load: bytes → instance. Import lookup stays at the format/host edge.
    pub fn from_bytes(
        bytes: &[u8],
        features: wasmparser::WasmFeatures,
        mut lookup: impl FnMut(
            &str,
            &str,
            &crate::hir::ImportKind,
            &[FuncSig],
        ) -> Result<ResolvedImport, InvokeError>,
    ) -> Result<Self, InvokeError> {
        let hir = crate::wasm::load(bytes, features).map_err(|_| InvokeError::Unimplemented)?;
        let mut resolved = Vec::new();
        for import in hir.imports.iter() {
            resolved.push(lookup(
                &import.module,
                &import.name,
                &import.kind,
                &hir.types,
            )?);
        }
        Self::from_hir_imports(hir, resolved)
    }

    pub fn unsupported() -> Self {
        let id = registry::alloc_id();
        let inner = Rc::new(Inner {
            id,
            types: Box::new([]),
            funcs: Box::new([]),
            func_types: Box::new([]),
            memories: Vec::new(),
            tables: Vec::new(),
            globals: Vec::new(),
            datas: RefCell::new(Vec::new()),
            elems: RefCell::new(Vec::new()),
            tags: Box::new([]),
            gc_types: Box::new([]),
            gc: registry::heap(),
            tag_ids: Box::new([]),
            exports: HashMap::new(),
            start: None,
        });
        registry::register(id, &inner);
        Self { inner }
    }

    pub fn spectest() -> Self {
        spectest()
    }

    pub fn id(&self) -> u32 {
        self.inner.id
    }

    pub fn types(&self) -> &[FuncSig] {
        &self.inner.types
    }

    pub fn funcs(&self) -> &[Func] {
        &self.inner.funcs
    }

    pub fn func_types(&self) -> &[u32] {
        &self.inner.func_types
    }

    pub fn memory(&self, index: u32) -> Option<Rc<RefCell<Memory>>> {
        self.inner.memories.get(index as usize).cloned()
    }

    pub fn table(&self, index: u32) -> Option<Rc<RefCell<Table>>> {
        self.inner.tables.get(index as usize).cloned()
    }

    pub fn global(&self, index: u32) -> Option<Rc<RefCell<Global>>> {
        self.inner.globals.get(index as usize).cloned()
    }

    pub fn datas(&self) -> &RefCell<Vec<Option<Box<[u8]>>>> {
        &self.inner.datas
    }

    pub fn elems(&self) -> &RefCell<Vec<Option<Box<[RefVal]>>>> {
        &self.inner.elems
    }

    pub fn gc(&self) -> &RefCell<crate::gc::GcHeap> {
        self.inner.gc.as_ref()
    }

    pub fn gc_type(&self, index: u32) -> Option<crate::hir::GcType> {
        self.inner.gc_types.get(index as usize).cloned()
    }

    pub fn tag_id(&self, index: u32) -> u32 {
        self.inner
            .tag_ids
            .get(index as usize)
            .copied()
            .unwrap_or(index)
    }

    pub fn resolve_export(&self, name: &str) -> Option<ResolvedImport> {
        match self.inner.exports.get(name)? {
            Export::Func(index) => Some(ResolvedImport::Func {
                func: self.export_func(*index)?,
                sig: self.func_sig(*index)?,
            }),
            Export::Memory(index) => Some(ResolvedImport::Memory(self.memory(*index)?)),
            Export::Table(index) => Some(ResolvedImport::Table(self.table(*index)?)),
            Export::Global(index) => Some(ResolvedImport::Global(self.global(*index)?)),
            Export::Tag(index) => Some(ResolvedImport::Tag {
                sig: self.inner.tags.get(*index as usize)?.clone(),
                id: *self.inner.tag_ids.get(*index as usize)?,
            }),
        }
    }

    pub fn match_import(
        &self,
        name: &str,
        kind: &crate::hir::ImportKind,
        types: &[FuncSig],
    ) -> Result<ResolvedImport, InvokeError> {
        let export = self
            .resolve_export(name)
            .ok_or(InvokeError::Unlinkable("unknown import"))?;
        link::match_import(kind, types, export)
    }

    pub fn invoke(&self, name: &str, args: &[Slot]) -> Result<Vec<Slot>, InvokeError> {
        let export = self
            .inner
            .exports
            .get(name)
            .ok_or(InvokeError::MissingExport)?;
        match *export {
            Export::Func(index) => self.call_func(index, args),
            _ => Err(InvokeError::Unimplemented),
        }
    }

    pub fn get_global(&self, name: &str) -> Result<Slot, InvokeError> {
        match self.inner.exports.get(name) {
            Some(Export::Global(index)) => self
                .global(*index)
                .map(|g| g.borrow().value.clone())
                .ok_or(InvokeError::MissingExport),
            Some(_) => Err(InvokeError::Unimplemented),
            None => Err(InvokeError::MissingExport),
        }
    }

    pub fn call_func(&self, index: u32, args: &[Slot]) -> Result<Vec<Slot>, InvokeError> {
        self.call_func_depth(index, args, 0)
    }

    pub fn call_func_depth(
        &self,
        index: u32,
        args: &[Slot],
        depth: usize,
    ) -> Result<Vec<Slot>, InvokeError> {
        let sig = self.func_sig(index).ok_or(InvokeError::Unimplemented)?;
        if !args_match(&sig.params, args) {
            return Err(InvokeError::TypeMismatch);
        }
        match self.inner.funcs.get(index as usize) {
            Some(Func::Unsupported) | None => Err(InvokeError::Unimplemented),
            Some(Func::Host(_)) => Ok(Vec::new()),
            Some(Func::Import { instance, index }) => instance.call_func_depth(*index, args, depth),
            Some(Func::Code(_)) => {
                interp::interpret(self, index, args, depth).map_err(InvokeError::Failure)
            }
        }
    }

    pub fn func_sig(&self, index: u32) -> Option<FuncSig> {
        match self.inner.funcs.get(index as usize)? {
            Func::Code(_) => {
                let ty = *self.inner.func_types.get(index as usize)?;
                self.inner.types.get(ty as usize).cloned()
            }
            Func::Host(sig) => Some(sig.clone()),
            Func::Import { instance, index } => instance.func_sig(*index),
            Func::Unsupported => None,
        }
    }

    fn export_func(&self, index: u32) -> Option<Func> {
        match self.inner.funcs.get(index as usize)? {
            Func::Import { instance, index } => Some(Func::Import {
                instance: instance.clone(),
                index: *index,
            }),
            Func::Host(sig) => Some(Func::Host(sig.clone())),
            Func::Code(_) => Some(Func::Import {
                instance: self.clone(),
                index,
            }),
            Func::Unsupported => Some(Func::Unsupported),
        }
    }
}

fn args_match(params: &[Kind], args: &[Slot]) -> bool {
    params.len() == args.len()
        && params.iter().zip(args).all(|(kind, value)| {
            matches!(
                (kind, value),
                (
                    Kind::I32,
                    Slot::Native(Native::I32(_)) | Slot::Fast(Fast::I32(_)),
                ) | (Kind::I64, Slot::Native(Native::I64(_)))
                    | (Kind::F32, Slot::Native(Native::F32(_)))
                    | (
                        Kind::F64,
                        Slot::Native(Native::F64(_))
                            | Slot::Fast(Fast::I32(_))
                            | Slot::Fast(Fast::Number(_)),
                    )
                    | (Kind::V128, Slot::Native(Native::V128(_)))
                    | (Kind::Ref, Slot::Native(Native::Ref(_)))
            )
        })
}

impl Memory {
    pub fn pages(&self) -> u64 {
        if self.page == 0 {
            0
        } else {
            self.data.len() as u64 / self.page as u64
        }
    }

    pub fn grow(&mut self, delta: u64) -> Option<u64> {
        let old = self.pages();
        let new = old.checked_add(delta)?;
        if self.page >= 65536 {
            let abs_max = if self.memory64 { 1u64 << 16 } else { 65536 };
            if new > abs_max {
                return None;
            }
        }
        if self.max.is_some_and(|max| new > max) {
            return None;
        }
        let bytes = new.checked_mul(self.page as u64)?;
        if bytes > (1u64 << 30) {
            return None;
        }
        self.data.resize(bytes as usize, 0);
        Some(old)
    }
}

pub fn load_bytes(
    instance: &Instance,
    mem: u32,
    addr: u64,
    offset: u64,
    size: usize,
) -> Result<Vec<u8>, Trap> {
    let memory = instance.memory(mem).ok_or(Trap::OutOfBoundsMemory)?;
    let memory = memory.borrow();
    let ea = addr.checked_add(offset).ok_or(Trap::OutOfBoundsMemory)?;
    let end = ea.checked_add(size as u64).ok_or(Trap::OutOfBoundsMemory)?;
    if end > memory.data.len() as u64 {
        return Err(Trap::OutOfBoundsMemory);
    }
    Ok(memory.data[ea as usize..end as usize].to_vec())
}

pub fn store_bytes(
    instance: &Instance,
    mem: u32,
    addr: u64,
    offset: u64,
    bytes: &[u8],
) -> Result<(), Trap> {
    let memory = instance.memory(mem).ok_or(Trap::OutOfBoundsMemory)?;
    let mut memory = memory.borrow_mut();
    let ea = addr.checked_add(offset).ok_or(Trap::OutOfBoundsMemory)?;
    let end = ea
        .checked_add(bytes.len() as u64)
        .ok_or(Trap::OutOfBoundsMemory)?;
    if end > memory.data.len() as u64 {
        return Err(Trap::OutOfBoundsMemory);
    }
    let start = ea as usize;
    memory.data[start..start + bytes.len()].copy_from_slice(bytes);
    Ok(())
}

pub fn addr_u64(slot: &Slot, memory64: bool) -> Result<u64, Trap> {
    match slot {
        Slot::Native(Native::I32(v)) if !memory64 => Ok(*v as u32 as u64),
        Slot::Native(Native::I64(v)) if memory64 => Ok(*v as u64),
        Slot::Native(Native::I32(v)) => Ok(*v as u32 as u64),
        Slot::Native(Native::I64(v)) => Ok(*v as u64),
        _ => Err(Trap::Unimplemented),
    }
}

pub fn lookup_func(inst: u32, index: u32) -> Option<Instance> {
    registry::get(inst)
        .map(|inner| Instance { inner })
        .filter(|i| i.inner.funcs.get(index as usize).is_some())
}

fn spectest() -> Instance {
    let prints: [(&str, FuncSig); 7] = [
        ("print", func_sig(&[], &[])),
        ("print_i32", func_sig(&[Kind::I32], &[])),
        ("print_i64", func_sig(&[Kind::I64], &[])),
        ("print_f32", func_sig(&[Kind::F32], &[])),
        ("print_f64", func_sig(&[Kind::F64], &[])),
        ("print_i32_f32", func_sig(&[Kind::I32, Kind::F32], &[])),
        ("print_f64_f64", func_sig(&[Kind::F64, Kind::F64], &[])),
    ];
    let mut exports = HashMap::new();
    let mut funcs = Vec::new();
    for (index, (name, sig)) in prints.iter().enumerate() {
        funcs.push(Func::Host(sig.clone()));
        exports.insert((*name).to_string(), Export::Func(index as u32));
    }
    exports.insert("global_i32".into(), Export::Global(0));
    exports.insert("global_i64".into(), Export::Global(1));
    exports.insert("global_f32".into(), Export::Global(2));
    exports.insert("global_f64".into(), Export::Global(3));
    exports.insert("table".into(), Export::Table(0));
    exports.insert("memory".into(), Export::Memory(0));
    let id = registry::alloc_id();
    let inner = Rc::new(Inner {
        id,
        types: Box::new([]),
        funcs: funcs.into_boxed_slice(),
        func_types: Box::new([]),
        memories: vec![Rc::new(RefCell::new(Memory {
            data: vec![0; 65536],
            min: 1,
            max: Some(2),
            page: 65536,
            memory64: false,
            shared: false,
        }))],
        tables: vec![Rc::new(RefCell::new(Table {
            elems: vec![RefVal::Null; 10],
            min: 10,
            max: Some(20),
            table64: false,
            refty: RefType {
                heap: HeapKind::Func,
                nullable: true,
            },
        }))],
        globals: vec![
            rc_global(Slot::Native(Native::I32(666)), Kind::I32),
            rc_global(Slot::Native(Native::I64(666)), Kind::I64),
            rc_global(Slot::Native(Native::F32(666.6f32.to_bits())), Kind::F32),
            rc_global(Slot::Native(Native::F64(666.6f64.to_bits())), Kind::F64),
        ],
        datas: RefCell::new(Vec::new()),
        elems: RefCell::new(Vec::new()),
        tags: Box::new([]),
        gc_types: Box::new([]),
        gc: registry::heap(),
        tag_ids: Box::new([]),
        exports,
        start: None,
    });
    registry::register(id, &inner);
    Instance { inner }
}

fn func_sig(params: &[Kind], results: &[Kind]) -> FuncSig {
    FuncSig {
        params: params.to_vec().into_boxed_slice(),
        results: results.to_vec().into_boxed_slice(),
        rec_len: 1,
        rec_index: 0,
        has_super: false,
        is_final: true,
        sub_depth: 0,
        chain: Box::new([]),
    }
}

fn rc_global(value: Slot, kind: Kind) -> Rc<RefCell<Global>> {
    Rc::new(RefCell::new(Global {
        value,
        mutable: false,
        ty: Ty::native(kind),
        refty: None,
    }))
}
