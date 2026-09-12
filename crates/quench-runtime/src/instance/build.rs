//! Bind imports, evaluate inits, run start.

use std::cell::RefCell;
use std::rc::Rc;

use super::const_eval;
use super::{registry, Func, Global, Inner, Instance, InvokeError, Memory, ResolvedImport, Table};
use crate::hir::{ConstExpr, Export, HirElem, HirMemory, HirModule, HirTable};
use crate::mir;
use crate::native::{Native, RefVal};
use crate::slot::Slot;
use crate::unwind::{Failure, Trap};

pub fn from_hir_imports(
    module: HirModule,
    resolved: Vec<ResolvedImport>,
) -> Result<Instance, InvokeError> {
    if !module.imports.is_empty() && resolved.len() != module.imports.len() {
        return Err(InvokeError::Unlinkable("unknown import"));
    }
    let mut funcs = Vec::new();
    let mut memories = Vec::new();
    let mut tables = Vec::new();
    let mut globals = Vec::new();
    let mut tags = Vec::new();
    let mut tag_ids = Vec::new();
    for item in resolved {
        match item {
            ResolvedImport::Func { func, .. } => funcs.push(func),
            ResolvedImport::Memory(memory) => memories.push(memory),
            ResolvedImport::Table(table) => tables.push(table),
            ResolvedImport::Global(slot) => globals.push(slot),
            ResolvedImport::Tag { sig, id } => {
                tags.push(sig);
                tag_ids.push(id);
            }
        }
    }
    funcs.extend(module.funcs.into_vec().into_iter().map(|func| match func {
        Some(func) => Func::Code(mir::specialise(func)),
        None => Func::Unsupported,
    }));
    for ty in module.memories.iter() {
        memories.push(Rc::new(RefCell::new(init_memory(ty)?)));
    }
    let defined_tables = tables.len();
    for ty in module.tables.iter() {
        tables.push(Rc::new(RefCell::new(init_table(ty))));
    }
    let id = registry::alloc_id();
    let gc = registry::heap();
    for global in module.globals.iter() {
        let value = const_eval::eval(
            &global.init,
            &globals,
            id,
            Some(gc.as_ref()),
            &module.gc_types,
        )?;
        globals.push(Rc::new(RefCell::new(Global {
            value,
            mutable: global.mutable,
            ty: global.ty,
            refty: global.refty,
        })));
    }
    for (i, ty) in module.tables.iter().enumerate() {
        if let Some(init) = ty.init.as_ref() {
            fill_table(&tables[defined_tables + i], init, &globals, id)?;
        }
    }
    for idx in module.tags.iter() {
        tags.push(
            module
                .types
                .get(*idx as usize)
                .cloned()
                .ok_or(InvokeError::Unimplemented)?,
        );
        tag_ids.push(new_tag_id());
    }
    let datas = module.datas.iter().map(|d| Some(d.bytes.clone())).collect();
    let elems = module
        .elems
        .iter()
        .map(|e| Some(vec![RefVal::Null; e.items.len()].into_boxed_slice()))
        .collect();
    let exports = module
        .exports
        .into_vec()
        .into_iter()
        .map(|(name, export)| (name.into_string(), export))
        .collect();
    let inner = Rc::new(Inner {
        id,
        types: module.types,
        funcs: funcs.into_boxed_slice(),
        func_types: module.func_types,
        memories,
        tables,
        globals,
        datas: RefCell::new(datas),
        elems: RefCell::new(elems),
        tags: tags.into_boxed_slice(),
        gc_types: module.gc_types,
        gc,
        tag_ids: tag_ids.into_boxed_slice(),
        exports,
        start: module.start,
    });
    registry::register(id, &inner);
    let instance = Instance { inner };
    if let Err(error) = finish(&instance, &module.datas, &module.elems) {
        registry::pin(instance.inner.clone());
        return Err(error);
    }
    Ok(instance)
}

fn finish(
    instance: &Instance,
    datas: &[crate::hir::HirData],
    elems: &[HirElem],
) -> Result<(), InvokeError> {
    apply_active_segments(instance, datas, elems)?;
    if let Some(start) = instance.inner.start {
        instance.call_func(start, &[])?;
    }
    Ok(())
}

fn new_tag_id() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn init_memory(ty: &HirMemory) -> Result<Memory, InvokeError> {
    let page = 1u32 << ty.page_size_log2;
    let bytes = ty
        .initial
        .checked_mul(page as u64)
        .ok_or(InvokeError::Unlinkable("memory size too big"))?;
    if bytes > isize::MAX as u64 {
        return Err(InvokeError::Unlinkable("memory size too big"));
    }
    Ok(Memory {
        data: vec![0; bytes as usize],
        min: ty.initial,
        max: ty.maximum,
        page,
        memory64: ty.memory64,
        shared: ty.shared,
    })
}

