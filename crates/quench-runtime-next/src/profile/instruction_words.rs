use crate::bytecode::{Instr, Op, ResidualProgram};

#[derive(Clone, Copy, Default)]
struct Domain {
    count: u64,
    max_a: u16,
    max_b: u16,
    max_c: u16,
    max_imm: u32,
    field_overflow: u64,
    immediate_overflow: u64,
}

fn field_fits(op: Op, position: usize, value: u16) -> bool {
    Instr::field_fits(op, position, value)
}

fn immediate_fits(op: Op, value: u32) -> bool {
    Instr::immediate_fits(op, value)
}

pub(crate) fn fits(program: &ResidualProgram) -> bool {
    program.functions.iter().all(|function| {
        function.code.iter().all(|instruction| {
            [instruction.a(), instruction.b(), instruction.c()]
                .into_iter()
                .enumerate()
                .all(|(position, value)| field_fits(instruction.op(), position, value))
                && immediate_fits(instruction.op(), instruction.imm())
        })
    })
}

pub(super) fn report(program: &ResidualProgram) {
    let mut domains = vec![Domain::default(); Op::COUNT];
    for instruction in program.functions.iter().flat_map(|function| &function.code) {
        let domain = &mut domains[instruction.op() as usize];
        domain.count += 1;
        domain.max_a = domain.max_a.max(instruction.a());
        domain.max_b = domain.max_b.max(instruction.b());
        domain.max_c = domain.max_c.max(instruction.c());
        domain.max_imm = domain.max_imm.max(instruction.imm());
        domain.field_overflow += u64::from(
            !field_fits(instruction.op(), 0, instruction.a())
                || !field_fits(instruction.op(), 1, instruction.b())
                || !field_fits(instruction.op(), 2, instruction.c()),
        );
        domain.immediate_overflow +=
            u64::from(!immediate_fits(instruction.op(), instruction.imm()));
    }
    eprint!(",\"instruction_word_domains\":[");
    let mut emitted = false;
    for (opcode, domain) in domains
        .into_iter()
        .enumerate()
        .filter(|(_, domain)| domain.count != 0)
    {
        if emitted {
            eprint!(",");
        }
        emitted = true;
        eprint!(
            "{{\"op\":\"{}\",\"count\":{},\"max_fields\":[{},{},{}],\"max_imm\":{},\"compound_imm\":{},\"field_overflow\":{},\"immediate_overflow\":{}}}",
            Op::NAMES[opcode],
            domain.count,
            domain.max_a,
            domain.max_b,
            domain.max_c,
            domain.max_imm,
            Op::from_index(opcode).is_some_and(|op| op.immediate_layout().uses_packed_pair()),
            domain.field_overflow,
            domain.immediate_overflow
        );
    }
    eprint!("]");
}
