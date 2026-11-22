//! The lane frame's element callouts (`staged::ElementFrame`): the array record table and the keys
//! the lowering holds, the array views it proved once (`LocalProofs::arrays`), the VM's read past
//! a hole and its element store. The rows decide what an element access is; this frame owns the
//! facts it is proved under, kept in `State` beside the registers'.

use super::*;
use quench_runtime::staged::flow::FlowStage;
use quench_runtime::staged::machine::{Lane, MachineScalar, Relation, Unary};
use super::frame::{calls, held_long, operand_int, register_tagged};
use quench_runtime::bytecode::{Operand, OperandKind};
use quench_runtime::staged::element_rows::ElementView;
use quench_runtime::staged::elements::DenseView;
use quench_runtime::staged::typed_elements::TypedView;
use quench_runtime::staged::{ElementFrame, Region, Stage};
use quench_runtime::staged::barrier::MarkList;
use crate::compiled::{emit_js_guard, emit_unlogged_property_store_guard};

/// The view of a loop's hoisted array an element row reads and writes through, which the loop's
/// entering edge proved once: a dense array's backing, or a typed array's elements.
#[derive(Clone, Copy)]
pub(crate) enum Held {
    /// A dense array's view, and what its access proves (`DenseView::packed`, `DenseView::within`).
    Dense(super::super::DenseArrayView, (bool, bool)),
    Typed(TypedView<ir::Value>),
}

/// The barrier gate of a store of the reference `value` into the direct array `base`, read from the
/// dispatch `payload`: continues where the array's record mark bit is clear
/// (`emit_unlogged_property_store_guard` over the array records' mark list, the test `Heap::store_logs`
/// makes), and leaves at `miss` for a base that is no direct array or a marked record. An immediate
/// `value` passes the test inline, as `emit_unlogged_property_store_guard` decides it, while old
/// records keep no counts; under counts every store into a marked record logs, so the gate word's
/// counts bit leaves at `miss` first.
pub(crate) fn store_gate(stage: &mut Emit<'_, '_>, payload: ir::Value, base: ir::Value, value: ir::Value,
    miss: ir::Block) -> Option<()> {
    let uncounted = stage.counts_clear(payload);
    emit_js_guard(stage.builder, uncounted, miss);
    let direct = quench_runtime::staged::value::is_direct_array(stage, base);
    stage.guard(direct, miss)?;
    let record = stage.and_imm(base, u64::from(u32::MAX));
    let list = MarkList {
        blocks: std::mem::offset_of!(JsStepDispatch, head.array_mark_blocks),
        count: std::mem::offset_of!(JsStepDispatch, head.array_mark_block_count),
    };
    emit_unlogged_property_store_guard(stage, list, payload, record, value, miss);
    Some(())
}

/// The VM's element read of the tagged `base` and `key` (`JsStepDispatch::element_get`): the
/// element the array and its prototypes give a hole or an index past them; the sentinel it answers
/// where the VM's own step must (`Vm::absent_array_element` declines) leaves at `miss`.
pub(crate) fn index_get(frame: &EmitFrame<'_>, stage: &mut Emit<'_, '_>, (base, key): (ir::Value, ir::Value),
    miss: ir::Block) -> ir::Value {
    let helper = emit_js_payload_word(stage.builder, frame.payload, std::mem::offset_of!(JsStepDispatch, element_get));
    let read = emit_lane_helper_call(stage.builder, frame.payload, helper, &[base, key]);
    let found = stage.raw_compare_imm(Relation::NotEqual, read, Value::DELETED.to_bits() as i64);
    emit_js_guard(stage.builder, found, miss);
    read
}

impl<'a, 'b, 'c> ElementFrame<Emit<'b, 'c>> for EmitFrame<'a> {
    /// The VM's store; a hoisted array's store leaves at `miss` instead, which no recorded miss
    /// planned a helper arm for (`Lowering::element_write`): the helper's call spills every live
    /// value, which a loop that never misses should not pay.
    fn index_set_host(&mut self, stage: &mut Emit<'b, 'c>, base: ir::Value, key: ir::Value, value: ir::Value,
        miss: ir::Block) -> Option<()> {
        if self.held.is_some() {
            let never = stage.ult(key, key);
            return stage.guard(never, miss);
        }
        calls::index_set(self, stage, (base, key, value), miss);
        Some(())
    }

    /// Continues where a store of the reference `value` into the direct array `base` passes the write
    /// barrier unlogged: the array's record mark bit is clear (`emit_unlogged_property_store_guard` over
    /// the array records' mark list, the same test `Heap::store_logs` makes). A base that is no direct
    /// array, or a marked one, leaves at `miss`: the general store logs it, which clears the mark, so
    /// the next store of the loop passes here without a call.
    fn array_store_gate(&mut self, stage: &mut Emit<'b, 'c>, base: ir::Value, value: ir::Value, miss: ir::Block)
        -> Option<()> {
        store_gate(stage, self.payload, base, value, miss)
    }

