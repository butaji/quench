use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn execute_const_array_object2(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
        instruction: WideInstruction,
    ) -> Result<Option<Value>, JsError> {
        let destination = instruction.result_register();
        let site = instruction.superinstruction_index();
        let [array, first, second, object] = program.superinstructions[site].code;
        debug_assert_eq!(array.op(), Op::MakeConstArray);
        debug_assert_eq!(first.op(), Op::Binary);
        debug_assert_eq!(second.op(), Op::Binary);
        debug_assert_eq!(object.op(), Op::MakeObject2);

        let start = array.constant_index();
        let end = start + array.element_count() as usize;
        let elements = self
            .programs
            .const_array(self.frames[frame].program, start, end - start)
            .ok_or_else(|| JsError::validation("constant array is outside program".into()))?;
        let array_value = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements,
        });
        self.write(frame, array.result_register(), array_value);

        self.execute_super_binary(program, frame, first)?;
        self.execute_super_binary(program, frame, second)?;
        let value = self.object_pair(
            program,
            object.object_site_index(),
            self.read(frame, object.register_b()),
            self.read(frame, object.register_c()),
        );
        if instruction.returns_from_frame() {
            Ok(Some(value))
        } else {
            self.write(frame, destination, value);
            Ok(None)
        }
    }

    #[inline(always)]
    fn execute_super_binary(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
        instruction: Instr,
    ) -> Result<(), JsError> {
        let operator = instruction.binary_operator();
        let left_operand = instruction.operand_b();
        let right_operand = instruction.operand_c();
        self.profile
            .binary(operator as usize, left_operand.0, right_operand.0);
        let left = self.resolve_operand(program, frame, left_operand)?;
        let right = self.resolve_operand(program, frame, right_operand)?;
        let value = self.binary(program, operator, left, right)?;
        self.write(frame, instruction.result_register(), value);
        Ok(())
    }
}
