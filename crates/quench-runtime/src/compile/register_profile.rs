use crate::bytecode::{FieldSite, Function, Op, Register, Superinstruction};

type MethodSite = (u32, u16, Vec<Register>, Option<(u32, u16)>);
type Excluded = (usize, usize, usize, usize);

#[derive(Default)]
struct Census {
    buckets: [usize; 5],
    total_registers: usize,
    total_max_live: usize,
    analyzed: usize,
    moves: usize,
    max_registers: u16,
    excluded: Vec<Excluded>,
}

pub(super) fn report(
    functions: &[Function],
    methods: &[MethodSite],
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) {
    let census = collect(functions, methods, fields, superinstructions);
    emit(&census, functions.len());
}

fn collect(
    functions: &[Function],
    methods: &[MethodSite],
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> Census {
    let mut census = Census::default();
    for (id, function) in functions.iter().enumerate() {
        let registers = function.registers as usize;
        let moves = function
            .code
            .iter()
            .filter(|ins| ins.op() == Op::Move)
            .count();
        census.buckets[bucket(registers)] += 1;
        census.total_registers += registers;
        census.moves += moves;
        census.max_registers = census.max_registers.max(function.registers);
        match super::liveness::analyze(function, methods, fields, superinstructions) {
            Some(live) => {
                census.analyzed += 1;
                census.total_max_live +=
                    live.iter().map(|mask| mask.count_ones()).max().unwrap_or(0) as usize;
            }
            None => census
                .excluded
                .push((id, registers, function.code.len(), moves)),
        }
    }
    census
}

fn emit(census: &Census, functions: usize) {
    let bytes = census.total_registers * size_of::<crate::value::Value>();
    eprint!(
        "{{\"kind\":\"rqj-register-census\",\"bucket_max\":[8,16,32,64,null],\"function_buckets\":{:?},\"functions\":{functions},\"analyzed_functions\":{},\"excluded_functions\":{},\"total_registers\":{},\"total_register_bytes\":{bytes},\"max_registers\":{},\"sum_max_live\":{},\"move_sites\":{},\"excluded\":[",
        census.buckets,
        census.analyzed,
        census.excluded.len(),
        census.total_registers,
        census.max_registers,
        census.total_max_live,
        census.moves,
    );
    for (index, &(id, registers, instructions, moves)) in census.excluded.iter().enumerate() {
        if index != 0 {
            eprint!(",");
        }
        eprint!(
            "{{\"function\":{id},\"registers\":{registers},\"instructions\":{instructions},\"moves\":{moves}}}"
        );
    }
    eprintln!("]}}");
}

fn bucket(registers: usize) -> usize {
    match registers {
        0..=8 => 0,
        9..=16 => 1,
        17..=32 => 2,
        33..=64 => 3,
        _ => 4,
    }
}
