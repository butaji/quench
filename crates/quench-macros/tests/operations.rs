use quench_macros::operation_semantics;

#[derive(Clone, Copy)]
enum Op {
    Add,
    Alias,
    Jump,
    Throw,
    Return,
}

#[derive(Clone, Copy)]
struct Instruction {
    op: Op,
    value: i32,
}

impl Instruction {
    fn op(self) -> Op {
        self.op
    }
}

#[derive(Default)]
struct Machine {
    value: i32,
    effects: usize,
}

impl Machine {
    #[operation_semantics(resolve)]
    fn step(&mut self, instruction: Instruction, pc: &mut usize) -> Result<i32, i32> {
        match instruction.op() {
            Op::Add | Op::Alias => {
                self.value += instruction.value;
                self.effects += 1;
            }
            Op::Jump => *pc = instruction.value as usize,
            Op::Throw => {
                self.effects += 1;
                return Err(instruction.value);
            }
            Op::Return => return Ok(instruction.value),
        }
        Ok(self.value)
    }
}

#[test]
fn resolved_operations_preserve_results_effects_and_control_flow() {
    let mut interpreted = Machine::default();
    let mut residual = Machine::default();
    let mut interpreted_pc = 0;
    let mut residual_pc = 0;
    for op in [Op::Add, Op::Alias, Op::Jump, Op::Throw, Op::Return] {
        let instruction = Instruction { op, value: 7 };
        let expected = interpreted.step(instruction, &mut interpreted_pc);
        let actual = Machine::resolve(op)(&mut residual, instruction, &mut residual_pc);
        assert_eq!(actual, expected);
        assert_eq!(residual_pc, interpreted_pc);
        assert_eq!(residual.value, interpreted.value);
        assert_eq!(residual.effects, interpreted.effects);
    }
}
