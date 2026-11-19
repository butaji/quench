//! The JavaScript program an analogue's row runs as: the driver's seeded `Math.random`, the row's
//! files in JetStream's load order, then the driver, which runs the row's `Benchmark` for the
//! analogue's iterations exactly as JetStream's driver does (seed reset before each iteration).

use std::{fs, path::Path};

use super::analogue::Analogue;

/// JetStream's deterministic `Math.random` (the driver's `addDeterministicRandom`).
const SEEDED_RANDOM: &str = "(() => {
  const initialSeed = 49734321;
  let seed = initialSeed;
  Math.random = () => {
    seed = ((seed + 0x7ed55d16) + (seed << 12)) & 0xffff_ffff;
    seed = ((seed ^ 0xc761c23c) ^ (seed >>> 19)) & 0xffff_ffff;
    seed = ((seed + 0x165667b1) + (seed << 5)) & 0xffff_ffff;
    seed = ((seed + 0xd3a2646c) ^ (seed << 9)) & 0xffff_ffff;
    seed = ((seed + 0xfd7046c5) + (seed << 3)) & 0xffff_ffff;
    seed = ((seed ^ 0xb55a4f09) ^ (seed >>> 16)) & 0xffff_ffff;
    return (seed >>> 0) / 0x1_0000_0000;
  };
  Math.random.__resetSeed = () => { seed = initialSeed; };
})();
";

/// The driver: runs the row's `Benchmark` for `iterations`, validates, and names what ran.
fn driver(analogue: &Analogue) -> String {
    let count = analogue.iterations;
    format!(
        "{{
  const benchmark = new Benchmark();
  for (let i = 0; i < {count}; i++) {{
    benchmark.prepareForNextIteration?.();
    Math.random.__resetSeed();
    benchmark.runIteration(i);
  }}
  benchmark.validate?.({count});
  console.log(\"{row} {count} iterations\");
}}
",
        row = analogue.row
    )
}

/// The program's text: the seeded random, every file of the row, then the driver.
pub fn compose(analogue: &Analogue, jetstream: &Path) -> Result<String, String> {
    let mut program = String::from(SEEDED_RANDOM);
    for file in &analogue.files {
        let path = jetstream.join(file);
        let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        program.push_str(&text);
        program.push('\n');
    }
    program.push_str(&driver(analogue));
    Ok(program)
}
