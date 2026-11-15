//! The fused store and move records, and the i32 updates and constant stores they complete.

use super::*;

/// The i32 loads the load fusions below serve.
macro_rules! i32_loads {
    ($m:ident) => {
        $m!(WasmI32Load, WasmI32Load8S, WasmI32Load8U, WasmI32Load16S, WasmI32Load16U)
    };
}


/// The pointer-chase handler whose second load is `op`.
pub(super) fn load_load_handler<H: Host>(op: Op, acc: u8) -> Option<Handler> {
    macro_rules! loads {
        ($($load:ident),+) => {
            match (op as u16, acc) {
                $(
                    (opc::$load, ACC_NONE) => Some(lane_load_load::<H, { opc::$load }, ACC_NONE> as Handler),
                    (opc::$load, ACC_C) => Some(lane_load_load::<H, { opc::$load }, ACC_C> as Handler),
                )+
                _ => None,
            }
        };
    }
    i32_loads!(loads)
}

/// The load-and-test handler for i32 load `op` with address field `acc`, jumping on zero
/// (`zero`) in direction `backward`.
pub(super) fn load_jump_handler<H: Host>(op: Op, acc: u8, zero: bool, backward: bool) -> Option<Handler> {
    macro_rules! loads {
        ($($load:ident),+) => {
            match (op as u16, acc, zero, backward) {
                $(
                    (opc::$load, ACC_NONE, false, false) => Some(lane_load_jump_i32_zero::<H, { opc::$load }, ACC_NONE, false, false> as Handler),
                    (opc::$load, ACC_NONE, false, true) => Some(lane_load_jump_i32_zero::<H, { opc::$load }, ACC_NONE, false, true> as Handler),
                    (opc::$load, ACC_NONE, true, false) => Some(lane_load_jump_i32_zero::<H, { opc::$load }, ACC_NONE, true, false> as Handler),
                    (opc::$load, ACC_NONE, true, true) => Some(lane_load_jump_i32_zero::<H, { opc::$load }, ACC_NONE, true, true> as Handler),
                    (opc::$load, ACC_C, false, false) => Some(lane_load_jump_i32_zero::<H, { opc::$load }, ACC_C, false, false> as Handler),
                    (opc::$load, ACC_C, false, true) => Some(lane_load_jump_i32_zero::<H, { opc::$load }, ACC_C, false, true> as Handler),
                    (opc::$load, ACC_C, true, false) => Some(lane_load_jump_i32_zero::<H, { opc::$load }, ACC_C, true, false> as Handler),
                    (opc::$load, ACC_C, true, true) => Some(lane_load_jump_i32_zero::<H, { opc::$load }, ACC_C, true, true> as Handler),
                )+
                _ => None,
            }
        };
    }
    i32_loads!(loads)
}

/// The move-then-load handler for i32 load `op`, the move reading through field `acc`.
pub(super) fn move_load_handler<H: Host>(op: Op, acc: u8) -> Option<Handler> {
    macro_rules! loads {
        ($($load:ident),+) => {
            match (op as u16, acc) {
                $(
                    (opc::$load, ACC_NONE) => Some(lane_move_load::<H, { opc::$load }, ACC_NONE> as Handler),
                    (opc::$load, ACC_B) => Some(lane_move_load::<H, { opc::$load }, ACC_B> as Handler),
                )+
                _ => None,
            }
        };
    }
    i32_loads!(loads)
}