fn init_table(ty: &HirTable) -> Table {
    Table {
        elems: vec![RefVal::Null; ty.initial as usize],
        min: ty.initial,
        max: ty.maximum,
        table64: ty.table64,
        refty: ty.refty,
    }
}

fn fill_table(
    table: &Rc<RefCell<Table>>,
    init: &ConstExpr,
    globals: &[Rc<RefCell<Global>>],
    inst: u32,
) -> Result<(), InvokeError> {
    let fill = slot_ref(const_eval::eval(init, globals, inst, None, &[])?, inst)?;
    table.borrow_mut().elems.fill(fill);
    Ok(())
}

fn slot_ref(slot: Slot, _inst: u32) -> Result<RefVal, InvokeError> {
    match slot {
        Slot::Native(Native::Ref(value)) => Ok(value),
        _ => Err(InvokeError::Unimplemented),
    }
}

fn apply_active_segments(
    instance: &Instance,
    datas: &[crate::hir::HirData],
    elems: &[HirElem],
) -> Result<(), InvokeError> {
    // Spec instantiate: active elems, then active data.
    for (index, elem) in elems.iter().enumerate() {
        eval_elem_items(instance, index, elem)?;
        if elem.declared {
            instance.inner.elems.borrow_mut()[index] = None;
            continue;
        }
        let Some(offset) = elem.offset.as_ref() else {
            continue;
        };
        let offset = const_eval::eval_u64(offset, &instance.inner.globals, instance.id())?;
        write_table(instance, elem.table, offset, index)?;
        instance.inner.elems.borrow_mut()[index] = None;
    }
    for (index, data) in datas.iter().enumerate() {
        let Some(offset) = data.offset.as_ref() else {
            continue;
        };
        let offset = const_eval::eval_u64(offset, &instance.inner.globals, instance.id())?;
        write_mem(instance, data.mem, offset, &data.bytes)?;
        instance.inner.datas.borrow_mut()[index] = None;
    }
    Ok(())
}

fn eval_elem_items(instance: &Instance, index: usize, elem: &HirElem) -> Result<(), InvokeError> {
    let inst = instance.id();
    let values: Result<Vec<_>, _> = elem
        .items
        .iter()
        .map(|item| {
            slot_ref(
                const_eval::eval(
                    item,
                    &instance.inner.globals,
                    inst,
                    Some(instance.inner.gc.as_ref()),
                    &instance.inner.gc_types,
                )?,
                inst,
            )
        })
        .collect();
    instance.inner.elems.borrow_mut()[index] = Some(values?.into_boxed_slice());
    Ok(())
}

fn write_mem(instance: &Instance, mem: u32, offset: u64, bytes: &[u8]) -> Result<(), InvokeError> {
    let memory = instance
        .inner
        .memories
        .get(mem as usize)
        .cloned()
        .ok_or(InvokeError::Failure(Failure::Trap(Trap::OutOfBoundsMemory)))?;
    let mut memory = memory.borrow_mut();
    let end = offset
        .checked_add(bytes.len() as u64)
        .ok_or(InvokeError::Failure(Failure::Trap(Trap::OutOfBoundsMemory)))?;
    if end > memory.data.len() as u64 {
        return Err(InvokeError::Failure(Failure::Trap(Trap::OutOfBoundsMemory)));
    }
    let start = offset as usize;
    memory.data[start..start + bytes.len()].copy_from_slice(bytes);
    Ok(())
}

fn write_table(
    instance: &Instance,
    table: u32,
    offset: u64,
    elem: usize,
) -> Result<(), InvokeError> {
    let items = instance
        .inner
        .elems
        .borrow()
        .get(elem)
        .and_then(|e| e.clone())
        .ok_or(InvokeError::Failure(Failure::Trap(Trap::OutOfBoundsTable)))?;
    let tab = instance
        .inner
        .tables
        .get(table as usize)
        .cloned()
        .ok_or(InvokeError::Failure(Failure::Trap(Trap::OutOfBoundsTable)))?;
    let mut tab = tab.borrow_mut();
    let end = offset
        .checked_add(items.len() as u64)
        .ok_or(InvokeError::Failure(Failure::Trap(Trap::OutOfBoundsTable)))?;
    if end > tab.elems.len() as u64 {
        return Err(InvokeError::Failure(Failure::Trap(Trap::OutOfBoundsTable)));
    }
    tab.elems[offset as usize..end as usize].copy_from_slice(&items);
    Ok(())
}

pub fn empty_exports() -> std::collections::HashMap<String, Export> {
    std::collections::HashMap::new()
}
