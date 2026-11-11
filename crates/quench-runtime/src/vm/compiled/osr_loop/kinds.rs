//! The element kinds an OSR loop's entry proves and keeps: int32s (`OTHERS_FROM_VALUES`) and
//! raw doubles (`DOUBLE_ELEMENTS`), which decide the reads' and stores' tag work.

use super::*;

impl<'a, 'b> Lowering<'a, 'b> {
    /// Proves at entry the element kinds of the region's arrays: int32 arrays by their count
    /// of other values (`prove_int32_kinds`), raw-double arrays by their capacity word's
    /// kind flag. A raw-double array another compilation promoted stays promoted until a
    /// non-Number store clears the kind; then this entry leaves and the next compilation
    /// takes the tagged path.
    pub(super) fn prove_element_kinds(&mut self) {
        self.prove_int32_kinds();
        self.prove_double_kinds();
        self.guard_tagged_store_kinds();
    }

    /// An array this region writes whose kind it did not see raw at compilation may have been
    /// promoted since: its encoded stores would leave a word a raw reader takes as a double,
    /// so the entry leaves and the next compilation takes the raw path (which converts its
    /// Numbers). Arrays already on that path store only doubles and need no guard.
    fn guard_tagged_store_kinds(&mut self) {
        for array in self.region.arrays.iter().filter(|array| array.written) {
            if self.double_arrays.contains(&array.register) { continue; }
            let Some(view) = self.arrays.get(&array.register).copied() else { continue };
            let elements = self.array_elements(view);
            let capacity = self.body.builder.ins().load(types::I32, flags(), elements, crate::heap::CAPACITY_FROM_VALUES);
            let doubles = self.body.builder.ins().band_imm_u(capacity, i64::from(crate::heap::DOUBLE_ELEMENTS));
            let set = self.body.builder.ins().icmp_imm_u(IntCC::NotEqual, doubles, 0);
            let widen = self.not_entered(Some(Widening::Elements(array.register)));
            self.pass_unless(set, widen);
        }
    }

    fn prove_double_kinds(&mut self) {
        for register in self.double_arrays.iter().copied().collect::<Vec<_>>() {
            let Some(view) = self.arrays.get(&register).copied() else { continue };
            let elements = self.array_elements(view);
            let capacity = self.body.builder.ins().load(types::I32, flags(), elements, crate::heap::CAPACITY_FROM_VALUES);
            let doubles = self.body.builder.ins().band_imm_u(capacity, i64::from(crate::heap::DOUBLE_ELEMENTS));
            let lost = self.body.builder.ins().icmp_imm_u(IntCC::Equal, doubles, 0);
            let widen = self.not_entered(Some(Widening::Elements(register)));
            self.pass_unless(lost, widen);
        }
    }

    /// The double a raw-double array's element `index` holds: the word as it loads, with no
    /// tag test. Any other array's element takes `unbox_float64`, which leaves on a tag.
    pub(super) fn element_load(&mut self, pc: u32, base: Register, dynamic: bool,
        address: ir::Value, displacement: i32) -> ir::Value {
        let double = self.body.builder.ins().load(types::F64, flags(), address, displacement);
        if !dynamic && self.double_arrays.contains(&base) {
            return double;
        }
        let exit = self.exit_block(pc);
        self.unbox_float64(double, exit)
    }

