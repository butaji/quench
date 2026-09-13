//! Native GC heap: structs, arrays, and ref.cast.

use crate::hir::{GcOp, GcStorage, GcType, HeapKind};
use crate::instance::Instance;
use crate::native::{Native, RefVal};
use crate::slot::Slot;
use crate::unwind::{Failure, Trap};

#[derive(Clone, Debug, Default)]
pub struct GcHeap {
    pub structs: Vec<GcStruct>,
    pub arrays: Vec<GcArray>,
    pub exns: Vec<GcExn>,
    pub externs: Vec<RefVal>,
}

#[derive(Clone, Debug)]
pub struct GcExn {
    pub tag: u32,
    pub args: Vec<Slot>,
}

#[derive(Clone, Debug)]
pub struct GcStruct {
    pub type_idx: u32,
    pub fields: Vec<Slot>,
    pub desc: RefVal,
}

#[derive(Clone, Debug)]
pub struct GcArray {
    pub type_idx: u32,
    pub elem: GcStorage,
    pub elems: Vec<Slot>,
    pub desc: RefVal,
}

pub fn alloc_struct(heap: &mut GcHeap, type_idx: u32, fields: Vec<Slot>, desc: RefVal) -> RefVal {
    let id = heap.structs.len() as u32;
    heap.structs.push(GcStruct {
        type_idx,
        fields,
        desc,
    });
    RefVal::Struct(id)
}

pub fn alloc_array(heap: &mut GcHeap, type_idx: u32, elem: GcStorage, elems: Vec<Slot>) -> RefVal {
    let id = heap.arrays.len() as u32;
    heap.arrays.push(GcArray {
        type_idx,
        elem,
        elems,
        desc: RefVal::Null,
    });
    RefVal::Array(id)
}

pub fn alloc_exn(heap: &mut GcHeap, tag: u32, args: Vec<Slot>) -> RefVal {
    let id = heap.exns.len() as u32;
    heap.exns.push(GcExn { tag, args });
    RefVal::Exn(id)
}

