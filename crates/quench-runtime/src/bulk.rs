//! Bulk memory and table kernels.

use std::cell::RefCell;
use std::rc::Rc;

use crate::instance::{addr_u64, Instance, Memory, Table};
use crate::native::{Native, RefVal};
use crate::slot::Slot;
use crate::unwind::{Failure, Trap};

fn uaddr(regs: &[Slot], reg: u16) -> Result<u64, Failure> {
    addr_u64(&regs[reg as usize], true).map_err(Failure::Trap)
}

fn i32_addr(regs: &[Slot], reg: u16) -> Result<usize, Failure> {
    Ok(uaddr(regs, reg)? as usize)
}

fn memory(vm: &Instance, index: u32) -> Result<Rc<RefCell<Memory>>, Failure> {
    vm.memory(index)
        .ok_or(Failure::Trap(Trap::OutOfBoundsMemory))
}

fn table(vm: &Instance, index: u32) -> Result<Rc<RefCell<Table>>, Failure> {
    vm.table(index).ok_or(Failure::Trap(Trap::OutOfBoundsTable))
}

pub fn memory_copy(
    vm: &Instance,
    regs: &[Slot],
    dst_mem: u32,
    src_mem: u32,
    dst: u16,
    src: u16,
    len: u16,
) -> Result<(), Failure> {
    let d = i32_addr(regs, dst)?;
    let s = i32_addr(regs, src)?;
    let n = i32_addr(regs, len)?;
    let src_m = memory(vm, src_mem)?;
    let slice = {
        let data = &src_m.borrow().data;
        data.get(
            s..s.checked_add(n)
                .ok_or(Failure::Trap(Trap::OutOfBoundsMemory))?,
        )
        .ok_or(Failure::Trap(Trap::OutOfBoundsMemory))?
        .to_vec()
    };
    let dst_cell = memory(vm, dst_mem)?;
    let mut dst_m = dst_cell.borrow_mut();
    write_bytes(&mut dst_m.data, d, &slice)
}

pub fn memory_fill(
    vm: &Instance,
    regs: &[Slot],
    mem: u32,
    dst: u16,
    val: u16,
    len: u16,
) -> Result<(), Failure> {
    let d = i32_addr(regs, dst)?;
    let n = i32_addr(regs, len)?;
    let byte = match regs[val as usize] {
        Slot::Native(Native::I32(v)) => v as u8,
        _ => return Err(Failure::Trap(Trap::Unimplemented)),
    };
    let cell = memory(vm, mem)?;
    let mut memory = cell.borrow_mut();
    let end = d
        .checked_add(n)
        .ok_or(Failure::Trap(Trap::OutOfBoundsMemory))?;
    if end > memory.data.len() {
        return Err(Failure::Trap(Trap::OutOfBoundsMemory));
    }
    memory.data[d..end].fill(byte);
    Ok(())
}

pub fn memory_init(
    vm: &Instance,
    regs: &[Slot],
    mem: u32,
    data: u32,
    dst: u16,
    src: u16,
    len: u16,
) -> Result<(), Failure> {
    let d = i32_addr(regs, dst)?;
    let s = i32_addr(regs, src)?;
    let n = i32_addr(regs, len)?;
    let slice = {
        let datas = vm.datas().borrow();
        let bytes = datas
            .get(data as usize)
            .and_then(|d| d.as_ref())
            .map(|b| &b[..])
            .unwrap_or(&[]);
        let end = s
            .checked_add(n)
            .ok_or(Failure::Trap(Trap::OutOfBoundsMemory))?;
        if end > bytes.len() {
            return Err(Failure::Trap(Trap::OutOfBoundsMemory));
        }
        bytes[s..end].to_vec()
    };
    let cell = memory(vm, mem)?;
    let mut mem = cell.borrow_mut();
    write_bytes(&mut mem.data, d, &slice)
}

fn write_bytes(data: &mut [u8], dst: usize, slice: &[u8]) -> Result<(), Failure> {
    let end = dst
        .checked_add(slice.len())
        .ok_or(Failure::Trap(Trap::OutOfBoundsMemory))?;
    if end > data.len() {
        return Err(Failure::Trap(Trap::OutOfBoundsMemory));
    }
    data[dst..end].copy_from_slice(slice);
    Ok(())
}

pub fn table_get(
    vm: &Instance,
    regs: &mut [Slot],
    dst: u16,
    table_idx: u32,
    index: u16,
) -> Result<(), Failure> {
    let i = i32_addr(regs, index)?;
    let val = table(vm, table_idx)?
        .borrow()
        .elems
        .get(i)
        .copied()
        .ok_or(Failure::Trap(Trap::OutOfBoundsTable))?;
    regs[dst as usize] = Slot::Native(Native::Ref(val));
    Ok(())
}

