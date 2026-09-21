#![cfg(feature = "profile-aggregate")]

use crate::bytecode::{NUMERIC_LOCAL_TARGET, Op, Operand, REGISTER_MASK, RETURN_REGISTER};

pub(super) fn binary_pairs(
    sites: &rustc_hash::FxHashMap<(u32, u32), u64>,
    program: &crate::bytecode::ResidualProgram,
) -> [u64; 4] {
    let mut counts = [0; 4];
    for (&(function, pc), &count) in sites {
        let Some(code) = program
            .functions
            .get(function as usize)
            .map(|function| function.code.as_slice())
        else {
            continue;
        };
        let Some([first, second]) = code.get(pc as usize..pc as usize + 2) else {
            continue;
        };
        if first.op() != Op::Binary || second.op() != Op::Binary {
            continue;
        }
        let Some(output) = binary_output(*first) else {
            continue;
        };
        let left = second.b() == output.0;
        let right = second.c() == output.0;
        counts[usize::from(left) + 2 * usize::from(right)] += count;
    }
    counts
}

fn binary_output(instruction: crate::bytecode::Instr) -> Option<Operand> {
    if instruction.a() & RETURN_REGISTER != 0 {
        None
    } else if instruction.a() & NUMERIC_LOCAL_TARGET != 0 {
        Some(Operand::local(instruction.a() & REGISTER_MASK))
    } else {
        Some(Operand::register(instruction.a()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_preserves_register_and_local_address_space() {
        let register = crate::bytecode::Instr::new(Op::Binary, 7, 0, 0, 8);
        let local = crate::bytecode::Instr::new(Op::Binary, NUMERIC_LOCAL_TARGET | 7, 0, 0, 8);
        assert_eq!(binary_output(register).unwrap().tag(), 0);
        assert_eq!(binary_output(local).unwrap().tag(), 3);
    }
}