pub fn step(
    vm: &Instance,
    op: GcOp,
    dst: u16,
    args: &[u16],
    regs: &mut [Slot],
) -> Result<(), Failure> {
    match op {
        GcOp::StructNewDefault { type_idx } => {
            let fields = zeros(vm, type_idx)?;
            let r = alloc_struct(&mut vm.gc().borrow_mut(), type_idx, fields, RefVal::Null);
            regs[dst as usize] = Slot::Native(Native::Ref(r));
        }
        GcOp::StructNew { type_idx } => {
            let fields: Vec<_> = args.iter().map(|r| regs[*r as usize].clone()).collect();
            let r = alloc_struct(&mut vm.gc().borrow_mut(), type_idx, fields, RefVal::Null);
            regs[dst as usize] = Slot::Native(Native::Ref(r));
        }
        GcOp::StructNewDesc { type_idx } => {
            let desc = pop_desc(regs, args.last().copied())?;
            let fields: Vec<_> = args
                .iter()
                .rev()
                .skip(1)
                .rev()
                .map(|r| regs[*r as usize].clone())
                .collect();
            let r = alloc_struct(&mut vm.gc().borrow_mut(), type_idx, fields, desc);
            regs[dst as usize] = Slot::Native(Native::Ref(r));
        }
        GcOp::StructNewDefaultDesc { type_idx } => {
            let desc = pop_desc(regs, args.first().copied())?;
            let fields = zeros(vm, type_idx)?;
            let r = alloc_struct(&mut vm.gc().borrow_mut(), type_idx, fields, desc);
            regs[dst as usize] = Slot::Native(Native::Ref(r));
        }
        GcOp::StructGet {
            field,
            signed,
            pack,
        } => {
            let id = obj(regs, args[0], true)?;
            let v = vm
                .gc()
                .borrow()
                .structs
                .get(id as usize)
                .and_then(|s| s.fields.get(field as usize).cloned())
                .ok_or(Failure::Trap(Trap::NullReference))?;
            regs[dst as usize] = extend(v, signed, pack);
        }
        GcOp::StructSet { field } => {
            let id = obj(regs, args[0], true)?;
            let v = regs[args[1] as usize].clone();
            vm.gc()
                .borrow_mut()
                .structs
                .get_mut(id as usize)
                .and_then(|s| s.fields.get_mut(field as usize))
                .map(|slot| *slot = v)
                .ok_or(Failure::Trap(Trap::NullReference))?;
        }
        GcOp::ArrayNew { type_idx } => {
            let fill = regs[args[0] as usize].clone();
            let n = i32_len(regs, args[1])?;
            let elem = array_elem(vm, type_idx)?;
            let r = alloc_array(&mut vm.gc().borrow_mut(), type_idx, elem, vec![fill; n]);
            regs[dst as usize] = Slot::Native(Native::Ref(r));
        }
        GcOp::ArrayNewDefault { type_idx } => {
            let n = i32_len(regs, args[0])?;
            let elem = array_elem(vm, type_idx)?;
            let fill = zero_storage(elem);
            let r = alloc_array(&mut vm.gc().borrow_mut(), type_idx, elem, vec![fill; n]);
            regs[dst as usize] = Slot::Native(Native::Ref(r));
        }
        GcOp::ArrayNewFixed { type_idx, n } => {
            let elem = array_elem(vm, type_idx)?;
            let elems: Vec<_> = args
                .iter()
                .take(n as usize)
                .map(|r| regs[*r as usize].clone())
                .collect();
            let r = alloc_array(&mut vm.gc().borrow_mut(), type_idx, elem, elems);
            regs[dst as usize] = Slot::Native(Native::Ref(r));
        }
        GcOp::ArrayGet { signed, pack } => {
            let id = arr(regs, args[0])?;
            let i = i32_len(regs, args[1])?;
            let v = vm
                .gc()
                .borrow()
                .arrays
                .get(id as usize)
                .and_then(|a| a.elems.get(i).cloned())
                .ok_or(Failure::Trap(Trap::OutOfBounds))?;
            regs[dst as usize] = extend(v, signed, pack);
        }
        GcOp::ArraySet => {
            let id = arr(regs, args[0])?;
            let i = i32_len(regs, args[1])?;
            let v = regs[args[2] as usize].clone();
            let mut heap = vm.gc().borrow_mut();
            let a = heap
                .arrays
                .get_mut(id as usize)
                .ok_or(Failure::Trap(Trap::NullReference))?;
            let slot = a.elems.get_mut(i).ok_or(Failure::Trap(Trap::OutOfBounds))?;
            *slot = v;
        }
        GcOp::ArrayLen => {
            let id = arr(regs, args[0])?;
            let n = vm
                .gc()
                .borrow()
                .arrays
                .get(id as usize)
                .map(|a| a.elems.len() as i32)
                .ok_or(Failure::Trap(Trap::NullReference))?;
            regs[dst as usize] = Slot::Native(Native::I32(n));
        }
        GcOp::ArrayFill => array_fill(vm, args, regs)?,
        GcOp::ArrayCopy => array_copy(vm, args, regs)?,
        GcOp::ArrayNewData { type_idx, data } => {
            array_new_data(vm, dst, args, regs, type_idx, data)?
        }
        GcOp::ArrayNewElem { type_idx, elem } => {
            array_new_elem(vm, dst, args, regs, type_idx, elem)?
        }
        GcOp::ArrayInitData { data } => array_init_data(vm, args, regs, data)?,
        GcOp::ArrayInitElem { elem } => array_init_elem(vm, args, regs, elem)?,
        GcOp::RefCast {
            nullable,
            exact,
            heap,
            type_idx,
        } => {
            let r = read_ref(regs, args[0])?;
            if !cast_ok(vm, r, nullable, exact, heap, type_idx) {
                return Err(Failure::Trap(Trap::CastFailure));
            }
            regs[dst as usize] = Slot::Native(Native::Ref(r));
        }
        GcOp::RefTest {
            nullable,
            exact,
            heap,
            type_idx,
        } => {
            let r = read_ref(regs, args[0])?;
            let ok = cast_ok(vm, r, nullable, exact, heap, type_idx);
            regs[dst as usize] = Slot::Native(Native::I32(i32::from(ok)));
        }
        GcOp::RefGetDesc => {
            let r = read_ref(regs, args[0])?;
            regs[dst as usize] = Slot::Native(Native::Ref(get_desc(vm, r)?));
        }
        GcOp::RefCastDesc {
            nullable,
            exact,
            type_idx,
        } => {
            let r = read_ref(regs, args[0])?;
            let desc = read_ref(regs, args[1])?;
            if !desc_cast_ok(vm, r, desc, nullable, exact, type_idx)? {
                return Err(Failure::Trap(Trap::DescriptorCast));
            }
            regs[dst as usize] = Slot::Native(Native::Ref(r));
        }
        GcOp::RefTestDesc {
            nullable,
            exact,
            type_idx,
        } => {
            let r = read_ref(regs, args[0])?;
            let desc = read_ref(regs, args[1])?;
            let ok = desc_cast_ok(vm, r, desc, nullable, exact, type_idx)?;
            regs[dst as usize] = Slot::Native(Native::I32(i32::from(ok)));
        }
        GcOp::AnyConvertExtern => {
            let r = unwrap_extern(vm, read_ref(regs, args[0])?)?;
            regs[dst as usize] = Slot::Native(Native::Ref(r));
        }
        GcOp::ExternConvertAny => {
            let r = wrap_extern(vm, read_ref(regs, args[0])?);
            regs[dst as usize] = Slot::Native(Native::Ref(r));
        }
    }
    Ok(())
}

