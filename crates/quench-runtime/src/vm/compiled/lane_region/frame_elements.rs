//! The lane frame's element callouts (`staged::ElementFrame`): the array record table and the keys
//! the lowering holds, the array views it proved once (`LocalProofs::arrays`), the VM's read past
//! a hole and its element store. The rows decide what an element access is; this frame owns the
//! facts it is proved under, kept in `State` beside the registers'.

use super::*;
use super::frame::{calls, operand_int};
use crate::bytecode::{Operand, OperandKind};
use crate::staged::element_rows::ElementView;
use crate::staged::elements::DenseView;
use crate::staged::typed_elements::TypedView;
use crate::staged::{ElementFrame, Stage};

/// The view of a loop's hoisted array an element row reads and writes through, which the loop's
/// entering edge proved once: a dense array's backing, or a typed array's elements.
#[derive(Clone, Copy)]
pub(super) enum Held {
    /// A dense array's view, and whether its read proves the element (`DenseView::packed`).
    Dense(super::super::DenseArrayView, bool),
    Typed(TypedView<ir::Value>),
}

/// The VM's element read of the tagged `base` and `key` (`JsStepDispatch::element_get`): the
/// element the array and its prototypes give a hole or an index past them; the sentinel it answers
/// where the VM's own step must (`Vm::absent_array_element` declines) leaves at `miss`.
pub(super) fn index_get(frame: &EmitFrame<'_>, stage: &mut Emit<'_, '_>, (base, key): (ir::Value, ir::Value),
    miss: ir::Block) -> ir::Value {
    let helper = emit_js_payload_word(stage.builder, frame.payload, std::mem::offset_of!(JsStepDispatch, element_get));
    let read = emit_lane_helper_call(stage.builder, frame.payload, helper, &[base, key]);
    let found = stage.builder.ins().icmp_imm_u(IntCC::NotEqual, read, Value::DELETED.to_bits() as i64);
    emit_js_guard(stage.builder, found, miss);
    read
}

impl<'a, 'b, 'c> ElementFrame<Emit<'b, 'c>> for EmitFrame<'a> {
    fn index_set_host(&mut self, stage: &mut Emit<'b, 'c>, base: ir::Value, key: ir::Value, value: ir::Value,
        miss: ir::Block) -> Option<()> {
        calls::index_set(self, stage, (base, key, value), miss);
        Some(())
    }

    fn array_records(&mut self, stage: &mut Emit<'b, 'c>) -> Option<ir::Value> {
        Some(emit_js_payload_word(stage.builder, self.payload, std::mem::offset_of!(JsStepDispatch, array_record_base)))
    }

    /// A register's tagged view, or an immediate constant's bits.
    fn key_word(&mut self, stage: &mut Emit<'b, 'c>, key: Operand) -> Option<ir::Value> {
        match key.kind()? {
            OperandKind::Register => Some(register_tagged(stage.builder, self.registers, self.state, key.register_index()?)),
            OperandKind::Constant => {
                let value = self.constants.get(usize::from(key.payload())).and_then(super::super::facts::immediate)?;
                Some(stage.builder.ins().iconst(types::I64, value.to_bits() as i64))
            }
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    /// The key's int32, from the representation its fact proved (`operand_int`).
    fn index_key(&mut self, stage: &mut Emit<'b, 'c>, key: Operand, miss: ir::Block) -> Option<ir::Value> {
        operand_int(stage.builder, self.registers, self.state, self.constants, key, miss)
    }

    /// The view an earlier access of the same array value proved, which no storage change has
    /// retired, or the hoisted array's (`Held`); a writable view answers a read.
    fn kept_view(&mut self, stage: &mut Emit<'b, 'c>, base: ir::Value, writable: bool) -> Option<ElementView<ir::Value>> {
        let held = match self.held {
            Some(Held::Typed(view)) => return Some(ElementView::Typed(view)),
            Some(Held::Dense(view, packed)) => Some((view, packed)).filter(|(view, _)| view.writable || !writable),
            None => None,
        };
        let (view, packed) = held.or_else(|| self.state.proofs.arrays.iter()
            .find(|(known, view)| *known == base && (view.writable || !writable)).map(|(_, view)| (*view, false)))?;
        Some(ElementView::Dense(DenseView { values: view.values, length: stage.uextend32(view.length), packed }))
    }

    fn keep_view(&mut self, stage: &mut Emit<'b, 'c>, base: ir::Value, writable: bool, view: DenseView<ir::Value>) {
        let length = stage.builder.ins().ireduce(types::I32, view.length);
        self.state.proofs.arrays.retain(|(known, _)| *known != base);
        self.state.proofs.arrays.push((base, super::super::DenseArrayView { values: view.values, length, writable, uncounted: false }));
    }

    /// The dense read, and where it leaves (a hole, an index past the backing) the VM's, which
    /// starts from the proofs that held before the read; past the join only they do.
    fn element_probed(&mut self, stage: &mut Emit<'b, 'c>, miss: ir::Block,
        fast: impl FnOnce(&mut Emit<'b, 'c>, &mut Self, ir::Block) -> Option<ir::Value>,
        host: impl FnOnce(&mut Emit<'b, 'c>, &mut Self, ir::Block) -> Option<ir::Value>) -> Option<ir::Value> {
        let (absent, join) = (stage.builder.create_block(), stage.builder.create_block());
        stage.builder.set_cold_block(absent);
        let value = stage.builder.append_block_param(join, types::I64);
        match fast(stage, self, absent) {
            Some(read) => { stage.builder.ins().jump(join, &[ir::BlockArg::Value(read)]); }
            None => { stage.builder.ins().jump(absent, &[]); }
        }
        stage.builder.switch_to_block(absent);
        let mut dominating = self.state.clone();
        std::mem::swap(&mut *self.state, &mut dominating);
        let hosted = host(stage, self, miss);
        std::mem::swap(&mut *self.state, &mut dominating);
        stage.builder.ins().jump(join, &[ir::BlockArg::Value(hosted?)]);
        stage.builder.switch_to_block(join);
        Some(value)
    }
}
