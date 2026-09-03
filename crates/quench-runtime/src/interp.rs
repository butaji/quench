//! One interpreter loop over MIR.

use crate::fast::Fast;
use crate::hir::{HirFunc, Inst, LoadOp, StoreOp, WideOp};
use crate::instance::{self, Func, Instance, MAX_CALL_DEPTH};
use crate::native::{Bits, ConvOp, Native, RefVal};
use crate::slot::Slot;
use crate::unwind::{Failure, Trap};

struct Frame {
    func: u32,
    pc: usize,
    regs: Vec<Slot>,
    dsts: Box<[u16]>,
    handlers: Vec<Box<[crate::hir::CatchClause]>>,
    caught: Vec<(u32, Vec<Slot>)>,
}

enum Step {
    Next,
    Jump(u32),
    Return(Vec<Slot>),
    Call {
        inst: u32,
        func: u32,
        args: Vec<Slot>,
        dsts: Box<[u16]>,
        tail: bool,
    },
}

pub fn interpret(
    vm: &Instance,
    func: u32,
    args: &[Slot],
    depth: usize,
) -> Result<Vec<Slot>, Failure> {
    if matches!(vm.funcs().get(func as usize), Some(Func::Host(_))) {
        return Ok(Vec::new());
    }
    let mut frames: Vec<Frame> = Vec::new();
    let mut current = new_frame(vm, func, args, Box::new([]))?;
    loop {
        if current.pc == usize::MAX {
            if let Some(mut caller) = frames.pop() {
                write_returns(&mut caller.regs, &current.dsts, current.regs);
                current = caller;
                continue;
            }
            return Ok(current.regs);
        }
        let inst = inst_at(vm, current.func, current.pc)?;
        match step(vm, &mut current, &inst) {
            Err(Failure::Exception { tag, args }) => {
                if !take_catch(vm, &mut frames, &mut current, tag, args) {
                    return Err(Failure::Exception {
                        tag,
                        args: Vec::new(),
                    });
                }
            }
            Err(error) => return Err(error),
            Ok(Step::Next) => current.pc += 1,
            Ok(Step::Jump(target)) => current.pc = target as usize,
            Ok(Step::Call {
                inst,
                func,
                args,
                dsts,
                tail,
            }) => {
                let result = if tail {
                    dispatch_tail(vm, &mut frames, &mut current, inst, func, args, depth)
                } else {
                    current.pc += 1;
                    dispatch_call(vm, &mut frames, &mut current, inst, func, args, dsts, depth)
                };
                if let Err(Failure::Exception { tag, args }) = result {
                    if !take_catch(vm, &mut frames, &mut current, tag, args) {
                        return Err(Failure::Exception {
                            tag,
                            args: Vec::new(),
                        });
                    }
                } else {
                    result?;
                }
            }
            Ok(Step::Return(values)) => {
                if let Some(mut caller) = frames.pop() {
                    write_returns(&mut caller.regs, &current.dsts, values);
                    current = caller;
                } else {
                    return Ok(values);
                }
            }
        }
    }
}