fn array_fill(vm: &Instance, args: &[u16], regs: &[Slot]) -> Result<(), Failure> {
    let id = arr(regs, args[0])?;
    let i = i32_len(regs, args[1])?;
    let fill = regs[args[2] as usize].clone();
    let n = i32_len(regs, args[3])?;
    let mut heap = vm.gc().borrow_mut();
    let a = heap
        .arrays
        .get_mut(id as usize)
        .ok_or(Failure::Trap(Trap::NullReference))?;
    let end = i.checked_add(n).ok_or(Failure::Trap(Trap::OutOfBounds))?;
    if end > a.elems.len() {
        return Err(Failure::Trap(Trap::OutOfBounds));
    }
    for slot in &mut a.elems[i..end] {
        *slot = fill.clone();
    }
    Ok(())
}

fn array_copy(vm: &Instance, args: &[u16], regs: &[Slot]) -> Result<(), Failure> {
    let dst = arr(regs, args[0])?;
    let di = i32_len(regs, args[1])?;
    let src = arr(regs, args[2])?;
    let si = i32_len(regs, args[3])?;
    let n = i32_len(regs, args[4])?;
    let mut heap = vm.gc().borrow_mut();
    let slice = heap
        .arrays
        .get(src as usize)
        .and_then(|a| a.elems.get(si..si.checked_add(n)?).map(|s| s.to_vec()))
        .ok_or(Failure::Trap(Trap::OutOfBounds))?;
    let a = heap
        .arrays
        .get_mut(dst as usize)
        .ok_or(Failure::Trap(Trap::NullReference))?;
    let end = di.checked_add(n).ok_or(Failure::Trap(Trap::OutOfBounds))?;
    if end > a.elems.len() {
        return Err(Failure::Trap(Trap::OutOfBounds));
    }
    a.elems[di..end].clone_from_slice(&slice);
    Ok(())
}

fn array_new_data(
    vm: &Instance,
    dst: u16,
    args: &[u16],
    regs: &mut [Slot],
    type_idx: u32,
    data: u32,
) -> Result<(), Failure> {
    let off = i32_len(regs, args[0])?;
    let n = i32_len(regs, args[1])?;
    let elem = array_elem(vm, type_idx)?;
    let elems = data_elems(vm, data, off, n, elem)?;
    let r = alloc_array(&mut vm.gc().borrow_mut(), type_idx, elem, elems);
    regs[dst as usize] = Slot::Native(Native::Ref(r));
    Ok(())
}

fn array_new_elem(
    vm: &Instance,
    dst: u16,
    args: &[u16],
    regs: &mut [Slot],
    type_idx: u32,
    elem: u32,
) -> Result<(), Failure> {
    let off = i32_len(regs, args[0])?;
    let n = i32_len(regs, args[1])?;
    let storage = array_elem(vm, type_idx)?;
    let elems = elem_slice(vm, elem, off, n)?;
    let r = alloc_array(&mut vm.gc().borrow_mut(), type_idx, storage, elems);
    regs[dst as usize] = Slot::Native(Native::Ref(r));
    Ok(())
}