    /// The bits a store of `value` writes into the array whose first value is `elements`: a
    /// raw-double array takes a Number's double bits and keeps the kind; a tagged word
    /// clears the kind first, since a raw reader must not take it as a double.
    pub(super) fn element_store_bits(&mut self, base: Register, value: Typed, elements: ir::Value) -> ir::Value {
        if !self.double_arrays.contains(&base) {
            return self.encoding(value);
        }
        match value.representation {
            Representation::Float64 => self.body.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), value.value),
            Representation::Int32 => {
                let number = self.float(value);
                self.body.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), number)
            }
            Representation::Tagged => self.number_or_tag_store(value, elements),
        }
    }

    /// The bits a tagged store writes into a raw-double backing: a runtime Number becomes its
    /// double, keeping the kind; any other value clears the kind and stores its tagged word.
    fn number_or_tag_store(&mut self, value: Typed, elements: ir::Value) -> ir::Value {
        let bits = value.value;
        let tag = self.body.builder.ins().band_imm_u(bits, ValueLayout::TAG_MASK as i64);
        let is_int = self.body.builder.ins().icmp_imm_u(IntCC::Equal, tag, ValueLayout::INT_TAG as i64);
        let field = self.body.builder.ins().ushr_imm_u(bits, i64::from(ValueLayout::TAG_SHIFT));
        let offset = self.body.builder.ins().iadd_imm_s(field, -(ValueLayout::FIRST_TAG_FIELD as i64));
        let tagged = self.body.builder.ins().icmp_imm_u(IntCC::UnsignedLessThanOrEqual, offset, ValueLayout::TAG_FIELD_SPAN as i64);
        let is_double = self.body.builder.ins().icmp_imm_u(IntCC::Equal, tagged, 0);
        let is_number = self.body.builder.ins().bor(is_int, is_double);
        let payload = self.body.builder.ins().ireduce(types::I32, bits);
        let widened = self.body.builder.ins().fcvt_from_sint(types::F64, payload);
        let raw = self.body.builder.ins().bitcast(types::F64, ir::MemFlagsData::new(), bits);
        let number = self.body.builder.ins().select(is_int, widened, raw);
        let number_bits = self.body.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), number);
        let (numbers, tags, join) = (self.body.builder.create_block(), self.cold_block(), self.body.builder.create_block());
        let word = self.body.builder.append_block_param(join, types::I64);
        self.body.builder.ins().brif(is_number, numbers, &[], tags, &[]);
        self.body.builder.switch_to_block(numbers);
        self.body.builder.seal_block(numbers);
        self.body.builder.ins().jump(join, &[ir::BlockArg::Value(number_bits)]);
        self.body.builder.switch_to_block(tags);
        self.body.builder.seal_block(tags);
        let capacity = self.body.builder.ins().load(types::I32, flags(), elements, crate::heap::CAPACITY_FROM_VALUES);
        let cleared = self.body.builder.ins().band_imm_u(capacity, !i64::from(crate::heap::DOUBLE_ELEMENTS));
        self.body.builder.ins().store(flags(), cleared, elements, crate::heap::CAPACITY_FROM_VALUES);
        let encoded = self.encoding(value);
        self.body.builder.ins().jump(join, &[ir::BlockArg::Value(encoded)]);
        self.body.builder.switch_to_block(join);
        self.body.builder.seal_block(join);
        word
    }
}

/// The entry sets `new` derives from the region and this activation: its unit registers and
/// its raw-double arrays.
pub(super) fn entry_sets(region: &LoopRegion, webs: &Webs,
    observation: &Observation, written: &std::collections::BTreeSet<Register>)
    -> (std::collections::BTreeSet<Register>, std::collections::BTreeSet<Register>) {
    (unit_registers(region, webs, observation, written), double_arrays(region, observation))
}

/// The region's arrays whose backing holds only raw doubles now: their reads take the double
/// the word loads as, and their stores write doubles.
pub(super) fn double_arrays(region: &LoopRegion, observation: &Observation) -> std::collections::BTreeSet<Register> {
    region.arrays.iter().map(|array| array.register)
        .filter(|register| (observation.double_elements)(*register)).collect()
}

/// The entry registers the region never writes that held exactly 1 when it compiled: each is
/// guarded at entry (`Widening::Unit`), and a product with or a quotient by one is its other
/// operand, exactly, for every double (signed zeros, infinities and NaN alike).
pub(super) fn unit_registers(region: &LoopRegion, webs: &Webs, observation: &Observation,
    written: &std::collections::BTreeSet<Register>) -> std::collections::BTreeSet<Register> {
    let one = Value::number(1.0);
    (0..region.numbers.len())
        .filter(|entry| webs.entry_read[*entry])
        .map(|entry| region.numbers[entry])
        .filter(|register| {
            !written.contains(register)
                && !observation.widened.contains(&Widening::Unit(*register))
                && observation.register.is_some_and(|read| read(*register) == one)
        })
        .collect()
}
