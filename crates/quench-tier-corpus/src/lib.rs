//! Seeded random JavaScript programs for the cross-tier differential and forced-deopt checks.
//!
//! A program is a pure function of its seed and options: it defines a few feature blocks
//! (numeric loops, int32 overflow, doubles, shape changes mid-loop, polymorphic sites, calls,
//! exceptions, holey arrays, strings), drives each from a hot loop, folds every result into one
//! int32 digest and reports the digest through the chosen [`Sink`]. Every execution tier must
//! report the same digest.

mod features;
mod rng;

use features::{FEATURES, Feature};
use rng::Rng;

/// How a program reports its digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sink {
    /// `console.log("digest", h)`: for command-line runs.
    Print,
    /// `globalThis.answer = h`: for embedders that read the global.
    Answer,
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Hot-loop iterations; tiering needs hundreds.
    pub iterations: u32,
    pub sink: Sink,
}

impl Default for Options {
    fn default() -> Self {
        Self { iterations: DEFAULT_ITERATIONS, sink: Sink::Print }
    }
}

pub const DEFAULT_ITERATIONS: u32 = 2000;
const MIN_BLOCKS: u64 = 3;
const MAX_BLOCKS: u64 = 6;

/// Folds `String(v)` into the int32 digest `h`.
const PRELUDE: &str = "let h = 0;\nfunction mix(v) { const t = String(v); for (let c = 0; c < t.length; c++) h = (Math.imul(h, 31) + t.charCodeAt(c)) | 0; }\n";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub seed: u64,
    /// The feature of each block, in order.
    pub features: Vec<&'static str>,
    pub source: String,
}

/// The seed of program `index` in a run seeded with `run_seed`.
pub fn program_seed(run_seed: u64, index: u64) -> u64 {
    rng::mix(run_seed ^ rng::mix(index))
}

fn fill(feature: &Feature, block: usize, rng: &mut Rng) -> String {
    let mut text = feature.template.replace("@K", &block.to_string());
    for param in feature.params {
        text = text.replace(param.token, &rng.range(param.low, param.high).to_string());
    }
    text
}

fn driver(blocks: usize, iterations: u32, in_function: bool) -> String {
    let calls: String = (0..blocks)
        .map(|k| format!("    try {{ mix(f{k}(i)); }} catch (e) {{ mix(e.name); }}\n"))
        .collect();
    let body = format!("for (let i = 0; i < {iterations}; i++) {{\n{calls}}}\n");
    if in_function { format!("function main() {{\n{body}}}\nmain();\n") } else { body }
}

fn sink(sink: Sink) -> &'static str {
    match sink {
        Sink::Print => "console.log('digest', h);\n",
        Sink::Answer => "globalThis.answer = h;\n",
    }
}

/// The program for `seed`.
pub fn generate(seed: u64, options: Options) -> Program {
    let mut rng = Rng::new(seed);
    let blocks = rng.range(MIN_BLOCKS, MAX_BLOCKS) as usize;
    let chosen: Vec<&Feature> = (0..blocks).map(|_| &FEATURES[rng.below(FEATURES.len())]).collect();
    let mut source = String::from(PRELUDE);
    for (block, feature) in chosen.iter().enumerate() {
        source.push_str(&fill(feature, block, &mut rng));
        source.push('\n');
    }
    source.push_str(&driver(blocks, options.iterations, rng.chance()));
    source.push_str(sink(options.sink));
    Program { seed, features: chosen.iter().map(|feature| feature.name).collect(), source }
}

/// Every feature name, for coverage reports.
pub fn feature_names() -> impl Iterator<Item = &'static str> {
    FEATURES.iter().map(|feature| feature.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEEDS: u64 = 200;

    #[test]
    fn a_seed_fixes_the_program() {
        for seed in 0..SEEDS {
            assert_eq!(generate(seed, Options::default()), generate(seed, Options::default()));
        }
    }

    #[test]
    fn different_seeds_give_different_programs() {
        let distinct: std::collections::HashSet<String> =
            (0..SEEDS).map(|seed| generate(seed, Options::default()).source).collect();
        assert!(distinct.len() > SEEDS as usize * 9 / 10);
    }

    #[test]
    fn program_seeds_are_stable_and_spread() {
        assert_eq!(program_seed(7, 3), program_seed(7, 3));
        assert_ne!(program_seed(7, 3), program_seed(7, 4));
        assert_ne!(program_seed(7, 3), program_seed(8, 3));
    }

    #[test]
    fn templates_are_fully_substituted() {
        for seed in 0..SEEDS {
            assert!(!generate(seed, Options::default()).source.contains('@'));
        }
    }

    #[test]
    fn the_corpus_reaches_every_feature() {
        let seen: std::collections::HashSet<&str> =
            (0..SEEDS).flat_map(|seed| generate(seed, Options::default()).features).collect();
        assert_eq!(seen.len(), feature_names().count());
    }

    #[test]
    fn the_sink_and_iterations_are_options() {
        let print = generate(1, Options { iterations: 5, sink: Sink::Print }).source;
        let answer = generate(1, Options { iterations: 5, sink: Sink::Answer }).source;
        assert!(print.contains("console.log") && print.contains("i < 5;"));
        assert!(answer.contains("globalThis.answer = h"));
    }
}

pub mod residuals;
