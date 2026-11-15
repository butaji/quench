//! The fused load records: a direct load and the store, add or access that completes it.

use super::*;

/// The fused handler for direct load `load`, with address field `acc`, and
/// direct store `store` writing its value: copies of one value type.
pub(super) fn load_store_handler<H: Host>(load: Op, store: Op, acc: u8) -> Option<Handler> {
    macro_rules! copies {
        ($(($load:ident, $store:ident)),+) => {
            match (load as u16, store as u16, acc) {
                $(
                    (opc::$load, opc::$store, ACC_NONE) => Some(lane_load_store::<H, { opc::$load }, { opc::$store }, ACC_NONE> as Handler),
                    (opc::$load, opc::$store, ACC_C) => Some(lane_load_store::<H, { opc::$load }, { opc::$store }, ACC_C> as Handler),
                )+
                _ => None,
            }
        };
    }
    copies!(
        (WasmI32Load, WasmI32Store),
        (WasmI32Load8U, WasmI32Store8),
        (WasmI64Load, WasmI64Store)
    )
}

/// The fused handler for i32 load `op`, with address field `acc`, the
/// branch table selecting on its value, and with `add` the plain add of a
/// constant ahead of the load.
pub(super) fn load_table_handler<H: Host>(op: Op, acc: u8, add: bool) -> Option<Handler> {
    macro_rules! loads {
        ($($load:ident),+) => {
            match (op as u16, acc, add) {
                $(
                    (opc::$load, ACC_NONE, false) => Some(lane_load_table::<H, { opc::$load }, ACC_NONE, false> as Handler),
                    (opc::$load, ACC_C, false) => Some(lane_load_table::<H, { opc::$load }, ACC_C, false> as Handler),
                    (opc::$load, ACC_NONE, true) => Some(lane_load_table::<H, { opc::$load }, ACC_NONE, true> as Handler),
                    (opc::$load, ACC_C, true) => Some(lane_load_table::<H, { opc::$load }, ACC_C, true> as Handler),
                )+
                _ => None,
            }
        };
    }
    loads!(
        WasmI32Load,
        WasmI32Load8S,
        WasmI32Load8U,
        WasmI32Load16S,
        WasmI32Load16U
    )
}

pub(super) const fn add_load_handler<H: Host>(op: Op) -> Option<Handler> {
    macro_rules! loads {
        ($($load:ident),+) => {
            match op as u16 {
                $(opc::$load => Some(lane_add_load::<H, { opc::$load }> as Handler),)+
                _ => None,
            }
        };
    }
    loads!(
        WasmI32Load,
        WasmI64Load,
        WasmF32Load,
        WasmF64Load,
        WasmI32Load8S,
        WasmI32Load8U,
        WasmI32Load16S,
        WasmI32Load16U,
        WasmI64Load8S,
        WasmI64Load8U,
        WasmI64Load16S,
        WasmI64Load16U,
        WasmI64Load32S,
        WasmI64Load32U
    )
}

lane_handler! {
    /// A direct load `LOAD`, with address field `ACC`, whose value the next
    /// record, direct store `STORE`, writes: both records run here, the value
    /// written to its register as the load writes it. An access the general
    /// path must run exits at its record.
    fn lane_load_store<LOAD: u16, STORE: u16, ACC: u8>(cx, r, ip, i, view) {
        let (load, store) = const {
            match (
                MemoryLoad::from_direct_op(opcode(LOAD)),
                MemoryStore::from_direct_op(opcode(STORE)),
            ) {
                (Some(load), Some(store)) => (load, store),
                _ => panic!("not a direct load and store"),
            }
        };
        // SAFETY: as for `lane_load`.
        let Some(value) = (unsafe { view.memory.read(load, lane_address::<ACC>(r, i, view)) })
        else {
            return exit(ip);
        };
        let Some(encoded) = lane_encode(view, value) else {
            return exit(ip);
        };
        r.set(i.a, encoded);
        let next = following(ip);
        // SAFETY: the view fuses a load only with a following store record.
        let access = unsafe { &*next };
        // SAFETY: as for `lane_load`.
        if unsafe { view.memory.write(store, lane_address::<ACC_NONE>(r, access, view), value) }
            .is_none()
        {
            return exit(next);
        }
        next!(cx, r, following(next), view)
    }
}

/// The direct load an instantiation serves.
pub(super) const fn direct_load(op: u16) -> MemoryLoad {
    match MemoryLoad::from_direct_op(opcode(op)) {
        Some(load) => load,
        None => panic!("not a direct load"),
    }
}