    fn array_records(&mut self, stage: &mut Emit<'b, 'c>) -> Option<ir::Value> {
        let at = if self.arrays_fixed { Region::RecordBases.readonly() } else { Region::RecordBases.mutable() };
        Some(stage.load_u64(at, self.payload, std::mem::offset_of!(JsStepDispatch, head.array_record_base) as i32))
    }

    /// A register's tagged view, or an immediate constant's bits.
    fn key_word(&mut self, stage: &mut Emit<'b, 'c>, key: Operand) -> Option<ir::Value> {
        match key.kind()? {
            OperandKind::Register => Some(register_tagged(stage.builder, self.registers, self.state, key.register_index()?)),
            OperandKind::Constant => {
                let value = self.constants.get(usize::from(key.payload())).and_then(super::super::facts::immediate)?;
                Some(stage.raw_const(Lane::I64, value.to_bits() as i64))
            }
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    /// The key's index word, from the representation its fact proved: a 64-bit integer word as it
    /// is (`held_long`), else its int32 (`operand_int`), which the rows zero-extend.
    fn index_key(&mut self, stage: &mut Emit<'b, 'c>, key: Operand, miss: ir::Block) -> Option<ir::Value> {
        let long = key.register_index().and_then(|register| held_long(stage.builder, self.registers, self.state, register));
        long.or_else(|| operand_int(stage.builder, self.registers, self.state, self.constants, key, miss))
    }

    /// The view an earlier access of the same array value proved, which no storage change has
    /// retired, or the hoisted array's (`Held`); a writable view answers a read.
    fn kept_view(&mut self, stage: &mut Emit<'b, 'c>, base: ir::Value, writable: bool) -> Option<ElementView<ir::Value>> {
        let held = match self.held {
            Some(Held::Typed(view)) => return Some(ElementView::Typed(view)),
            Some(Held::Dense(view, proved)) => Some((view, proved)).filter(|(view, _)| view.writable || !writable),
            None => None,
        };
        let (view, (packed, within)) = held.or_else(|| self.state.proofs.arrays.iter()
            .find(|(known, view)| *known == base && (view.writable || !writable)).map(|(_, view)| (*view, (false, false))))?;
        Some(ElementView::Dense(DenseView { values: view.values, length: stage.uextend32(view.length), packed, within, double: view.double,
            uncounted: view.uncounted }))
    }

    fn keep_view(&mut self, stage: &mut Emit<'b, 'c>, base: ir::Value, writable: bool, view: DenseView<ir::Value>) {
        let length = stage.raw_unary(Unary::Narrow(Lane::I32), view.length);
        self.state.proofs.arrays.retain(|(known, _)| *known != base);
        self.state.proofs.arrays.push((base, super::super::DenseArrayView { values: view.values, length, writable, uncounted: false, double: view.double }));
    }

    /// The dense read, and where it leaves (a hole, an index past the backing) the VM's, which
    /// starts from the proofs that held before the read; past the join only they do.
    fn element_probed(&mut self, stage: &mut Emit<'b, 'c>, miss: ir::Block,
        fast: impl FnOnce(&mut Emit<'b, 'c>, &mut Self, ir::Block) -> Option<ir::Value>,
        host: impl FnOnce(&mut Emit<'b, 'c>, &mut Self, ir::Block) -> Option<ir::Value>) -> Option<ir::Value> {
        let (absent, join) = (stage.builder.create_block(), stage.builder.create_block());
        stage.builder.set_cold_block(absent);
        let value = stage.builder.append_block_param(join, types::I64);
        // What holds before the read: the fast path's facts (a tagged view it boxed, a view it proved) hold
        // only in the blocks it dominates, so the VM's read, and past the join everything, start from these.
        let before = (!self.hole_leaves).then(|| self.state.clone());
        match fast(stage, self, absent) {
            Some(read) => { stage.jump(join, &[read]); }
            None => { stage.jump(absent, &[]); }
        }
        stage.builder.switch_to_block(absent);
        if self.hole_leaves {
            stage.jump(miss, &[]);
            stage.builder.switch_to_block(join);
            return Some(value);
        }
        let before = before.expect("a read whose hole does not leave keeps the state before it");
        *self.state = before.clone();
        let hosted = host(stage, self, miss);
        *self.state = before;
        stage.jump(join, &[hosted?]);
        stage.builder.switch_to_block(join);
        Some(value)
    }
}
