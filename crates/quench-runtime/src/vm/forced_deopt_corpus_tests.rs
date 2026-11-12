//! Forced deopt (R26) over the generated tier corpus (`quench-tier-corpus`): every guard of every
//! generated program, forced to leave once, must end with the interpreter's digest. The forced-miss
//! hook is test-only, so this is the corpus's forced-deopt driver:
//! `cargo test -p quench-runtime --lib forced_deopt_corpus`.

use super::forced_deopt_tests::forced_everywhere;
use quench_tier_corpus::{generate, program_seed, Options, Sink};

const RUN_SEED: u64 = 0x5EED;
const PROGRAMS: u64 = 12;
/// Enough iterations to compile, few enough that each forced run stays quick.
const ITERATIONS: u32 = 600;
const GUARDS_PER_PROGRAM: usize = 120;

#[test]
fn forced_deopt_corpus_matches_the_interpreter() {
    let options = Options { iterations: ITERATIONS, sink: Sink::Answer };
    let mut guards_total = 0;
    for index in 0..PROGRAMS {
        let program = generate(program_seed(RUN_SEED, index), options);
        let name = format!("tier-corpus-{:016x}.js", program.seed);
        let (guards, mismatches) = forced_everywhere(&program.source, &name, GUARDS_PER_PROGRAM);
        eprintln!("forced deopt {name} [{}]: {guards} guards, mismatches {mismatches:?}", program.features.join(","));
        assert!(mismatches.is_empty(), "{name}: forced guards {mismatches:?} changed the answer\n{}", program.source);
        guards_total += guards;
    }
    assert!(guards_total > 0, "the corpus compiled no guard");
}
