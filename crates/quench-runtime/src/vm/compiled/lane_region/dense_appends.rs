//! Appends to a loop's hoisted dense arrays: a store at the length writes the element and
//! advances the length in place (`staged::elements::append_element`) while the array's
//! proved room lasts. A reserve (`LaneHelper::AppendRoom`) proves the room: the array
//! appends as OrdinarySet defines the index (dense, extensible, its backing its own, no
//! prototype holding an index) and its backing has a slot past the length, grown when full.
//! The proof holds in a variable until a helper that may change a prototype or a backing
//! runs (`forget_append_rooms`), so a loop appends with one comparison past the in-place
//! store's and calls the VM once per growth.

use super::*;

impl Lowering<'_, '_> {
    /// The append the store of `value` at `index` to hoisted `array` at `pc` makes where its
    /// in-place write missed: within the proved room, else after a reserve, which reloads
    /// every view it may have moved. Both continue at `done`. Any other index, a view another
    /// hoisted register may alias, or a declined reserve continues at the block left
    /// current: the store's general path.
    pub(super) fn dense_append(&mut self, array: LoopArray, pc: u32, index: ir::Value, value: ir::Value,
        base: ir::Value, done: ir::Block) {
        let (reserve, attempt, general) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        for block in [reserve, attempt, general] { self.builder.set_cold_block(block); }
        self.append_within_room(array, index, value, reserve, done);
        self.builder.switch_to_block(reserve);
        self.builder.seal_block(reserve);
        let (_, length) = self.hoisted_view(array);
        let at_end = self.ins().icmp(IntCC::Equal, index, length);
        let aliased = self.aliased_view(array, pc as usize);
        let reserves = self.ins().band_not(at_end, aliased);
        self.ins().brif(reserves, super::super::forced_or(attempt, general), &[], general, &[]);
        self.builder.switch_to_block(attempt);
        self.builder.seal_block(attempt);
        let room = self.helper_call(LaneHelper::AppendRoom, &[base]);
        let granted = self.ins().icmp_imm_u(IntCC::NotEqual, room, 0);
        let reserved = self.builder.create_block();
        self.ins().brif(granted, super::super::forced_or(reserved, general), &[], general, &[]);
        self.builder.switch_to_block(reserved);
        self.builder.seal_block(reserved);
        self.reload_dense_arrays(pc as usize);
        let room = self.ins().ireduce(types::I32, room);
        let variables = self.array_variables(array);
        self.builder.def_var(variables.room, room);
        self.append_within_room(array, index, value, general, done);
        self.builder.switch_to_block(general);
        self.builder.seal_block(general);
    }

    /// The staged append through `array`'s view and room, whose new length the view takes;
    /// an index other than the length, or no room, continues at `miss`.
    fn append_within_room(&mut self, array: LoopArray, index: ir::Value, value: ir::Value, miss: ir::Block, done: ir::Block) {
        let (values, length) = self.hoisted_view(array);
        let variables = self.array_variables(array);
        let room = self.builder.use_var(variables.room);
        // The emit stage's words are 64-bit; the view's index, length and room are 32-bit.
        let [index, length, room] = [index, length, room].map(|word| self.ins().uextend(types::I64, word));
        let mut stage = Emit { builder: &mut self.builder, property_values: None };
        let grown = crate::staged::elements::append_element(&mut stage, values, length, room, index, value, miss)
            .expect("the emit stage continues past its guards");
        let grown = self.ins().ireduce(types::I32, grown);
        self.builder.def_var(variables.length, grown);
        self.ins().jump(done, &[]);
    }

    /// Whether another view hoisted around `pc` holds `array`'s array: an append through one
    /// would leave the other's length behind, so neither appends natively.
    fn aliased_view(&mut self, array: LoopArray, pc: usize) -> ir::Value {
        let others: Vec<LoopArray> = self.loop_arrays.iter()
            .filter(|other| other.source != array.source && (other.header..=other.back).contains(&pc))
            .copied().collect();
        let bits = self.array_variables(array).bits;
        let bits = self.builder.use_var(bits);
        let mut aliased = self.ins().iconst(types::I8, 0);
        for other in others {
            let other = self.array_variables(other).bits;
            let other = self.builder.use_var(other);
            let same = self.ins().icmp(IntCC::Equal, bits, other);
            aliased = self.ins().bor(aliased, same);
        }
        aliased
    }

    /// Unproves the append room of every dense view hoisted around `pc`, after a helper that
    /// may have changed a prototype or a backing: the next append reserves again.
    pub(super) fn forget_append_rooms(&mut self, pc: usize) {
        let arrays: Vec<LoopArray> = self.loop_arrays.iter()
            .filter(|array| array.views == ElementViews::Dense && (array.header..=array.back).contains(&pc)).copied().collect();
        for array in arrays {
            let room = self.array_variables(array).room;
            let unproved = self.ins().iconst(types::I32, 0);
            self.builder.def_var(room, unproved);
        }
    }
}
