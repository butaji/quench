use super::{Profile, dispatch_opcodes};
use crate::bytecode::{DispatchClass, Op, ResidualProgram};

const PAIRS: usize = Op::COUNT * Op::COUNT;

struct Matrix {
    pairs: [[u64; PAIRS]; 2],
    boundaries: [[u64; Op::COUNT]; 2],
}

fn derive(profile: &Profile, program: &ResidualProgram) -> Matrix {
    let targets = dispatch_opcodes::derive(profile, program);
    let mut pairs = [[0; PAIRS]; 2];
    let mut outgoing = [[0; Op::COUNT]; 2];
    for (&(function_id, pc), &count) in &profile.pair_sites {
        let function = &program.functions[function_id as usize];
        let class = usize::from(function.dispatch == DispatchClass::Numeric);
        let first = function.code[pc as usize];
        let second = function.code[pc as usize + 1];
        let first = if first.is_wide() {
            function.wide[first.wide_index()].op()
        } else {
            first.op()
        } as usize;
        let second = if second.is_wide() {
            function.wide[second.wide_index()].op()
        } else {
            second.op()
        } as usize;
        pairs[class][first * Op::COUNT + second] += count;
        outgoing[class][first] += count;
    }
    for (pair, (&general, &numeric)) in pairs[0].iter().zip(pairs[1].iter()).enumerate() {
        assert_eq!(
            general + numeric,
            profile.pairs.get(pair).copied().unwrap_or(0),
            "dispatch/pair matrix lost physical adjacent executions"
        );
    }
    let mut boundaries = [[0; Op::COUNT]; 2];
    for class in 0..2 {
        for opcode in 0..Op::COUNT {
            boundaries[class][opcode] = targets[class][opcode]
                .checked_sub(outgoing[class][opcode])
                .unwrap_or_else(|| panic!("pair count exceeds {} targets", Op::NAMES[opcode]));
        }
        assert_eq!(
            pairs[class].iter().sum::<u64>() + boundaries[class].iter().sum::<u64>(),
            targets[class].iter().sum::<u64>(),
            "dispatch/pair matrix lost physical targets"
        );
    }
    Matrix { pairs, boundaries }
}

pub(super) fn report(profile: &Profile, program: &ResidualProgram) {
    let matrix = derive(profile, program);
    eprint!(",\"dispatch_pair_targets\":{{");
    for (class, name) in ["general", "numeric"].into_iter().enumerate() {
        if class != 0 {
            eprint!(",");
        }
        let mut rows: Vec<_> = matrix.pairs[class]
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, count)| *count != 0)
            .collect();
        rows.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        eprint!("\"{name}\":{{\"pairs\":[");
        for (position, (pair, count)) in rows.into_iter().enumerate() {
            if position != 0 {
                eprint!(",");
            }
            eprint!(
                "[\"{}\",\"{}\",{}]",
                Op::NAMES[pair / Op::COUNT],
                Op::NAMES[pair % Op::COUNT],
                count
            );
        }
        eprint!("],\"boundaries\":{{");
        for (opcode, count) in matrix.boundaries[class].iter().enumerate() {
            if opcode != 0 {
                eprint!(",");
            }
            eprint!("\"{}\":{}", Op::NAMES[opcode], count);
        }
        eprint!("}}}}");
    }
    eprint!("}}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{AtomTable, Function, Instr};
    use rustc_hash::FxHashMap;

    fn function(dispatch: DispatchClass, ops: &[Op]) -> Function {
        Function {
            parent: None,
            name: None,
            params: 0,
            rest: false,
            locals: 0,
            code: ops.iter().map(|op| Instr::new(*op, 0, 0, 0, 0)).collect(),
            wide: Vec::new(),
            registers: 1,
            dispatch,
            handlers: Vec::new(),
            register_root_offset: 0,
        }
    }

    #[test]
    fn partitions_pairs_and_nonsequential_boundaries_by_dispatch_class() {
        let program = ResidualProgram {
            specialized: true,
            atoms: AtomTable::default(),
            constants: Vec::new(),
            functions: vec![
                function(
                    DispatchClass::General,
                    &[Op::LoadLocal, Op::StoreLocal, Op::Return],
                ),
                function(
                    DispatchClass::Numeric,
                    &[Op::LoadLocal, Op::LoadLocal, Op::Return],
                ),
            ],
            cache_sites: 0,
            method_sites: Vec::new(),
            method_arguments: Vec::new(),
            field_sites: Vec::new(),
            object_sites: Vec::new(),
            superinstructions: Vec::new(),
            register_roots: Vec::new(),
        };
        let mut pairs = vec![0; PAIRS];
        pairs[Op::LoadLocal as usize * Op::COUNT + Op::StoreLocal as usize] = 4;
        pairs[Op::StoreLocal as usize * Op::COUNT + Op::Return as usize] = 4;
        pairs[Op::LoadLocal as usize * Op::COUNT + Op::LoadLocal as usize] = 6;
        pairs[Op::LoadLocal as usize * Op::COUNT + Op::Return as usize] = 6;
        let profile = Profile {
            pairs,
            pair_sites: FxHashMap::from_iter([((0, 0), 4), ((0, 1), 4), ((1, 0), 6), ((1, 1), 6)]),
            site_counts: vec![vec![5, 4, 4], vec![7, 6, 6]],
            ..Profile::default()
        };

        let matrix = derive(&profile, &program);
        assert_eq!(matrix.boundaries[0][Op::LoadLocal as usize], 1);
        assert_eq!(matrix.boundaries[0][Op::Return as usize], 4);
        assert_eq!(matrix.boundaries[1][Op::LoadLocal as usize], 1);
        assert_eq!(matrix.boundaries[1][Op::Return as usize], 6);
    }
}