fn dispatch_tail(
    vm: &Instance,
    frames: &mut Vec<Frame>,
    current: &mut Frame,
    inst: u32,
    func: u32,
    args: Vec<Slot>,
    depth: usize,
) -> Result<(), Failure> {
    let dsts = current.dsts.clone();
    if inst != vm.id() {
        let other = instance::lookup_func(inst, func).ok_or(Failure::Trap(Trap::Unimplemented))?;
        let values = other
            .call_func_depth(func, &args, depth + frames.len())
            .map_err(|err| match err {
                crate::instance::InvokeError::Failure(failure) => failure,
                _ => Failure::Trap(Trap::Unimplemented),
            })?;
        write_returns(&mut current.regs, &dsts, values);
        current.pc = usize::MAX;
        return Ok(());
    }
    match vm.funcs().get(func as usize) {
        Some(Func::Host(_)) => tail_values(frames, current, dsts, Vec::new()),
        Some(Func::Import { instance, index }) => {
            let values = instance
                .call_func_depth(*index, &args, depth + frames.len())
                .map_err(|err| match err {
                    crate::instance::InvokeError::Failure(failure) => failure,
                    _ => Failure::Trap(Trap::Unimplemented),
                })?;
            tail_values(frames, current, dsts, values)
        }
        Some(Func::Code(_)) => {
            *current = new_frame(vm, func, &args, dsts)?;
            Ok(())
        }
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn tail_values(
    frames: &mut Vec<Frame>,
    current: &mut Frame,
    dsts: Box<[u16]>,
    values: Vec<Slot>,
) -> Result<(), Failure> {
    if let Some(mut caller) = frames.pop() {
        write_returns(&mut caller.regs, &dsts, values);
        *current = caller;
    } else {
        *current = Frame {
            func: current.func,
            pc: usize::MAX,
            regs: values,
            dsts,
            handlers: Vec::new(),
            caught: Vec::new(),
        };
    }
    Ok(())
}

fn dispatch_call(
    vm: &Instance,
    frames: &mut Vec<Frame>,
    current: &mut Frame,
    inst: u32,
    func: u32,
    args: Vec<Slot>,
    dsts: Box<[u16]>,
    depth: usize,
) -> Result<(), Failure> {
    if inst != vm.id() {
        let other = instance::lookup_func(inst, func).ok_or(Failure::Trap(Trap::Unimplemented))?;
        if depth + frames.len() + 1 >= MAX_CALL_DEPTH {
            return Err(Failure::Trap(Trap::CallStackExhausted));
        }
        let values = other
            .call_func_depth(func, &args, depth + frames.len() + 1)
            .map_err(|err| match err {
                crate::instance::InvokeError::Failure(failure) => failure,
                _ => Failure::Trap(Trap::Unimplemented),
            })?;
        write_returns(&mut current.regs, &dsts, values);
        return Ok(());
    }
    match vm.funcs().get(func as usize) {
        Some(Func::Host(_)) => Ok(()),
        Some(Func::Import { instance, index }) => {
            if depth + frames.len() + 1 >= MAX_CALL_DEPTH {
                return Err(Failure::Trap(Trap::CallStackExhausted));
            }
            let values = instance
                .call_func_depth(*index, &args, depth + frames.len() + 1)
                .map_err(|err| match err {
                    crate::instance::InvokeError::Failure(failure) => failure,
                    _ => Failure::Trap(Trap::Unimplemented),
                })?;
            write_returns(&mut current.regs, &dsts, values);
            Ok(())
        }
        Some(Func::Code(_)) => {
            if depth + frames.len() + 1 >= MAX_CALL_DEPTH {
                return Err(Failure::Trap(Trap::CallStackExhausted));
            }
            frames.push(std::mem::replace(
                current,
                new_frame(vm, func, &args, dsts)?,
            ));
            Ok(())
        }
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn inst_at(vm: &Instance, func: u32, pc: usize) -> Result<Inst, Failure> {
    match vm.funcs().get(func as usize) {
        Some(Func::Code(code)) => code
            .code
            .get(pc)
            .cloned()
            .ok_or(Failure::Trap(Trap::Unreachable)),
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn new_frame(vm: &Instance, func: u32, args: &[Slot], dsts: Box<[u16]>) -> Result<Frame, Failure> {
    let code = match vm.funcs().get(func as usize) {
        Some(Func::Code(code)) => code,
        Some(Func::Host(_)) => {
            return Ok(Frame {
                func,
                pc: usize::MAX,
                regs: Vec::new(),
                dsts,
                handlers: Vec::new(),
                caught: Vec::new(),
            });
        }
        _ => return Err(Failure::Trap(Trap::Unimplemented)),
    };
    let mut regs = zeros(code)?;
    bind_args(&mut regs, code, args)?;
    Ok(Frame {
        func,
        pc: 0,
        regs,
        dsts,
        handlers: Vec::new(),
        caught: Vec::new(),
    })
}

fn zeros(func: &HirFunc) -> Result<Vec<Slot>, Failure> {
    let n = func.nregs as usize;
    let mut regs = Vec::with_capacity(n.max(1));
    for ty in func.params.iter().chain(func.locals.iter()) {
        regs.push(Slot::zero(*ty).ok_or(Failure::Trap(Trap::Unimplemented))?);
    }
    while regs.len() < n {
        regs.push(Slot::Native(Native::I32(0)));
    }
    Ok(regs)
}

fn bind_args(regs: &mut [Slot], func: &HirFunc, args: &[Slot]) -> Result<(), Failure> {
    if args.len() != func.params.len() {
        return Err(Failure::Trap(Trap::Unimplemented));
    }
    for (index, arg) in args.iter().enumerate() {
        regs[index] = arg.clone();
    }
    Ok(())
}

fn write_returns(regs: &mut [Slot], dsts: &[u16], values: Vec<Slot>) {
    for (dst, value) in dsts.iter().zip(values) {
        regs[*dst as usize] = value;
    }
}

fn step(vm: &Instance, frame: &mut Frame, inst: &Inst) -> Result<Step, Failure> {
    match inst {
        Inst::TryBegin { catches } => {
            frame.handlers.push(catches.clone());
            return Ok(Step::Next);
        }
        Inst::TryEnd => {
            frame.handlers.pop();
            return Ok(Step::Next);
        }
        Inst::Throw { tag, args } => {
            return Err(Failure::Exception {
                tag: vm.tag_id(*tag),
                args: args
                    .iter()
                    .map(|r| frame.regs[*r as usize].clone())
                    .collect(),
            });
        }
        Inst::ThrowRef { src } => {
            return throw_ref(vm, &frame.regs, *src);
        }
        Inst::Rethrow { depth } => {
            let i = frame.caught.len().saturating_sub(1 + *depth as usize);
            return match frame.caught.get(i) {
                Some((tag, args)) => Err(Failure::Exception {
                    tag: *tag,
                    args: args.clone(),
                }),
                None => Err(Failure::Trap(Trap::Unreachable)),
            };
        }
        Inst::Gc { op, dst, args } => {
            crate::gc::step(vm, *op, *dst, args, &mut frame.regs)?;
            return Ok(Step::Next);
        }
        _ => {}
    }
    let regs = &mut frame.regs;
    match inst {
        Inst::Nop => Ok(Step::Next),
        Inst::Unreachable => Err(Failure::Trap(Trap::Unreachable)),
        Inst::ConstI32 { dst, val } => write(regs, *dst, Slot::Native(Native::I32(*val))),
        Inst::ConstI64 { dst, val } => write(regs, *dst, Slot::Native(Native::I64(*val))),
        Inst::ConstF32 { dst, bits } => write(regs, *dst, Slot::Native(Native::F32(*bits))),
        Inst::ConstF64 { dst, bits } => write(regs, *dst, Slot::Native(Native::F64(*bits))),
        Inst::ConstV128 { dst, bits } => write(regs, *dst, Slot::Native(Native::V128(*bits))),
        Inst::ConstRefNull { dst } => write(regs, *dst, Slot::Native(Native::Ref(RefVal::Null))),
        Inst::ConstRefFunc { dst, func } => write(
            regs,
            *dst,
            Slot::Native(Native::Ref(RefVal::Func {
                inst: vm.id(),
                index: *func,
            })),
        ),
        Inst::UnI32 { op, dst, src } => {
            let v = op.apply(read_i32(regs, *src)?);
            write(regs, *dst, Slot::Native(Native::I32(v)))
        }
        Inst::BinI32 { op, dst, lhs, rhs } => {
            let v = op
                .apply(read_i32(regs, *lhs)?, read_i32(regs, *rhs)?)
                .map_err(Failure::Trap)?;
            write(regs, *dst, Slot::Native(Native::I32(v)))
        }
        Inst::UnI64 { op, dst, src } => {
            let v = op.apply(read_i64(regs, *src)?);
            if matches!(op, crate::native::UnI64::Eqz) {
                write(regs, *dst, Slot::Native(Native::I32(v as i32)))
            } else {
                write(regs, *dst, Slot::Native(Native::I64(v)))
            }
        }
        Inst::BinI64 { op, dst, lhs, rhs } => {
            let v = op
                .apply(read_i64(regs, *lhs)?, read_i64(regs, *rhs)?)
                .map_err(Failure::Trap)?;
            if op.is_rel() {
                write(regs, *dst, Slot::Native(Native::I32(v as i32)))
            } else {
                write(regs, *dst, Slot::Native(Native::I64(v)))
            }
        }
        Inst::UnF32 { op, dst, src } => {
            let v = op.apply(read_f32(regs, *src)?);
            write(regs, *dst, Slot::Native(Native::F32(v)))
        }
        Inst::BinF32 { op, dst, lhs, rhs } => step_bin_f32(regs, *op, *dst, *lhs, *rhs),
        Inst::UnF64 { op, dst, src } => {
            let v = op.apply(read_f64(regs, *src)?);
            write(regs, *dst, Slot::Native(Native::F64(v)))
        }
        Inst::BinF64 { op, dst, lhs, rhs } => step_bin_f64(regs, *op, *dst, *lhs, *rhs),
        Inst::Conv { op, dst, src } => step_conv(regs, *op, *dst, *src),
        Inst::Move { dst, src } => {
            regs[*dst as usize] = regs[*src as usize].clone();
            Ok(Step::Next)
        }
        Inst::Select { dst, a, b, cond } => {
            let pick = if read_i32(regs, *cond)? != 0 { *a } else { *b };
            regs[*dst as usize] = regs[pick as usize].clone();
            Ok(Step::Next)
        }
        Inst::Jump { target } => Ok(Step::Jump(*target)),
        Inst::JumpIf { cond, target, zero } => {
            let z = read_i32(regs, *cond)? == 0;
            if z == *zero {
                Ok(Step::Jump(*target))
            } else {
                Ok(Step::Next)
            }
        }
        Inst::JumpTable {
            index,
            targets,
            default,
        } => {
            let i = read_i32(regs, *index)? as u32 as usize;
            Ok(Step::Jump(*targets.get(i).unwrap_or(default)))
        }
        Inst::Call { func, args, dsts } => Ok(Step::Call {
            inst: vm.id(),
            func: *func,
            args: args.iter().map(|r| regs[*r as usize].clone()).collect(),
            dsts: dsts.clone(),
            tail: false,
        }),
        Inst::ReturnCall { func, args } => Ok(Step::Call {
            inst: vm.id(),
            func: *func,
            args: args.iter().map(|r| regs[*r as usize].clone()).collect(),
            dsts: Box::new([]),
            tail: true,
        }),
        Inst::ReturnCallIndirect {
            table,
            type_idx,
            index,
            args,
        } => {
            let mut step = step_call_indirect(vm, regs, *table, *type_idx, *index, args, &[])?;
            if let Step::Call { tail, .. } = &mut step {
                *tail = true;
            }
            Ok(step)
        }
        Inst::CallIndirect {
            table,
            type_idx,
            index,
            args,
            dsts,
        } => step_call_indirect(vm, regs, *table, *type_idx, *index, args, dsts),
        Inst::Return { srcs } => Ok(Step::Return(
            srcs.iter().map(|r| regs[*r as usize].clone()).collect(),
        )),
        Inst::Load {
            dst,
            addr,
            offset,
            mem,
            op,
        } => step_load(vm, regs, *dst, *addr, *offset, *mem, *op),
        Inst::Store {
            addr,
            src,
            offset,
            mem,
            op,
        } => step_store(vm, regs, *addr, *src, *offset, *mem, *op),
        Inst::MemorySize { dst, mem } => step_memory_size(vm, regs, *dst, *mem),
        Inst::MemoryGrow { dst, delta, mem } => step_memory_grow(vm, regs, *dst, *delta, *mem),
        Inst::GlobalGet { dst, global } => {
            let cell = vm
                .global(*global)
                .ok_or(Failure::Trap(Trap::Unimplemented))?;
            let value = cell.borrow().value.clone();
            write(regs, *dst, value)
        }
        Inst::GlobalSet { global, src } => {
            let value = regs[*src as usize].clone();
            let cell = vm
                .global(*global)
                .ok_or(Failure::Trap(Trap::Unimplemented))?;
            cell.borrow_mut().value = value;
            Ok(Step::Next)
        }
        Inst::RefIsNull { dst, src } => {
            let is_null = matches!(regs[*src as usize], Slot::Native(Native::Ref(RefVal::Null)));
            write(regs, *dst, Slot::Native(Native::I32(i32::from(is_null))))
        }
        Inst::RefAsNonNull { src } => {
            if matches!(regs[*src as usize], Slot::Native(Native::Ref(RefVal::Null))) {
                Err(Failure::Trap(Trap::NullReference))
            } else {
                Ok(Step::Next)
            }
        }
        Inst::RefI31 { dst, src } => {
            let v = read_i32(regs, *src)?;
            write(
                regs,
                *dst,
                Slot::Native(Native::Ref(RefVal::I31(v as u32 & 0x7fff_ffff))),
            )
        }
        Inst::I31Get { dst, src, signed } => match read_ref(regs, *src)? {
            RefVal::Null => Err(Failure::Trap(Trap::NullI31)),
            RefVal::I31(bits) => {
                let v = if *signed {
                    ((bits as i32) << 1) >> 1
                } else {
                    bits as i32
                };
                write(regs, *dst, Slot::Native(Native::I32(v)))
            }
            _ => Err(Failure::Trap(Trap::Unimplemented)),
        },
        Inst::RefEq { dst, lhs, rhs } => {
            let a = read_ref(regs, *lhs)?;
            let b = read_ref(regs, *rhs)?;
            write(regs, *dst, Slot::Native(Native::I32(i32::from(a == b))))
        }
        Inst::MemoryCopy {
            dst_mem,
            src_mem,
            dst,
            src,
            len,
        } => {
            crate::bulk::memory_copy(vm, regs, *dst_mem, *src_mem, *dst, *src, *len)?;
            Ok(Step::Next)
        }
        Inst::MemoryFill { mem, dst, val, len } => {
            crate::bulk::memory_fill(vm, regs, *mem, *dst, *val, *len)?;
            Ok(Step::Next)
        }
        Inst::MemoryInit {
            mem,
            data,
            dst,
            src,
            len,
        } => {
            crate::bulk::memory_init(vm, regs, *mem, *data, *dst, *src, *len)?;
            Ok(Step::Next)
        }
        Inst::DataDrop { data } => {
            if let Some(slot) = vm.datas().borrow_mut().get_mut(*data as usize) {
                *slot = None;
            }
            Ok(Step::Next)
        }
        Inst::TableGet { dst, table, index } => {
            crate::bulk::table_get(vm, regs, *dst, *table, *index)?;
            Ok(Step::Next)
        }
        Inst::TableSet { table, index, src } => {
            crate::bulk::table_set(vm, regs, *table, *index, *src)?;
            Ok(Step::Next)
        }
        Inst::TableSize { dst, table } => {
            crate::bulk::table_size(vm, regs, *dst, *table)?;
            Ok(Step::Next)
        }
        Inst::TableGrow {
            dst,
            table,
            fill,
            delta,
        } => {
            crate::bulk::table_grow(vm, regs, *dst, *table, *fill, *delta)?;
            Ok(Step::Next)
        }
        Inst::TableFill {
            table,
            dst,
            val,
            len,
        } => {
            crate::bulk::table_fill(vm, regs, *table, *dst, *val, *len)?;
            Ok(Step::Next)
        }
        Inst::TableCopy {
            dst_table,
            src_table,
            dst,
            src,
            len,
        } => {
            crate::bulk::table_copy(vm, regs, *dst_table, *src_table, *dst, *src, *len)?;
            Ok(Step::Next)
        }
        Inst::TableInit {
            table,
            elem,
            dst,
            src,
            len,
        } => {
            crate::bulk::table_init(vm, regs, *table, *elem, *dst, *src, *len)?;
            Ok(Step::Next)
        }
        Inst::ElemDrop { elem } => {
            if let Some(slot) = vm.elems().borrow_mut().get_mut(*elem as usize) {
                *slot = None;
            }
            Ok(Step::Next)
        }
        Inst::Wide {
            op,
            dst_lo,
            dst_hi,
            a,
            b,
            c,
            d,
        } => {
            let (lo, hi) = wide(*op, regs, *a, *b, *c, *d)?;
            write(regs, *dst_lo, Slot::Native(Native::I64(lo)))?;
            write(regs, *dst_hi, Slot::Native(Native::I64(hi)))
        }
        Inst::CallRef {
            type_idx,
            func,
            args,
            dsts,
        } => step_call_ref(vm, regs, *type_idx, *func, args, dsts, false),
        Inst::ReturnCallRef {
            type_idx,
            func,
            args,
        } => step_call_ref(vm, regs, *type_idx, *func, args, &[], true),
        Inst::SimdShuffle { dst, a, b, lanes } => {
            let av = read_v128(regs, *a)?;
            let bv = read_v128(regs, *b)?;
            write(
                regs,
                *dst,
                Slot::Native(Native::V128(shuffle(av, bv, *lanes))),
            )
        }
        Inst::Simd {
            op,
            dst,
            a,
            b,
            c,
            lane,
        } => step_simd(regs, *op, *dst, *a, *b, *c, *lane),
        Inst::Atomic {
            op,
            dst,
            addr,
            a,
            b,
            offset,
            mem,
            bytes,
            wide,
        } => {
            crate::wasm_atomic::step(
                vm, *op, *dst, *addr, *a, *b, *offset, *mem, *bytes, *wide, regs,
            )?;
            Ok(Step::Next)
        }
        Inst::BoxToDynamic { dst, src } => {
            let boxed = regs[*src as usize]
                .box_dynamic()
                .ok_or(Failure::Trap(Trap::Unimplemented))?;
            write(regs, *dst, boxed)
        }
        Inst::Gc { .. }
        | Inst::Throw { .. }
        | Inst::ThrowRef { .. }
        | Inst::Rethrow { .. }
        | Inst::TryBegin { .. }
        | Inst::TryEnd => Ok(Step::Next),
        Inst::Guard { dst, src, kind } => {
            let guarded = regs[*src as usize]
                .guard(*kind)
                .ok_or(Failure::Trap(Trap::Unimplemented))?;
            write(regs, *dst, guarded)
        }
    }
}

fn write(regs: &mut [Slot], dst: u16, value: Slot) -> Result<Step, Failure> {
    regs[dst as usize] = value;
    Ok(Step::Next)
}

fn step_bin_f32(
    regs: &mut [Slot],
    op: crate::native::BinF32,
    dst: u16,
    lhs: u16,
    rhs: u16,
) -> Result<Step, Failure> {
    let bits = op.apply(read_f32(regs, lhs)?, read_f32(regs, rhs)?);
    if op.is_rel() {
        write(regs, dst, Slot::Native(Native::I32(bits as i32)))
    } else {
        write(regs, dst, Slot::Native(Native::F32(bits)))
    }
}

fn step_bin_f64(
    regs: &mut [Slot],
    op: crate::native::BinF64,
    dst: u16,
    lhs: u16,
    rhs: u16,
) -> Result<Step, Failure> {
    let bits = op.apply(read_f64(regs, lhs)?, read_f64(regs, rhs)?);
    if op.is_rel() {
        write(regs, dst, Slot::Native(Native::I32(bits as i32)))
    } else {
        write(regs, dst, Slot::Native(Native::F64(bits)))
    }
}

fn step_conv(regs: &mut [Slot], op: ConvOp, dst: u16, src: u16) -> Result<Step, Failure> {
    let bits = bits_of(&regs[src as usize])?;
    match op.apply(bits).map_err(Failure::Trap)? {
        Bits::I32(v) => write(regs, dst, Slot::Native(Native::I32(v))),
        Bits::I64(v) => write(regs, dst, Slot::Native(Native::I64(v))),
        Bits::F32(v) => write(regs, dst, Slot::Native(Native::F32(v))),
        Bits::F64(v) => write(regs, dst, Slot::Native(Native::F64(v))),
    }
}

fn bits_of(slot: &Slot) -> Result<Bits, Failure> {
    match slot {
        Slot::Native(Native::I32(v)) => Ok(Bits::I32(*v)),
        Slot::Native(Native::I64(v)) => Ok(Bits::I64(*v)),
        Slot::Native(Native::F32(v)) => Ok(Bits::F32(*v)),
        Slot::Native(Native::F64(v)) => Ok(Bits::F64(*v)),
        // A successful Guard is already a semantic proof.  Consume the
        // guarded payload at the native boundary without boxing it again.
        Slot::Fast(Fast::I32(v)) => Ok(Bits::I32(*v)),
        Slot::Fast(Fast::Number(v)) => Ok(Bits::F64(v.to_bits())),
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn wide(op: WideOp, regs: &[Slot], a: u16, b: u16, c: u16, d: u16) -> Result<(i64, i64), Failure> {
    match op {
        WideOp::Add128 => {
            let (lo, hi) = add128(
                read_i64(regs, a)?,
                read_i64(regs, b)?,
                read_i64(regs, c)?,
                read_i64(regs, d)?,
            );
            Ok((lo, hi))
        }
        WideOp::Sub128 => {
            let (lo, hi) = sub128(
                read_i64(regs, a)?,
                read_i64(regs, b)?,
                read_i64(regs, c)?,
                read_i64(regs, d)?,
            );
            Ok((lo, hi))
        }
        WideOp::MulWideS => Ok(mul_wide_s(read_i64(regs, a)?, read_i64(regs, b)?)),
        WideOp::MulWideU => Ok(mul_wide_u(read_i64(regs, a)?, read_i64(regs, b)?)),
    }
}

fn add128(a_lo: i64, a_hi: i64, b_lo: i64, b_hi: i64) -> (i64, i64) {
    let (lo, c) = (a_lo as u64).overflowing_add(b_lo as u64);
    let hi = (a_hi as u64)
        .wrapping_add(b_hi as u64)
        .wrapping_add(u64::from(c));
    (lo as i64, hi as i64)
}

fn sub128(a_lo: i64, a_hi: i64, b_lo: i64, b_hi: i64) -> (i64, i64) {
    let (lo, brw) = (a_lo as u64).overflowing_sub(b_lo as u64);
    let hi = (a_hi as u64)
        .wrapping_sub(b_hi as u64)
        .wrapping_sub(u64::from(brw));
    (lo as i64, hi as i64)
}

fn mul_wide_s(a: i64, b: i64) -> (i64, i64) {
    let p = (a as i128).wrapping_mul(b as i128) as u128;
    (p as i64, (p >> 64) as i64)
}

fn mul_wide_u(a: i64, b: i64) -> (i64, i64) {
    let p = (a as u64 as u128).wrapping_mul(b as u64 as u128);
    (p as i64, (p >> 64) as i64)
}

fn take_catch(
    vm: &Instance,
    frames: &mut Vec<Frame>,
    current: &mut Frame,
    tag: u32,
    args: Vec<Slot>,
) -> bool {
    if catch_in(vm, current, tag, &args) {
        return true;
    }
    while let Some(mut caller) = frames.pop() {
        if catch_in(vm, &mut caller, tag, &args) {
            *current = caller;
            return true;
        }
    }
    false
}

fn catch_in(vm: &Instance, frame: &mut Frame, tag: u32, args: &[Slot]) -> bool {
    let mut i = frame.handlers.len() as i32 - 1;
    while i >= 0 {
        let idx = i as usize;
        if let [c] = frame.handlers[idx].as_ref() {
            if c.target & 0x8000_0000 != 0 {
                let d = c.target & 0x7fff_ffff;
                i -= d as i32 + 1;
                continue;
            }
        }
        for catch in frame.handlers[idx].iter() {
            if catch.tag.map(|t| vm.tag_id(t) == tag).unwrap_or(true) {
                let mut payload = args.to_vec();
                if catch.with_ref {
                    let r = crate::gc::alloc_exn(&mut vm.gc().borrow_mut(), tag, args.to_vec());
                    payload.push(Slot::Native(Native::Ref(r)));
                }
                frame.caught.push((tag, args.to_vec()));
                write_returns(&mut frame.regs, &catch.dsts, payload);
                frame.pc = catch.target as usize;
                frame.handlers.truncate(idx);
                return true;
            }
        }
        i -= 1;
    }
    false
}

fn throw_ref(vm: &Instance, regs: &[Slot], src: u16) -> Result<Step, Failure> {
    match read_ref(regs, src)? {
        RefVal::Null => Err(Failure::Trap(Trap::NullExn)),
        RefVal::Exn(id) => {
            let exn = vm
                .gc()
                .borrow()
                .exns
                .get(id as usize)
                .cloned()
                .ok_or(Failure::Trap(Trap::NullExn))?;
            Err(Failure::Exception {
                tag: exn.tag,
                args: exn.args,
            })
        }
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn shuffle(a: u128, b: u128, lanes: [u8; 16]) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let mut out = [0u8; 16];
    for i in 0..16 {
        let idx = lanes[i] as usize;
        out[i] = if idx < 16 {
            aa[idx]
        } else if idx < 32 {
            bb[idx - 16]
        } else {
            0
        };
    }
    u128::from_le_bytes(out)
}

fn step_call_ref(
    vm: &Instance,
    regs: &[Slot],
    type_idx: u32,
    func: u16,
    args: &[u16],
    dsts: &[u16],
    tail: bool,
) -> Result<Step, Failure> {
    let (inst, index) = match read_ref(regs, func)? {
        RefVal::Null => return Err(Failure::Trap(Trap::NullFunc)),
        RefVal::Func { inst, index } => (inst, index),
        _ => return Err(Failure::Trap(Trap::IndirectCallMismatch)),
    };
    if !crate::gc::func_type_ok(vm, inst, index, type_idx, false) {
        return Err(Failure::Trap(Trap::IndirectCallMismatch));
    }
    Ok(Step::Call {
        inst,
        func: index,
        args: args.iter().map(|r| regs[*r as usize].clone()).collect(),
        dsts: dsts.to_vec().into_boxed_slice(),
        tail,
    })
}

fn step_call_indirect(
    vm: &Instance,
    regs: &[Slot],
    table: u32,
    type_idx: u32,
    index: u16,
    args: &[u16],
    dsts: &[u16],
) -> Result<Step, Failure> {
    let i = match regs[index as usize] {
        Slot::Native(Native::I32(v)) => v as u32 as usize,
        Slot::Native(Native::I64(v)) => v as u64 as usize,
        _ => return Err(Failure::Trap(Trap::Unimplemented)),
    };
    let tab = vm
        .table(table)
        .ok_or(Failure::Trap(Trap::OutOfBoundsTable))?;
    let tab = tab.borrow();
    let (inst, func) = match tab.elems.get(i) {
        Some(RefVal::Func { inst, index }) => (*inst, *index),
        Some(RefVal::Null) => return Err(Failure::Trap(Trap::UninitializedElement)),
        Some(_) => return Err(Failure::Trap(Trap::IndirectCallMismatch)),
        None => return Err(Failure::Trap(Trap::UndefinedElement)),
    };
    drop(tab);
    if !crate::gc::func_type_ok(vm, inst, func, type_idx, false) {
        return Err(Failure::Trap(Trap::IndirectCallMismatch));
    }
    Ok(Step::Call {
        inst,
        func,
        args: args.iter().map(|r| regs[*r as usize].clone()).collect(),
        dsts: dsts.to_vec().into_boxed_slice(),
        tail: false,
    })
}

fn step_load(
    vm: &Instance,
    regs: &mut [Slot],
    dst: u16,
    addr: u16,
    offset: u64,
    mem: u32,
    op: LoadOp,
) -> Result<Step, Failure> {
    let memory64 = vm.memory(mem).map(|m| m.borrow().memory64).unwrap_or(false);
    let ea = instance::addr_u64(&regs[addr as usize], memory64).map_err(Failure::Trap)?;
    let (size, signed) = load_size(op);
    let bytes = instance::load_bytes(vm, mem, ea, offset, size).map_err(Failure::Trap)?;
    let value = decode_load(op, &bytes, signed);
    write(regs, dst, value)
}

fn load_size(op: LoadOp) -> (usize, bool) {
    match op {
        LoadOp::I32 | LoadOp::F32 => (4, true),
        LoadOp::I64 | LoadOp::F64 => (8, true),
        LoadOp::I32_8S | LoadOp::I64_8S => (1, true),
        LoadOp::I32_8U | LoadOp::I64_8U => (1, false),
        LoadOp::I32_16S | LoadOp::I64_16S => (2, true),
        LoadOp::I32_16U | LoadOp::I64_16U => (2, false),
        LoadOp::I64_32S => (4, true),
        LoadOp::I64_32U => (4, false),
        LoadOp::V128 => (16, false),
        LoadOp::V128Splat8 => (1, false),
        LoadOp::V128Splat16 => (2, false),
        LoadOp::V128Splat32 | LoadOp::V128Zero32 => (4, false),
        LoadOp::V128Splat64 | LoadOp::V128Zero64 => (8, false),
        LoadOp::V128Ext8x8S | LoadOp::V128Ext8x8U => (8, false),
        LoadOp::V128Ext16x4S | LoadOp::V128Ext16x4U => (8, false),
        LoadOp::V128Ext32x2S | LoadOp::V128Ext32x2U => (8, false),
    }
}

fn decode_load(op: LoadOp, bytes: &[u8], _signed: bool) -> Slot {
    match op {
        LoadOp::I32 => Slot::Native(Native::I32(i32::from_le_bytes(bytes.try_into().unwrap()))),
        LoadOp::I64 => Slot::Native(Native::I64(i64::from_le_bytes(bytes.try_into().unwrap()))),
        LoadOp::F32 => Slot::Native(Native::F32(u32::from_le_bytes(bytes.try_into().unwrap()))),
        LoadOp::F64 => Slot::Native(Native::F64(u64::from_le_bytes(bytes.try_into().unwrap()))),
        LoadOp::I32_8S => Slot::Native(Native::I32(bytes[0] as i8 as i32)),
        LoadOp::I32_8U => Slot::Native(Native::I32(bytes[0] as i32)),
        LoadOp::I32_16S => Slot::Native(Native::I32(
            i16::from_le_bytes(bytes.try_into().unwrap()) as i32
        )),
        LoadOp::I32_16U => Slot::Native(Native::I32(
            u16::from_le_bytes(bytes.try_into().unwrap()) as i32
        )),
        LoadOp::I64_8S => Slot::Native(Native::I64(bytes[0] as i8 as i64)),
        LoadOp::I64_8U => Slot::Native(Native::I64(bytes[0] as i64)),
        LoadOp::I64_16S => Slot::Native(Native::I64(
            i16::from_le_bytes(bytes.try_into().unwrap()) as i64
        )),
        LoadOp::I64_16U => Slot::Native(Native::I64(
            u16::from_le_bytes(bytes.try_into().unwrap()) as i64
        )),
        LoadOp::I64_32S => Slot::Native(Native::I64(
            i32::from_le_bytes(bytes.try_into().unwrap()) as i64
        )),
        LoadOp::I64_32U => Slot::Native(Native::I64(
            u32::from_le_bytes(bytes.try_into().unwrap()) as i64
        )),
        LoadOp::V128 => {
            let mut bits = [0u8; 16];
            bits.copy_from_slice(bytes);
            Slot::Native(Native::V128(u128::from_le_bytes(bits)))
        }
        LoadOp::V128Splat8 => Slot::Native(Native::V128(u128::from_le_bytes([bytes[0]; 16]))),
        LoadOp::V128Splat16 => {
            let v = u16::from_le_bytes(bytes.try_into().unwrap());
            Slot::Native(Native::V128(
                crate::native::SimdOp::I16x8Splat.apply(v as u128, 0),
            ))
        }
        LoadOp::V128Splat32 => {
            let v = u32::from_le_bytes(bytes.try_into().unwrap());
            Slot::Native(Native::V128(
                crate::native::SimdOp::I32x4Splat.apply(v as u128, 0),
            ))
        }
        LoadOp::V128Splat64 => {
            let v = u64::from_le_bytes(bytes.try_into().unwrap());
            Slot::Native(Native::V128(
                crate::native::SimdOp::I64x2Splat.apply(v as u128, 0),
            ))
        }
        LoadOp::V128Zero32 => {
            let mut bits = [0u8; 16];
            bits[..4].copy_from_slice(bytes);
            Slot::Native(Native::V128(u128::from_le_bytes(bits)))
        }
        LoadOp::V128Zero64 => {
            let mut bits = [0u8; 16];
            bits[..8].copy_from_slice(bytes);
            Slot::Native(Native::V128(u128::from_le_bytes(bits)))
        }
        LoadOp::V128Ext8x8S => Slot::Native(Native::V128(ext8x8(bytes, true))),
        LoadOp::V128Ext8x8U => Slot::Native(Native::V128(ext8x8(bytes, false))),
        LoadOp::V128Ext16x4S => Slot::Native(Native::V128(ext16x4(bytes, true))),
        LoadOp::V128Ext16x4U => Slot::Native(Native::V128(ext16x4(bytes, false))),
        LoadOp::V128Ext32x2S => Slot::Native(Native::V128(ext32x2(bytes, true))),
        LoadOp::V128Ext32x2U => Slot::Native(Native::V128(ext32x2(bytes, false))),
    }
}

fn ext8x8(bytes: &[u8], signed: bool) -> u128 {
    let mut out = [0u8; 16];
    for i in 0..8 {
        let v = if signed {
            bytes[i] as i8 as i16
        } else {
            bytes[i] as i16
        };
        out[i * 2..i * 2 + 2].copy_from_slice(&(v as u16).to_le_bytes());
    }
    u128::from_le_bytes(out)
}

fn ext16x4(bytes: &[u8], signed: bool) -> u128 {
    let mut out = [0u8; 16];
    for i in 0..4 {
        let s = u16::from_le_bytes(bytes[i * 2..i * 2 + 2].try_into().unwrap());
        let v = if signed { s as i16 as i32 } else { s as i32 };
        out[i * 4..i * 4 + 4].copy_from_slice(&(v as u32).to_le_bytes());
    }
    u128::from_le_bytes(out)
}

fn ext32x2(bytes: &[u8], signed: bool) -> u128 {
    let mut out = [0u8; 16];
    for i in 0..2 {
        let s = u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        let v = if signed { s as i32 as i64 } else { s as i64 };
        out[i * 8..i * 8 + 8].copy_from_slice(&(v as u64).to_le_bytes());
    }
    u128::from_le_bytes(out)
}

fn step_store(
    vm: &Instance,
    regs: &[Slot],
    addr: u16,
    src: u16,
    offset: u64,
    mem: u32,
    op: StoreOp,
) -> Result<Step, Failure> {
    let memory64 = vm.memory(mem).map(|m| m.borrow().memory64).unwrap_or(false);
    let ea = instance::addr_u64(&regs[addr as usize], memory64).map_err(Failure::Trap)?;
    let bytes = encode_store(op, &regs[src as usize])?;
    instance::store_bytes(vm, mem, ea, offset, &bytes).map_err(Failure::Trap)?;
    Ok(Step::Next)
}

fn step_simd(
    regs: &mut [Slot],
    op: crate::native::SimdOp,
    dst: u16,
    a: u16,
    b: u16,
    c: u16,
    lane: u8,
) -> Result<Step, Failure> {
    let av = simd_src(regs, a, op)?;
    let bv = simd_b(regs, op, b)?;
    let cv = if op.arity() == 3 {
        read_v128(regs, c)?
    } else {
        0
    };
    let bits = op.apply_ex(av, bv, cv, lane);
    if crate::native::simd_extra::returns_i32(op) {
        write(regs, dst, Slot::Native(Native::I32(bits as i32)))
    } else if crate::native::simd_extra::returns_i64(op) {
        write(regs, dst, Slot::Native(Native::I64(bits as i64)))
    } else if crate::native::simd_extra::returns_f32(op) {
        write(regs, dst, Slot::Native(Native::F32(bits as u32)))
    } else if crate::native::simd_extra::returns_f64(op) {
        write(regs, dst, Slot::Native(Native::F64(bits as u64)))
    } else {
        write(regs, dst, Slot::Native(Native::V128(bits)))
    }
}

fn simd_b(regs: &[Slot], op: crate::native::SimdOp, b: u16) -> Result<u128, Failure> {
    if crate::native::simd_extra::is_shift(op) {
        return Ok(read_i32(regs, b)? as u32 as u128);
    }
    if op.arity() == 1 {
        return Ok(0);
    }
    match regs[b as usize] {
        Slot::Native(Native::V128(v)) => Ok(v),
        Slot::Native(Native::I32(v)) => Ok(v as u32 as u128),
        Slot::Native(Native::I64(v)) => Ok(v as u64 as u128),
        Slot::Native(Native::F32(v)) => Ok(v as u128),
        Slot::Native(Native::F64(v)) => Ok(v as u128),
        _ => read_v128(regs, b),
    }
}

fn encode_store(op: StoreOp, slot: &Slot) -> Result<Vec<u8>, Failure> {
    Ok(match (op, slot) {
        (StoreOp::I32, Slot::Native(Native::I32(v))) => v.to_le_bytes().to_vec(),
        (StoreOp::I64, Slot::Native(Native::I64(v))) => v.to_le_bytes().to_vec(),
        (StoreOp::F32, Slot::Native(Native::F32(v))) => v.to_le_bytes().to_vec(),
        (StoreOp::F64, Slot::Native(Native::F64(v))) => v.to_le_bytes().to_vec(),
        (StoreOp::I32_8, Slot::Native(Native::I32(v))) => vec![*v as u8],
        (StoreOp::I32_16, Slot::Native(Native::I32(v))) => (*v as u16).to_le_bytes().to_vec(),
        (StoreOp::I64_8, Slot::Native(Native::I64(v))) => vec![*v as u8],
        (StoreOp::I64_16, Slot::Native(Native::I64(v))) => (*v as u16).to_le_bytes().to_vec(),
        (StoreOp::I64_32, Slot::Native(Native::I64(v))) => (*v as u32).to_le_bytes().to_vec(),
        (StoreOp::V128, Slot::Native(Native::V128(v))) => v.to_le_bytes().to_vec(),
        _ => return Err(Failure::Trap(Trap::Unimplemented)),
    })
}

fn step_memory_size(vm: &Instance, regs: &mut [Slot], dst: u16, mem: u32) -> Result<Step, Failure> {
    let memory = vm
        .memory(mem)
        .ok_or(Failure::Trap(Trap::OutOfBoundsMemory))?;
    let memory = memory.borrow();
    let pages = memory.pages();
    if memory.memory64 {
        write(regs, dst, Slot::Native(Native::I64(pages as i64)))
    } else {
        write(regs, dst, Slot::Native(Native::I32(pages as i32)))
    }
}

fn step_memory_grow(
    vm: &Instance,
    regs: &mut [Slot],
    dst: u16,
    delta: u16,
    mem: u32,
) -> Result<Step, Failure> {
    let cell = vm
        .memory(mem)
        .ok_or(Failure::Trap(Trap::OutOfBoundsMemory))?;
    let memory64 = cell.borrow().memory64;
    let delta = instance::addr_u64(&regs[delta as usize], memory64).map_err(Failure::Trap)?;
    let mut memory = cell.borrow_mut();
    let result = memory.grow(delta);
    if memory.memory64 {
        write(
            regs,
            dst,
            Slot::Native(Native::I64(result.unwrap_or(u64::MAX) as i64)),
        )
    } else {
        write(
            regs,
            dst,
            Slot::Native(Native::I32(result.map(|v| v as i32).unwrap_or(-1))),
        )
    }
}

fn read_i32(regs: &[Slot], reg: u16) -> Result<i32, Failure> {
    match regs[reg as usize] {
        Slot::Native(Native::I32(v)) => Ok(v),
        Slot::Fast(Fast::I32(v)) => Ok(v),
        Slot::Fast(Fast::Number(v))
            if v.is_finite()
                && v >= i32::MIN as f64
                && v <= i32::MAX as f64
                && (v as i32 as f64) == v =>
        {
            Ok(v as i32)
        }
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn read_i64(regs: &[Slot], reg: u16) -> Result<i64, Failure> {
    match regs[reg as usize] {
        Slot::Native(Native::I64(v)) => Ok(v),
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn read_f32(regs: &[Slot], reg: u16) -> Result<u32, Failure> {
    match regs[reg as usize] {
        Slot::Native(Native::F32(v)) => Ok(v),
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn read_f64(regs: &[Slot], reg: u16) -> Result<u64, Failure> {
    match regs[reg as usize] {
        Slot::Native(Native::F64(v)) => Ok(v),
        Slot::Fast(Fast::I32(v)) => Ok((v as f64).to_bits()),
        Slot::Fast(Fast::Number(v)) => Ok(v.to_bits()),
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn read_v128(regs: &[Slot], reg: u16) -> Result<u128, Failure> {
    match regs[reg as usize] {
        Slot::Native(Native::V128(v)) => Ok(v),
        Slot::Native(Native::I32(v)) => Ok(v as u32 as u128),
        Slot::Native(Native::I64(v)) => Ok(v as u64 as u128),
        Slot::Native(Native::F32(v)) => Ok(v as u128),
        Slot::Native(Native::F64(v)) => Ok(v as u128),
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

fn simd_src(regs: &[Slot], reg: u16, op: crate::native::SimdOp) -> Result<u128, Failure> {
    if op.arity() == 1 && !matches!(op, crate::native::SimdOp::Not) {
        read_v128(regs, reg)
    } else if matches!(op, crate::native::SimdOp::Not) {
        match regs[reg as usize] {
            Slot::Native(Native::V128(v)) => Ok(v),
            _ => Err(Failure::Trap(Trap::Unimplemented)),
        }
    } else {
        read_v128(regs, reg)
    }
}

fn read_ref(regs: &[Slot], reg: u16) -> Result<RefVal, Failure> {
    match regs[reg as usize] {
        Slot::Native(Native::Ref(v)) => Ok(v),
        _ => Err(Failure::Trap(Trap::Unimplemented)),
    }
}

#[cfg(test)]
mod tests {
    use super::{read_f64, read_i32, Fast, Slot};

    #[test]
    fn guarded_dynamic_numbers_reach_native_consumers() {
        let dynamic_i32 = Slot::Dynamic(crate::dynamic::Dynamic::from_number(7.0));
        let dynamic_number = Slot::Dynamic(crate::dynamic::Dynamic::from_number(1.5));
        let regs = vec![
            dynamic_i32
                .guard(crate::layer::GuardKind::I32)
                .expect("i32 guard"),
            dynamic_number
                .guard(crate::layer::GuardKind::Number)
                .expect("number guard"),
        ];
        assert!(matches!(regs[0], Slot::Fast(Fast::I32(7))));
        assert_eq!(read_i32(&regs, 0).expect("native i32 read"), 7);
        assert_eq!(
            read_f64(&regs, 1).expect("native f64 read"),
            1.5f64.to_bits()
        );
    }
}
