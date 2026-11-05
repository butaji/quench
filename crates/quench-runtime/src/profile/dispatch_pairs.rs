use super::{Profile, dispatch_opcodes};
use crate::bytecode::{DispatchClass, Op, ResidualProgram};

const PAIRS: usize = Op::COUNT * Op::COUNT;

struct Matrix {
    pairs: [[u64; PAIRS]; 2],
    boundaries: [[u64; Op::COUNT]; 2],
}

fn derive(profile: &Profile, program_id: u32, program: &ResidualProgram) -> Matrix {
    let targets = dispatch_opcodes::derive(profile, program_id, program);
    let mut pairs = [[0; PAIRS]; 2];
    let mut outgoing = [[0; Op::COUNT]; 2];
    for (&(site_program, function_id, pc), &count) in &profile.pair_sites {
        if site_program != program_id {
            continue;
        }
        let function = &program.function(function_id as usize);
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
    let program_pairs = profile
        .pairs
        .get(program_id as usize)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    for (pair, (&general, &numeric)) in pairs[0].iter().zip(pairs[1].iter()).enumerate() {
        assert_eq!(
            general + numeric,
            program_pairs.get(pair).copied().unwrap_or(0),
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

pub(super) fn report(profile: &Profile, program_id: u32, program: &ResidualProgram) {
    let matrix = derive(profile, program_id, program);
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
    use crate::bytecode::Instr;
    use rustc_hash::FxHashMap;

    #[test]
    fn partitions_pairs_and_nonsequential_boundaries_by_dispatch_class() {
        let mut program =
            crate::Engine::specialize("function second() {}", "dispatch-pair-census.js").unwrap();
        let cases = [
            (
                DispatchClass::General,
                [Op::LoadLocal, Op::StoreLocal, Op::Return],
            ),
            (
                DispatchClass::Numeric,
                [Op::LoadLocal, Op::LoadLocal, Op::Return],
            ),
        ];
        assert_eq!(program.functions.len(), cases.len());
        for (index, (dispatch, ops)) in cases.into_iter().enumerate() {
            // Resolve a lazy body before replacing this census-only instruction fixture.
            program.function(index);
            let function = &mut program.functions[index];
            function.dispatch = dispatch;
            function.code = ops.into_iter().map(|op| Instr::new(op, 0, 0, 0, 0)).collect();
            function.wide.clear();
        }
        let mut pairs = vec![0; PAIRS];
        pairs[Op::LoadLocal as usize * Op::COUNT + Op::StoreLocal as usize] = 4;
        pairs[Op::StoreLocal as usize * Op::COUNT + Op::Return as usize] = 4;
        pairs[Op::LoadLocal as usize * Op::COUNT + Op::LoadLocal as usize] = 6;
        pairs[Op::LoadLocal as usize * Op::COUNT + Op::Return as usize] = 6;
        let mut other_program_pairs = vec![0; PAIRS];
        other_program_pairs[Op::LoadLocal as usize * Op::COUNT + Op::StoreLocal as usize] = 40;
        let profile = Profile {
            pair_sites: FxHashMap::from_iter([
                ((0, 0, 0), 4),
                ((0, 0, 1), 4),
                ((0, 1, 0), 6),
                ((0, 1, 1), 6),
                ((1, 0, 0), 40),
            ]),
            site_counts: vec![vec![vec![5, 4, 4], vec![7, 6, 6]], vec![vec![45, 40, 0]]],
            opcodes: vec![vec![]],
            dispatched_opcodes: vec![vec![]],
            pairs: vec![pairs.clone(), other_program_pairs],
            ..Profile::default()
        };

        let matrix = derive(&profile, 0, &program);
        assert_eq!(matrix.boundaries[0][Op::LoadLocal as usize], 1);
        assert_eq!(matrix.boundaries[0][Op::Return as usize], 4);
        assert_eq!(matrix.boundaries[1][Op::LoadLocal as usize], 1);
        assert_eq!(matrix.boundaries[1][Op::Return as usize], 6);
    }
}
