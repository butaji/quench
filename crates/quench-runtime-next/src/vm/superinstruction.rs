use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn execute_const_array_object2(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
        destination: Register,
        site: u32,
    ) -> Result<Option<Value>, JsError> {
        let [array, first, second, object] = program.superinstructions[site as usize].code;
        debug_assert_eq!(array.op(), Op::MakeConstArray);
        debug_assert_eq!(first.op(), Op::Binary);
        debug_assert_eq!(second.op(), Op::Binary);
        debug_assert_eq!(object.op(), Op::MakeObject2);

        let start = array.imm() as usize;
        let end = start + array.b() as usize;
        let elements = self.const_arrays[start]
            .get_or_insert_with(|| Rc::new(self.constants[start..end].to_vec()))
            .clone();
        let array_value = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements,
        });
        self.write(frame, array.a(), array_value);

        self.execute_super_binary(program, frame, first)?;
        self.execute_super_binary(program, frame, second)?;
        let value = self.object_pair(
            program,
            object.imm() as usize,
            self.read(frame, object.b()),
            self.read(frame, object.c()),
        );
        if destination & RETURN_REGISTER != 0 {
            Ok(Some(value))
        } else {
            self.write(frame, destination & REGISTER_MASK, value);
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
        self.profile
            .binary(instruction.imm() as usize, instruction.b(), instruction.c());
        let left = self.resolve_operand(program, frame, Operand(instruction.b()))?;
        let right = self.resolve_operand(program, frame, Operand(instruction.c()))?;
        let value = self.binary(program, instruction.imm(), left, right)?;
        self.write(frame, instruction.a(), value);
        Ok(())
    }
}