pub fn table_set(
    vm: &Instance,
    regs: &[Slot],
    table_idx: u32,
    index: u16,
    src: u16,
) -> Result<(), Failure> {
    let i = i32_addr(regs, index)?;
    let val = match regs[src as usize] {
        Slot::Native(Native::Ref(v)) => v,
        _ => RefVal::Null,
    };
    let cell = table(vm, table_idx)?;
    let mut tab = cell.borrow_mut();
    let slot = tab
        .elems
        .get_mut(i)
        .ok_or(Failure::Trap(Trap::OutOfBoundsTable))?;
    *slot = val;
    Ok(())
}

pub fn table_size(
    vm: &Instance,
    regs: &mut [Slot],
    dst: u16,
    table_idx: u32,
) -> Result<(), Failure> {
    let n = table(vm, table_idx)?.borrow().elems.len() as i32;
    regs[dst as usize] = Slot::Native(Native::I32(n));
    Ok(())
}

pub fn table_grow(
    vm: &Instance,
    regs: &mut [Slot],
    dst: u16,
    table_idx: u32,
    fill: u16,
    delta: u16,
) -> Result<(), Failure> {
    let n = i32_addr(regs, delta)?;
    let fill = match regs[fill as usize] {
        Slot::Native(Native::Ref(v)) => v,
        _ => RefVal::Null,
    };
    let cell = table(vm, table_idx)?;
    let mut tab = cell.borrow_mut();
    let old = tab.elems.len();
    let new = match (old as u64).checked_add(n as u64) {
        Some(new) => new,
        None => {
            regs[dst as usize] = Slot::Native(Native::I32(-1));
            return Ok(());
        }
    };
    if !tab.table64 && new > u32::MAX as u64 {
        regs[dst as usize] = Slot::Native(Native::I32(-1));
        return Ok(());
    }
    if tab.max.is_some_and(|max| new > max) {
        regs[dst as usize] = Slot::Native(Native::I32(-1));
        return Ok(());
    }
    tab.elems.resize(new as usize, fill);
    regs[dst as usize] = Slot::Native(Native::I32(old as i32));
    Ok(())
}

pub fn table_fill(
    vm: &Instance,
    regs: &[Slot],
    table_idx: u32,
    dst: u16,
    val: u16,
    len: u16,
) -> Result<(), Failure> {
    let d = i32_addr(regs, dst)?;
    let n = i32_addr(regs, len)?;
    let fill = match regs[val as usize] {
        Slot::Native(Native::Ref(v)) => v,
        _ => RefVal::Null,
    };
    let cell = table(vm, table_idx)?;
    let mut tab = cell.borrow_mut();
    let end = d
        .checked_add(n)
        .ok_or(Failure::Trap(Trap::OutOfBoundsTable))?;
    if end > tab.elems.len() {
        return Err(Failure::Trap(Trap::OutOfBoundsTable));
    }
    tab.elems[d..end].fill(fill);
    Ok(())
}

pub fn table_copy(
    vm: &Instance,
    regs: &[Slot],
    dst_table: u32,
    src_table: u32,
    dst: u16,
    src: u16,
    len: u16,
) -> Result<(), Failure> {
    let d = i32_addr(regs, dst)?;
    let s = i32_addr(regs, src)?;
    let n = i32_addr(regs, len)?;
    let src_elems = {
        let tab = table(vm, src_table)?;
        let elems = &tab.borrow().elems;
        elems
            .get(
                s..s.checked_add(n)
                    .ok_or(Failure::Trap(Trap::OutOfBoundsTable))?,
            )
            .ok_or(Failure::Trap(Trap::OutOfBoundsTable))?
            .to_vec()
    };
    let cell = table(vm, dst_table)?;
    let mut dst_t = cell.borrow_mut();
    let end = d
        .checked_add(n)
        .ok_or(Failure::Trap(Trap::OutOfBoundsTable))?;
    if end > dst_t.elems.len() {
        return Err(Failure::Trap(Trap::OutOfBoundsTable));
    }
    dst_t.elems[d..end].copy_from_slice(&src_elems);
    Ok(())
}

pub fn table_init(
    vm: &Instance,
    regs: &[Slot],
    table_idx: u32,
    elem: u32,
    dst: u16,
    src: u16,
    len: u16,
) -> Result<(), Failure> {
    let d = i32_addr(regs, dst)?;
    let s = i32_addr(regs, src)?;
    let n = i32_addr(regs, len)?;
    let slice = {
        let elems = vm.elems().borrow();
        let items = elems
            .get(elem as usize)
            .and_then(|e| e.as_ref())
            .map(|b| &b[..])
            .unwrap_or(&[]);
        let end = s
            .checked_add(n)
            .ok_or(Failure::Trap(Trap::OutOfBoundsTable))?;
        if end > items.len() {
            return Err(Failure::Trap(Trap::OutOfBoundsTable));
        }
        items[s..end].to_vec()
    };
    let cell = table(vm, table_idx)?;
    let mut tab = cell.borrow_mut();
    let end = d
        .checked_add(n)
        .ok_or(Failure::Trap(Trap::OutOfBoundsTable))?;
    if end > tab.elems.len() {
        return Err(Failure::Trap(Trap::OutOfBoundsTable));
    }
    tab.elems[d..end].copy_from_slice(&slice);
    Ok(())
}