fn array_init_data(vm: &Instance, args: &[u16], regs: &[Slot], data: u32) -> Result<(), Failure> {
    let id = arr(regs, args[0])?;
    let dest = i32_len(regs, args[1])?;
    let off = i32_len(regs, args[2])?;
    let n = i32_len(regs, args[3])?;
    let elem = vm
        .gc()
        .borrow()
        .arrays
        .get(id as usize)
        .map(|a| a.elem)
        .ok_or(Failure::Trap(Trap::NullReference))?;
    let vals = data_elems(vm, data, off, n, elem)?;
    write_array_range(vm, id, dest, &vals)
}

fn array_init_elem(vm: &Instance, args: &[u16], regs: &[Slot], elem: u32) -> Result<(), Failure> {
    let id = arr(regs, args[0])?;
    let dest = i32_len(regs, args[1])?;
    let off = i32_len(regs, args[2])?;
    let n = i32_len(regs, args[3])?;
    let vals = elem_slice(vm, elem, off, n)?;
    write_array_range(vm, id, dest, &vals)
}

fn write_array_range(vm: &Instance, id: u32, dest: usize, vals: &[Slot]) -> Result<(), Failure> {
    let mut heap = vm.gc().borrow_mut();
    let a = heap
        .arrays
        .get_mut(id as usize)
        .ok_or(Failure::Trap(Trap::NullReference))?;
    let end = dest
        .checked_add(vals.len())
        .ok_or(Failure::Trap(Trap::OutOfBounds))?;
    if end > a.elems.len() {
        return Err(Failure::Trap(Trap::OutOfBounds));
    }
    a.elems[dest..end].clone_from_slice(vals);
    Ok(())
}

fn data_elems(
    vm: &Instance,
    data: u32,
    off: usize,
    n: usize,
    elem: GcStorage,
) -> Result<Vec<Slot>, Failure> {
    let size = elem_bytes(elem);
    let bytes = vm.datas().borrow();
    let empty: &[u8] = &[];
    let src = bytes
        .get(data as usize)
        .and_then(|d| d.as_deref())
        .unwrap_or(empty);
    let start = off;
    let len = n
        .checked_mul(size)
        .ok_or(Failure::Trap(Trap::OutOfBounds))?;
    let slice = src
        .get(
            start
                ..start
                    .checked_add(len)
                    .ok_or(Failure::Trap(Trap::OutOfBounds))?,
        )
        .ok_or(Failure::Trap(Trap::OutOfBounds))?;
    Ok(slice.chunks(size).map(|c| decode_elem(c, elem)).collect())
}

fn elem_slice(vm: &Instance, elem: u32, off: usize, n: usize) -> Result<Vec<Slot>, Failure> {
    let elems_sec = vm.elems().borrow();
    let empty: &[RefVal] = &[];
    let src = elems_sec
        .get(elem as usize)
        .and_then(|e| e.as_deref())
        .unwrap_or(empty);
    let slice = src
        .get(off..off.checked_add(n).ok_or(Failure::Trap(Trap::OutOfBounds))?)
        .ok_or(Failure::Trap(Trap::OutOfBounds))?;
    Ok(slice
        .iter()
        .map(|r| Slot::Native(Native::Ref(*r)))
        .collect())
}

fn elem_bytes(s: GcStorage) -> usize {
    match s {
        GcStorage::I8 => 1,
        GcStorage::I16 => 2,
        GcStorage::Val(crate::hir::Kind::I32) | GcStorage::Val(crate::hir::Kind::F32) => 4,
        GcStorage::Val(crate::hir::Kind::I64) | GcStorage::Val(crate::hir::Kind::F64) => 8,
        GcStorage::Val(crate::hir::Kind::V128) => 16,
        _ => 1,
    }
}