lane_handler! {
    /// An i32 load, with address field `ACC`, whose value the next record, direct i32 load
    /// `SECOND`, takes as its address (a pointer chase, `p->field->member`): both records run
    /// here, each value written as its own record writes it. An access the general path must
    /// run exits at its record.
    fn lane_load_load<SECOND: u16, ACC: u8>(cx, r, ip, i, view) {
        let first = const { direct_load(opc::WasmI32Load) };
        let second = const { direct_load(SECOND) };
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(pointer)) =
            (unsafe { view.memory.read(first, lane_address::<ACC>(r, i, view)) })
        else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(pointer));
        let next = following(ip);
        // SAFETY: the view fuses a load only with a following load reading its value.
        let access = unsafe { &*next };
        let address = u64::from(pointer as u32) + u64::from(access.imm());
        // SAFETY: as for `lane_load`.
        let Some(value) = (unsafe { view.memory.read(second, address) }) else {
            return exit(next);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(next);
        };
        produce!(cx, r, next, access.a, value, view)
    }
}

lane_handler! {
    /// A direct i32 load `OP`, with address field `ACC`, whose value the next record, a
    /// zero-test jump without a carried register, tests (`p = p->next; while (p)`): both
    /// records run here, the value carried as the load carries it.
    fn lane_load_jump_i32_zero<OP: u16, ACC: u8, WHEN_ZERO: bool, BACKWARD: bool>(cx, r, ip, i, view) {
        let load = const { direct_load(OP) };
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(value)) =
            (unsafe { view.memory.read(load, lane_address::<ACC>(r, i, view)) })
        else {
            return exit(ip);
        };
        let encoded = view.integers.encode(value);
        r.set(i.a, encoded);
        let view = Carried { acc: Slot::of(encoded), ..view };
        let next = following(ip);
        if (value == 0) == WHEN_ZERO {
            resume!(cx, lane_jump::<H>(r, next, BACKWARD, view, None));
        }
        next!(cx, r, following(next), view)
    }
}

lane_handler! {
    /// A move, reading its source through field `ACC`, whose moved value the next record,
    /// direct i32 load `OP`, takes as its address: both records run here, each value written
    /// as its own record writes it.
    fn lane_move_load<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let load = const { direct_load(OP) };
        let moved = read::<ACC, ACC_B>(r, i.b, view);
        r.set(i.a, moved);
        let next = following(ip);
        // SAFETY: the view fuses a move only with a following load reading its value.
        let access = unsafe { &*next };
        let address = u64::from(moved.wasm_bits32()) + u64::from(access.imm());
        // SAFETY: as for `lane_load`.
        let Some(value) = (unsafe { view.memory.read(load, address) }) else {
            return exit(next);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(next);
        };
        produce!(cx, r, next, access.a, value, view)
    }
}

lane_handler! {
    /// A direct store `OP`, reading through field `ACC`, followed by a move: both records run
    /// here, the moved value carried as the move carries it.
    fn lane_store_move<OP: u16, ACC: u8>(cx, r, ip, i, view) {
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
        let next = following(ip);
        // SAFETY: the view fuses a store only with a following move.
        let moved = unsafe { &*next };
        produce!(cx, r, next, moved.a, r.get(moved.b), view)
    }
}

lane_handler! {
    /// Two direct i32 loads `LOAD` and the i32 register operator `OP` combining their values
    /// (`a[i] * b[j]`), the first value as its left operand when `FIRST_LEFT`: three records
    /// run here, the loads issued together, each value written as its own record writes it.
    /// An access the general path must run exits at its record.
    fn lane_load_pair_binary<LOAD: u16, OP: u16, FIRST_LEFT: bool>(cx, r, ip, i, view) {
        let load = const { direct_load(LOAD) };
        let operator = const {
            match I32BinaryOperator::from_register_op(opcode(OP)) {
                Some(operator) => operator,
                None => panic!("not an i32 register operator"),
            }
        };
        let second_ip = following(ip);
        // SAFETY: the view fuses a load only with a following load and operator.
        let second_record = unsafe { &*second_ip };
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(first)) =
            (unsafe { view.memory.read(load, lane_address::<ACC_NONE>(r, i, view)) })
        else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(first));
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(second)) =
            (unsafe { view.memory.read(load, lane_address::<ACC_NONE>(r, second_record, view)) })
        else {
            return exit(second_ip);
        };
        r.set(second_record.a, view.integers.encode(second));
        let combine = following(second_ip);
        // SAFETY: as above.
        let o = unsafe { &*combine };
        let (left, right) = if FIRST_LEFT { (first, second) } else { (second, first) };
        let Ok(result) = operator.evaluate(left, right) else {
            return exit(combine);
        };
        produce!(cx, r, combine, o.a, view.integers.encode(result), view)
    }
}