lane_handler! {
    /// A plain direct store `OP`, a move and a zero-test jump (`WHEN_ZERO`, `BACKWARD`) testing
    /// the register the record before them carries as the accumulator, which the taken jump
    /// carries on to its target (a loop latch whose head reads the register there): three
    /// records run here, the moved value carried as the move carries it when falling through.
    fn lane_store_move_jump_i32_zero<OP: u16, WHEN_ZERO: bool, BACKWARD: bool>(cx, r, ip, i, view) {
        let store = const {
            match MemoryStore::from_direct_op(opcode(OP)) {
                Some(store) => store,
                None => panic!("not a direct store"),
            }
        };
        // SAFETY: the view selects this record only after the producer that set it.
        let tested = unsafe { view.acc.get() };
        let Some(value) = stored_value(store, r.get(i.a)) else {
            return exit(ip);
        };
        // SAFETY: as for `lane_load`.
        if unsafe { view.memory.write(store, lane_address::<ACC_NONE>(r, i, view), value) }
            .is_none()
        {
            return exit(ip);
        }
        let move_ip = following(ip);
        // SAFETY: the view fuses a store only with a following move and zero-test jump.
        let moved = unsafe { &*move_ip };
        let value = r.get(moved.b);
        r.set(moved.a, value);
        let jump_ip = following(move_ip);
        if (tested.wasm_bits32() == 0) == WHEN_ZERO {
            resume!(cx, lane_jump::<H>(r, jump_ip, BACKWARD, Carried { acc: Slot::of(tested), ..view }, None));
        }
        next!(cx, r, following(jump_ip), Carried { acc: Slot::of(value), ..view })
    }
}

/// The store-move-latch handler for direct store `op`.
pub(super) fn store_move_jump_handler<H: Host>(op: Op, zero: bool, backward: bool) -> Option<Handler> {
    macro_rules! stores {
        ($($store:ident),+) => {
            match (op as u16, zero, backward) {
                $(
                    (opc::$store, false, false) => Some(lane_store_move_jump_i32_zero::<H, { opc::$store }, false, false> as Handler),
                    (opc::$store, false, true) => Some(lane_store_move_jump_i32_zero::<H, { opc::$store }, false, true> as Handler),
                    (opc::$store, true, false) => Some(lane_store_move_jump_i32_zero::<H, { opc::$store }, true, false> as Handler),
                    (opc::$store, true, true) => Some(lane_store_move_jump_i32_zero::<H, { opc::$store }, true, true> as Handler),
                )+
                _ => None,
            }
        };
    }
    stores!(WasmI32Store, WasmI32Store8, WasmI32Store16)
}

/// The store-then-move handler for direct store `op` reading through field `acc`.
pub(super) fn store_move_handler<H: Host>(op: Op, acc: u8) -> Option<Handler> {
    macro_rules! stores {
        ($($store:ident),+) => {
            match (op as u16, acc) {
                $(
                    (opc::$store, ACC_NONE) => Some(lane_store_move::<H, { opc::$store }, ACC_NONE> as Handler),
                    (opc::$store, ACC_A) => Some(lane_store_move::<H, { opc::$store }, ACC_A> as Handler),
                    (opc::$store, ACC_C) => Some(lane_store_move::<H, { opc::$store }, ACC_C> as Handler),
                )+
                _ => None,
            }
        };
    }
    stores!(WasmI32Store, WasmI32Store8, WasmI32Store16)
}

/// The scalar direct store `store` writes for slot value `value`: registers
/// hold their static Wasm types, and a value whose bits need a box has none.
#[inline(always)]
pub(super) fn stored_value(store: MemoryStore, value: Value) -> Option<crate::WasmValue> {
    Some(match store.value_type() {
        crate::WasmType::I32 => crate::WasmValue::I32(value.wasm_bits32() as i32),
        crate::WasmType::F32 => crate::WasmValue::F32(value.as_int()? as u32),
        crate::WasmType::I64 => crate::WasmValue::I64(value.bits64()? as i64),
        crate::WasmType::F64 => crate::WasmValue::F64(value.bits64()?),
        _ => return None,
    })
}