fn decode_elem(bytes: &[u8], s: GcStorage) -> Slot {
    match s {
        GcStorage::I8 => Slot::Native(Native::I32(bytes[0] as i32)),
        GcStorage::I16 => {
            Slot::Native(Native::I32(i16::from_le_bytes([bytes[0], bytes[1]]) as i32))
        }
        GcStorage::Val(crate::hir::Kind::I32) => Slot::Native(Native::I32(i32::from_le_bytes(
            bytes[..4].try_into().unwrap_or([0; 4]),
        ))),
        GcStorage::Val(crate::hir::Kind::I64) => Slot::Native(Native::I64(i64::from_le_bytes(
            bytes[..8].try_into().unwrap_or([0; 8]),
        ))),
        GcStorage::Val(crate::hir::Kind::F32) => Slot::Native(Native::F32(u32::from_le_bytes(
            bytes[..4].try_into().unwrap_or([0; 4]),
        ))),
        GcStorage::Val(crate::hir::Kind::F64) => Slot::Native(Native::F64(u64::from_le_bytes(
            bytes[..8].try_into().unwrap_or([0; 8]),
        ))),
        GcStorage::Val(crate::hir::Kind::V128) => {
            let mut bits = [0u8; 16];
            bits.copy_from_slice(&bytes[..bytes.len().min(16)]);
            Slot::Native(Native::V128(u128::from_le_bytes(bits)))
        }
        _ => Slot::Native(Native::Ref(RefVal::Null)),
    }
}

fn zeros(vm: &Instance, type_idx: u32) -> Result<Vec<Slot>, Failure> {
    match vm.gc_type(type_idx) {
        Some(GcType::Struct { fields, .. }) => {
            Ok(fields.iter().map(|s| zero_storage(*s)).collect())
        }
        _ => Ok(Vec::new()),
    }
}

fn array_elem(vm: &Instance, type_idx: u32) -> Result<GcStorage, Failure> {
    match vm.gc_type(type_idx) {
        Some(GcType::Array { elem, .. }) => Ok(elem),
        _ => Ok(GcStorage::Val(crate::hir::Kind::I32)),
    }
}

pub(crate) fn zero_storage(s: GcStorage) -> Slot {
    match s {
        GcStorage::I8 | GcStorage::I16 | GcStorage::Val(crate::hir::Kind::I32) => {
            Slot::Native(Native::I32(0))
        }
        GcStorage::Val(crate::hir::Kind::I64) => Slot::Native(Native::I64(0)),
        GcStorage::Val(crate::hir::Kind::F32) => Slot::Native(Native::F32(0)),
        GcStorage::Val(crate::hir::Kind::F64) => Slot::Native(Native::F64(0)),
        GcStorage::Val(crate::hir::Kind::V128) => Slot::Native(Native::V128(0)),
        GcStorage::Ref { .. } | _ => Slot::Native(Native::Ref(RefVal::Null)),
    }
}

fn extend(v: Slot, signed: Option<bool>, pack: u8) -> Slot {
    match (v, signed, pack) {
        (Slot::Native(Native::I32(x)), Some(true), 8) => Slot::Native(Native::I32(x as i8 as i32)),
        (Slot::Native(Native::I32(x)), Some(false), 8) => Slot::Native(Native::I32(x as u8 as i32)),
        (Slot::Native(Native::I32(x)), Some(true), 16) => {
            Slot::Native(Native::I32(x as i16 as i32))
        }
        (Slot::Native(Native::I32(x)), Some(false), 16) => {
            Slot::Native(Native::I32(x as u16 as i32))
        }
        (other, _, _) => other,
    }
}

