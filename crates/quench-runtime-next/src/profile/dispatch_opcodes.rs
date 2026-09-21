#![cfg(feature = "profile-aggregate")]

use super::Profile;
use crate::bytecode::{DispatchClass, Op, ResidualProgram};

pub(super) fn derive(profile: &Profile, program: &ResidualProgram) -> [[u64; Op::COUNT]; 2] {
    let mut counts = [[0; Op::COUNT]; 2];
    for (function_id, function) in program.functions.iter().enumerate() {
        let class = usize::from(function.dispatch == DispatchClass::Numeric);
        let sites = profile
            .site_counts
            .get(function_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for (pc, instruction) in function.code.iter().enumerate() {
            counts[class][instruction.op() as usize] += sites.get(pc).copied().unwrap_or(0);
        }
    }
    counts
}

pub(super) fn report(profile: &Profile, program: &ResidualProgram) {
    let counts = derive(profile, program);
    let physical_total = profile.site_counts.iter().flatten().sum::<u64>();
    assert_eq!(
        counts.iter().flatten().sum::<u64>(),
        physical_total,
        "dispatch/opcode matrix lost physical executions"
    );
    let mut virtual_counts = [0; Op::COUNT];
    for opcode in 0..Op::COUNT {
        virtual_counts[opcode] = profile
            .opcodes
            .get(opcode)
            .copied()
            .unwrap_or(0)
            .checked_sub(counts[0][opcode] + counts[1][opcode])
            .unwrap_or_else(|| panic!("physical count exceeds semantic {}", Op::NAMES[opcode]));
    }
    eprint!(",\"dispatch_opcode_targets\":{{");
    for (class, (name, row)) in [
        ("general", counts[0]),
        ("numeric", counts[1]),
        ("virtual", virtual_counts),
    ]
    .into_iter()
    .enumerate()
    {
        if class > 0 {
            eprint!(",");
        }
        eprint!("\"{name}\":{{");
        for (opcode, count) in row.iter().enumerate() {
            if opcode != 0 {
                eprint!(",");
            }
            eprint!("\"{}\":{}", Op::NAMES[opcode], count);
        }
        eprint!("}}");
    }
    eprint!("}}");
}
