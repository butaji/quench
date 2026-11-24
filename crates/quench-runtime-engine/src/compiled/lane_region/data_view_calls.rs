//! The DataView getters and setters of fixed-width elements a body calls by identity (`native_calls`), run inline on
//! their staged rows (`staged::data_view`) over the view's bytes. Each call proves its receiver's view from the record
//! (`prove_data_view`), and a record the VM has not published is published on the call's slow arm, as a typed array's
//! is (`LaneHelper::TypedArrayView`). Anything the rows leave to the VM (a range error, a detached buffer, a big-endian
//! float, a value that is no Number, an endianness that is no Boolean) leaves at the call before it changes anything.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::ImmediateLayout;
use quench_runtime::tier_surface::Native;
use quench_runtime::staged::data_view::{self, DataViewElement, DataViewWords};
use quench_runtime::staged::typed_view::TypedRecords;
use super::builtin_calls::continues;

/// A DataView getter or setter of a fixed-width element, by its element kind and direction.
#[derive(Clone, Copy)]
pub(crate) struct DataViewCall {
    element: DataViewElement,
    setter: bool,
}

impl DataViewCall {
    /// The getter or setter `native` is, where it reads or writes a fixed-width element (not Float16 or BigInt).
    pub(crate) fn of(native: Native) -> Option<Self> {
        let (element, setter) = match native {
            Native::DataViewGetInt8 => (DataViewElement::Int8, false),
            Native::DataViewGetUint8 => (DataViewElement::Uint8, false),
            Native::DataViewGetInt16 => (DataViewElement::Int16, false),
            Native::DataViewGetUint16 => (DataViewElement::Uint16, false),
            Native::DataViewGetInt32 => (DataViewElement::Int32, false),
            Native::DataViewGetUint32 => (DataViewElement::Uint32, false),
            Native::DataViewGetFloat32 => (DataViewElement::Float32, false),
            Native::DataViewGetFloat64 => (DataViewElement::Float64, false),
            Native::DataViewSetInt8 => (DataViewElement::Int8, true),
            Native::DataViewSetUint8 => (DataViewElement::Uint8, true),
            Native::DataViewSetInt16 => (DataViewElement::Int16, true),
            Native::DataViewSetUint16 => (DataViewElement::Uint16, true),
            Native::DataViewSetInt32 => (DataViewElement::Int32, true),
            Native::DataViewSetUint32 => (DataViewElement::Uint32, true),
            Native::DataViewSetFloat32 => (DataViewElement::Float32, true),
            Native::DataViewSetFloat64 => (DataViewElement::Float64, true),
            _ => return None,
        };
        Some(Self { element, setter })
    }

    /// The arguments the call takes at least: an offset, and for a setter the value after it.
    pub(crate) const fn required(self) -> u16 {
        if self.setter { 2 } else { 1 }
    }

    /// Whether a call passing `count` arguments runs inline: the required ones, and the endianness flag after them.
    pub(crate) const fn accepts(self, count: u16) -> bool {
        count == self.required() || count == self.required() + 1
    }
}

impl Lowering<'_, '_> {
    /// The DataView call `call` whose callee `exit` guarded: its result is the call's register (a Number for a getter,
    /// `undefined` for a setter), or the call leaves through `exit` before it changes anything.
    pub(crate) fn data_view_call(&mut self, state: &mut State, instruction: WideInstruction, call: DataViewCall, exit: ir::Block) {
        let window = ImmediateLayout::call_window_base(instruction.imm());
        let argc = ImmediateLayout::argument_count(instruction.imm());
        let (receiver, result) = (instruction.register_c(), instruction.result_register());
        // The offset, the setter's value, then the endianness flag, each where its call passes it.
        let index = self.int(state, window, exit);
        let index = self.raw_unary(Unary::ZeroExtend(Lane::I64), index);
        let value = call.setter.then(|| self.number(state, window + 1, exit));
        let flag = (argc == call.required() + 1).then(|| self.tagged(state, window + if call.setter { 2 } else { 1 }));
        let bits = self.tagged(state, receiver);
        let writable = call.setter;

        // The view's proof on the call's entry, and where it fails the slow arm publishes the record and proves again.
        let view = self.data_view_proof(bits, writable, exit);
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        let little = match flag {
            Some(flag) => {
                // The flag is a Boolean (ToBoolean reads nothing of any other value): anything else leaves to the VM.
                let yes = stage.eq_imm(flag, Value::TRUE.to_bits());
                let no = stage.eq_imm(flag, Value::FALSE.to_bits());
                let boolean = stage.any(yes, no);
                continues(stage.guard(boolean, exit));
                yes
            }
            None => {
                let zero = stage.constant(0);
                stage.eq_imm(zero, 1u64)
            }
        };
        match value {
            None => {
                let number = continues(data_view::data_view_read(&mut stage, view, index, call.element, little, exit));
                drop(stage);
                self.set_number(state, result, number);
            }
            Some(number) => {
                continues(data_view::data_view_write(&mut stage, view, index, call.element, little, number, exit));
                drop(stage);
                let undefined = self.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
                self.set_tagged(state, result, undefined);
            }
        }
    }

    /// The bytes of the DataView `bits` holds, proved once for this call: a proof that leaves takes the slow arm, which
    /// publishes the record (`LaneHelper::TypedArrayView`) and proves it again, leaving at `exit` where that fails still.
    fn data_view_proof(&mut self, bits: ir::Value, writable: bool, exit: ir::Block) -> DataViewWords<ir::Value> {
        let values = self.builder.declare_var(ir::types::I64);
        let length = self.builder.declare_var(ir::types::I64);
        let publish = cold_block(&mut self.builder);
        let done = self.builder.create_block();
        self.prove_data_view_into(values, length, bits, writable, publish);
        self.goto(done, &[]);
        self.builder.switch_to_block(publish);
        self.builder.seal_block(publish);
        let published = self.helper_call(LaneHelper::TypedArrayView, &[bits]);
        let admitted = self.raw_compare_imm(Relation::NotEqual, published, 0);
        emit_js_guard(&mut self.builder, admitted, exit);
        self.prove_data_view_into(values, length, bits, writable, exit);
        self.goto(done, &[]);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
        DataViewWords { values: self.builder.use_var(values), length: self.builder.use_var(length) }
    }

    /// The proof of the DataView `bits` for an access, leaving at `miss`, whose bytes the variables then hold.
    fn prove_data_view_into(&mut self, values: Variable, length: Variable, bits: ir::Value, writable: bool, miss: ir::Block) {
        let records = TypedRecords {
            directory: emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, typed_view_directory)),
            pages: emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, typed_view_pages)),
            relocations: emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, buffer_relocations)),
        };
        let view = data_view::prove_data_view(&mut super::super::object_slots::emit(&mut self.builder), records, bits, writable, miss)
            .expect("the emit stage continues past its guards");
        self.builder.def_var(values, view.values);
        self.builder.def_var(length, view.length);
    }
}