fn i32_len(regs: &[Slot], r: u16) -> Result<usize, Failure> {
    match regs[r as usize] {
        Slot::Native(Native::I32(v)) if v >= 0 => Ok(v as usize),
        Slot::Native(Native::I32(_)) => Err(Failure::Trap(Trap::OutOfBounds)),
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn read_ref(regs: &[Slot], r: u16) -> Result<RefVal, Failure> {
    match regs[r as usize] {
        Slot::Native(Native::Ref(v)) => Ok(v),
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn obj(regs: &[Slot], r: u16, struct_: bool) -> Result<u32, Failure> {
    match read_ref(regs, r)? {
        RefVal::Null => Err(Failure::Trap(if struct_ {
            Trap::NullStruct
        } else {
            Trap::NullArray
        })),
        RefVal::Struct(id) if struct_ => Ok(id),
        RefVal::Array(id) if !struct_ => Ok(id),
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn arr(regs: &[Slot], r: u16) -> Result<u32, Failure> {
    obj(regs, r, false)
}

fn pop_desc(regs: &[Slot], r: Option<u16>) -> Result<RefVal, Failure> {
    let r = r.ok_or(Failure::Trap(Trap::NullDescriptor))?;
    match read_ref(regs, r)? {
        RefVal::Null => Err(Failure::Trap(Trap::NullDescriptor)),
        other => Ok(other),
    }
}

fn get_desc(vm: &Instance, r: RefVal) -> Result<RefVal, Failure> {
    match r {
        RefVal::Null => Err(Failure::Trap(Trap::NullReference)),
        RefVal::Struct(id) => vm
            .gc()
            .borrow()
            .structs
            .get(id as usize)
            .map(|s| s.desc)
            .ok_or(Failure::Trap(Trap::NullReference)),
        RefVal::Array(id) => vm
            .gc()
            .borrow()
            .arrays
            .get(id as usize)
            .map(|a| a.desc)
            .ok_or(Failure::Trap(Trap::NullReference)),
        _ => Err(Failure::Trap(Trap::CastFailure)),
    }
}

fn desc_cast_ok(
    vm: &Instance,
    val: RefVal,
    desc: RefVal,
    nullable: bool,
    exact: bool,
    type_idx: Option<u32>,
) -> Result<bool, Failure> {
    if matches!(desc, RefVal::Null) {
        return Err(Failure::Trap(Trap::NullDescriptor));
    }
    if matches!(val, RefVal::Null) {
        return Ok(nullable);
    }
    let (got, got_desc) = match val {
        RefVal::Struct(id) => vm
            .gc()
            .borrow()
            .structs
            .get(id as usize)
            .map(|s| (s.type_idx, s.desc))
            .ok_or(Failure::Trap(Trap::NullReference))?,
        RefVal::Array(id) => vm
            .gc()
            .borrow()
            .arrays
            .get(id as usize)
            .map(|a| (a.type_idx, a.desc))
            .ok_or(Failure::Trap(Trap::NullReference))?,
        _ => return Ok(false),
    };
    if got_desc != desc {
        return Ok(false);
    }
    Ok(concrete_ok(vm, val, type_idx) && (!exact || type_eq_val(vm, got, type_idx)))
}

fn type_eq_val(vm: &Instance, got: u32, type_idx: Option<u32>) -> bool {
    type_idx.is_some_and(|want| type_eq(vm, got, want))
}

fn cast_ok(
    vm: &Instance,
    r: RefVal,
    nullable: bool,
    exact: bool,
    heap: HeapKind,
    type_idx: Option<u32>,
) -> bool {
    if matches!(r, RefVal::Null) {
        return nullable;
    }
    match heap {
        HeapKind::Any => !matches!(r, RefVal::Extern(_) | RefVal::ExternBox(_) | RefVal::Exn(_)),
        HeapKind::Eq => matches!(r, RefVal::I31(_) | RefVal::Struct(_) | RefVal::Array(_)),
        HeapKind::I31 => matches!(r, RefVal::I31(_)),
        HeapKind::Struct => matches!(r, RefVal::Struct(_)),
        HeapKind::Array => matches!(r, RefVal::Array(_)),
        HeapKind::None => false,
        HeapKind::Extern => matches!(r, RefVal::Extern(_) | RefVal::ExternBox(_)),
        HeapKind::Func => matches!(r, RefVal::Func { .. }),
        HeapKind::Exn => matches!(r, RefVal::Exn(_)),
        HeapKind::NoExn | HeapKind::NoFunc | HeapKind::NoExtern => false,
        HeapKind::Concrete => {
            if exact {
                concrete_exact(vm, r, type_idx)
            } else {
                concrete_ok(vm, r, type_idx)
            }
        }
        HeapKind::Other => true,
    }
}

fn concrete_exact(vm: &Instance, r: RefVal, type_idx: Option<u32>) -> bool {
    let want = match type_idx {
        Some(idx) => idx,
        None => return false,
    };
    let got = match r {
        RefVal::Struct(id) => vm
            .gc()
            .borrow()
            .structs
            .get(id as usize)
            .map(|s| s.type_idx),
        RefVal::Array(id) => vm.gc().borrow().arrays.get(id as usize).map(|a| a.type_idx),
        RefVal::Func { inst, index } => {
            return func_ok(vm, inst, index, want, true);
        }
        _ => return false,
    };
    got.is_some_and(|got| type_eq(vm, got, want))
}

pub(crate) fn func_type_ok(vm: &Instance, inst: u32, index: u32, want: u32, exact: bool) -> bool {
    func_ok(vm, inst, index, want, exact)
}

fn func_ok(vm: &Instance, inst: u32, index: u32, want: u32, exact: bool) -> bool {
    let Some(got) = crate::instance::lookup_func(inst, index).and_then(|c| c.func_sig(index))
    else {
        return false;
    };
    let Some(want) = vm.types().get(want as usize) else {
        return false;
    };
    if exact {
        got == *want
    } else {
        got.assignable_to(want)
    }
}

fn wrap_extern(vm: &Instance, r: RefVal) -> RefVal {
    match r {
        RefVal::Null => RefVal::Null,
        RefVal::Host(id) | RefVal::Extern(id) => RefVal::Extern(id),
        RefVal::ExternBox(id) => RefVal::ExternBox(id),
        inner => {
            let mut heap = vm.gc().borrow_mut();
            let id = heap.externs.len() as u32;
            heap.externs.push(inner);
            RefVal::ExternBox(id)
        }
    }
}

fn unwrap_extern(vm: &Instance, r: RefVal) -> Result<RefVal, Failure> {
    match r {
        RefVal::Null => Ok(RefVal::Null),
        RefVal::Extern(id) | RefVal::Host(id) => Ok(RefVal::Host(id)),
        RefVal::ExternBox(id) => vm
            .gc()
            .borrow()
            .externs
            .get(id as usize)
            .copied()
            .ok_or(Failure::Trap(Trap::CastFailure)),
        _ => Err(Failure::Trap(Trap::CastFailure)),
    }
}

fn concrete_ok(vm: &Instance, r: RefVal, type_idx: Option<u32>) -> bool {
    let want = match type_idx {
        Some(idx) => idx,
        None => return false,
    };
    let got = match r {
        RefVal::Struct(id) => vm
            .gc()
            .borrow()
            .structs
            .get(id as usize)
            .map(|s| s.type_idx),
        RefVal::Array(id) => vm.gc().borrow().arrays.get(id as usize).map(|a| a.type_idx),
        RefVal::Func { inst, index } => {
            return func_ok(vm, inst, index, want, false);
        }
        _ => return false,
    };
    got.is_some_and(|got| is_sub_ty(vm, got, want))
}

fn is_sub_ty(vm: &Instance, got: u32, want: u32) -> bool {
    if type_eq(vm, got, want) {
        return true;
    }
    match vm.gc_type(got) {
        Some(GcType::Struct { super_idx, .. })
        | Some(GcType::Array { super_idx, .. })
        | Some(GcType::Func { super_idx, .. }) => super_idx.is_some_and(|s| is_sub_ty(vm, s, want)),
        _ => false,
    }
}

fn type_eq(vm: &Instance, a: u32, b: u32) -> bool {
    if a == b {
        return true;
    }
    match (vm.gc_type(a), vm.gc_type(b)) {
        (
            Some(GcType::Struct {
                fields: fa,
                super_idx: sa,
                descriptor_idx: da,
                describes_idx: dsa,
                is_final: fa_final,
            }),
            Some(GcType::Struct {
                fields: fb,
                super_idx: sb,
                descriptor_idx: db,
                describes_idx: dsb,
                is_final: fb_final,
            }),
        ) => fa == fb && da == db && dsa == dsb && fa_final == fb_final && super_eq(vm, sa, sb),
        (
            Some(GcType::Array {
                elem: ea,
                super_idx: sa,
                descriptor_idx: da,
                describes_idx: dsa,
                is_final: fa_final,
            }),
            Some(GcType::Array {
                elem: eb,
                super_idx: sb,
                descriptor_idx: db,
                describes_idx: dsb,
                is_final: fb_final,
            }),
        ) => ea == eb && da == db && dsa == dsb && fa_final == fb_final && super_eq(vm, sa, sb),
        (
            Some(GcType::Func {
                super_idx: sa,
                descriptor_idx: da,
                describes_idx: dsa,
                is_final: fa_final,
            }),
            Some(GcType::Func {
                super_idx: sb,
                descriptor_idx: db,
                describes_idx: dsb,
                is_final: fb_final,
            }),
        ) => da == db && dsa == dsb && fa_final == fb_final && super_eq(vm, sa, sb),
        _ => false,
    }
}

fn super_eq(vm: &Instance, a: Option<u32>, b: Option<u32>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => type_eq(vm, a, b),
        _ => false,
    }
}
