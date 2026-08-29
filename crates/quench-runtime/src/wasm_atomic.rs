//! Native threads atomics. Single-threaded: RMW is load/op/store.

use crate::hir::AtomicOp;
use crate::instance::{self, Instance};
use crate::native::Native;
use crate::slot::Slot;
use crate::unwind::{Failure, Trap};

pub fn step(
    vm: &Instance,
    op: AtomicOp,
    dst: u16,
    addr: u16,
    a: u16,
    b: u16,
    offset: u64,
    mem: u32,
    bytes: u8,
    wide: bool,
    regs: &mut [Slot],
) -> Result<(), Failure> {
    if matches!(op, AtomicOp::Fence) {
        return Ok(());
    }
    let memory64 = vm.memory(mem).map(|m| m.borrow().memory64).unwrap_or(false);
    let ea_base = instance::addr_u64(&regs[addr as usize], memory64).map_err(Failure::Trap)?;
    let ea = ea_base
        .checked_add(offset)
        .ok_or(Failure::Trap(Trap::OutOfBoundsMemory))?;
    let size = bytes as usize;
    if size > 1 && ea % size as u64 != 0 {
        return Err(Failure::Trap(Trap::UnalignedAtomic));
    }
    match op {
        AtomicOp::Load => {
            let v = load(vm, mem, ea_base, offset, size)?;
            write_int(regs, dst, v, wide);
        }
        AtomicOp::Store => {
            let v = read_int(regs, a, wide);
            store(vm, mem, ea_base, offset, size, v)?;
        }
        AtomicOp::Wait => wait(vm, mem, ea_base, offset, size, a, b, wide, regs, dst)?,
        AtomicOp::Notify => {
            load(vm, mem, ea_base, offset, size)?;
            write_int(regs, dst, 0, false);
        }
        AtomicOp::Fence => {}
        rmw => {
            let old = load(vm, mem, ea_base, offset, size)?;
            let arg = read_int(regs, a, wide);
            let mask = mask(size);
            let next = if rmw == AtomicOp::Cmpxchg {
                let neu = read_int(regs, b, wide);
                if (old & mask) == (arg & mask) {
                    neu
                } else {
                    old
                }
            } else {
                apply(rmw, old, arg, mask)
            };
            store(vm, mem, ea_base, offset, size, next)?;
            write_int(regs, dst, old & mask, wide);
        }
    }
    Ok(())
}

fn mask(size: usize) -> u64 {
    if size >= 8 {
        u64::MAX
    } else {
        (1u64 << (size * 8)) - 1
    }
}

fn apply(op: AtomicOp, old: u64, arg: u64, mask: u64) -> u64 {
    let x = old & mask;
    let y = arg & mask;
    let v = match op {
        AtomicOp::Add => x.wrapping_add(y),
        AtomicOp::Sub => x.wrapping_sub(y),
        AtomicOp::And => x & y,
        AtomicOp::Or => x | y,
        AtomicOp::Xor => x ^ y,
        AtomicOp::Xchg => y,
        _ => x,
    };
    (old & !mask) | (v & mask)
}

fn load(vm: &Instance, mem: u32, addr: u64, offset: u64, size: usize) -> Result<u64, Failure> {
    let bytes = instance::load_bytes(vm, mem, addr, offset, size).map_err(Failure::Trap)?;
    let mut buf = [0u8; 8];
    buf[..size].copy_from_slice(&bytes);
    Ok(u64::from_le_bytes(buf))
}

fn store(
    vm: &Instance,
    mem: u32,
    addr: u64,
    offset: u64,
    size: usize,
    val: u64,
) -> Result<(), Failure> {
    let bytes = val.to_le_bytes();
    instance::store_bytes(vm, mem, addr, offset, &bytes[..size]).map_err(Failure::Trap)
}

fn wait(
    vm: &Instance,
    mem: u32,
    addr: u64,
    offset: u64,
    size: usize,
    expected: u16,
    timeout: u16,
    wide: bool,
    regs: &mut [Slot],
    dst: u16,
) -> Result<(), Failure> {
    let shared = vm.memory(mem).map(|m| m.borrow().shared).unwrap_or(false);
    if !shared {
        return Err(Failure::Trap(Trap::ExpectedShared));
    }
    let got = load(vm, mem, addr, offset, size)? & mask(size);
    let exp = read_int(regs, expected, wide) & mask(size);
    let code = if got != exp {
        1
    } else {
        let _ = timeout;
        2
    };
    write_int(regs, dst, code, false);
    Ok(())
}

fn read_int(regs: &[Slot], r: u16, wide: bool) -> u64 {
    match regs[r as usize] {
        Slot::Native(Native::I64(v)) if wide => v as u64,
        Slot::Native(Native::I32(v)) => v as u32 as u64,
        Slot::Native(Native::I64(v)) => v as u64,
        _ => 0,
    }
}

fn write_int(regs: &mut [Slot], dst: u16, v: u64, wide: bool) {
    regs[dst as usize] = if wide {
        Slot::Native(Native::I64(v as i64))
    } else {
        Slot::Native(Native::I32(v as i32))
    };
}