lane_handler! {
    fn lane_store<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let store = const {
            match MemoryStore::from_direct_op(opcode(OP)) {
                Some(store) => store,
                None => panic!("not a direct store"),
            }
        };
        let Some(value) = stored_value(store, read::<ACC, ACC_A>(r, i.a, view)) else {
            return exit(ip);
        };
        // SAFETY: as for `lane_load`.
        if unsafe { view.memory.write(store, lane_address::<ACC>(r, i, view), value) }
            .is_none()
        {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// An i32 load, with address field `ACC`, the i32 operator `OP` taking
    /// the loaded value as its left (`LEFT`) or right operand, and the i32
    /// store writing the operator's result (`mem[x] op= v`): three records
    /// run here, each result written as its own record writes it. An access
    /// the general path must run, or a trapping operator, exits at its
    /// record.
    fn lane_i32_update<OP: u16, LEFT: bool, ACC: u8>(cx, r, ip, i, view) {
        let operator = const {
            match I32BinaryOperator::from_register_op(opcode(OP)) {
                Some(operator) => operator,
                None => panic!("not an i32 register operator"),
            }
        };
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(loaded)) = (unsafe {
            view.memory.read(MemoryLoad::I32Load, lane_address::<ACC>(r, i, view))
        }) else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(loaded));
        let combine = following(ip);
        // SAFETY: the view fuses a load only with a following operator and
        // store.
        let o = unsafe { &*combine };
        let (left, right) = if LEFT { (loaded, r.i32(o.c)) } else { (r.i32(o.b), loaded) };
        let Ok(result) = operator.evaluate(left, right) else {
            return exit(combine);
        };
        r.set(o.a, view.integers.encode(result));
        let store = following(combine);
        // SAFETY: as above.
        let access = unsafe { &*store };
        // SAFETY: as for `lane_store`.
        if unsafe {
            view.memory.write(
                MemoryStore::I32Store,
                lane_address::<ACC_NONE>(r, access, view),
                crate::WasmValue::I32(result),
            )
        }
        .is_none()
        {
            return exit(store);
        }
        next!(cx, r, following(store), view)
    }
}

/// The i32 update handler for register operator `op`, loaded value side
/// `left` and load address field `acc`.
pub(super) fn i32_update_handler<H: Host>(op: Op, left: bool, acc: u8) -> Option<Handler> {
    macro_rules! updates {
        ($($op:ident),+) => {
            match (op as u16, left, acc) {
                $(
                    (opc::$op, false, ACC_NONE) => Some(lane_i32_update::<H, { opc::$op }, false, ACC_NONE> as Handler),
                    (opc::$op, true, ACC_NONE) => Some(lane_i32_update::<H, { opc::$op }, true, ACC_NONE> as Handler),
                    (opc::$op, false, ACC_C) => Some(lane_i32_update::<H, { opc::$op }, false, ACC_C> as Handler),
                    (opc::$op, true, ACC_C) => Some(lane_i32_update::<H, { opc::$op }, true, ACC_C> as Handler),
                )+
                _ => None,
            }
        };
    }
    updates!(
        WasmI32Add,
        WasmI32Subtract,
        WasmI32Multiply,
        WasmI32And,
        WasmI32Or,
        WasmI32Xor,
        WasmI32ShiftLeft,
        WasmI32ShiftRightSigned,
        WasmI32ShiftRightUnsigned,
        WasmI32Equal,
        WasmI32NotEqual,
        WasmI32LessSigned,
        WasmI32LessUnsigned
    )
}

lane_handler! {
    /// A constant that the next record, direct store `OP`, writes to
    /// memory: both records run here, the constant written to its register
    /// as its own record writes it.
    fn lane_constant_store<OP: u16>(cx, r, ip, i, view) {
        let store = const {
            match MemoryStore::from_direct_op(opcode(OP)) {
                Some(store) => store,
                None => panic!("not a direct store"),
            }
        };
        let constant = i.constant();
        r.set(i.a, constant);
        let next = following(ip);
        // SAFETY: the view fuses a constant only with a following store.
        let access = unsafe { &*next };
        let Some(value) = stored_value(store, constant) else {
            return exit(next);
        };
        // SAFETY: as for `lane_load`.
        if unsafe { view.memory.write(store, lane_address::<ACC_NONE>(r, access, view), value) }
            .is_none()
        {
            return exit(next);
        }
        next!(cx, r, following(next), view)
    }
}

/// The constant-store handler for direct store `op`.
pub(super) const fn constant_store_handler<H: Host>(op: Op) -> Option<Handler> {
    macro_rules! stores {
        ($($store:ident),+) => {
            match op as u16 {
                $(opc::$store => Some(lane_constant_store::<H, { opc::$store }> as Handler),)+
                _ => None,
            }
        };
    }
    stores!(
        WasmI32Store,
        WasmI64Store,
        WasmF32Store,
        WasmF64Store,
        WasmI32Store8,
        WasmI32Store16,
        WasmI64Store8,
        WasmI64Store16,
        WasmI64Store32
    )
}
