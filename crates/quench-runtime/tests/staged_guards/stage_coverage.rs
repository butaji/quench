//! Every tier is a stage of the one staged definition: the interpreter runs it over `Concrete`
//! (`ConcreteWasm`), b-jit compiles it with rustc over `Concrete` on const-generic operand places
//! plus `StencilStage` for its holes, u-jit emits it over `Emit` (`Emitter`). A stage trait a tier
//! lacks is semantics that tier cannot derive, which it then writes again by hand or leaves to the
//! interpreter. `stage_debt.txt` lists the (trait, tier) pairs missing today; it may only shrink,
//! and a new tier or trait starts complete.

use std::{collections::BTreeSet, fs, path::Path};

/// Each stage trait and the stages that must implement it. `FlowStage` (blocks and transfers) is a
/// specializer's alone: the interpreter runs the host's control flow, and b-jit's stencils chain by
/// `become`.
const REQUIRED: [(&str, &[&str]); 4] = [
    ("Stage", &["Concrete", "Emit", "StencilStage"]),
    ("NumericStage", &["Concrete", "Emit"]),
    ("FlowStage", &["Emit"]),
    ("MachineStage", &["ConcreteWasm", "Emitter", "StencilStage"]),
];
const DEBT: &str = include_str!("../../src/staged/stage_debt.txt");
/// The source trees tiers are written in, from this crate's manifest.
const TREES: [&str; 3] = ["src", "../quench-runtime-engine/src", "../quench-bjit/stencils"];

fn sources(directory: &Path, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() { sources(&path, found) } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(fs::read_to_string(&path).unwrap_or_default());
        }
    }
}

/// The (trait, tier) pairs some `impl Trait for Tier` writes, paths and generics dropped.
fn implemented(texts: &[String]) -> BTreeSet<(String, String)> {
    let mut pairs = BTreeSet::new();
    for line in texts.iter().flat_map(|text| text.lines()).map(str::trim) {
        let Some(rest) = line.strip_prefix("impl") else { continue };
        let rest = if rest.starts_with('<') { rest.split_once("> ").map_or(rest, |(_, after)| after) } else { rest.trim_start() };
        let Some((trait_path, tier)) = rest.split_once(" for ") else { continue };
        let name = |path: &str| path.split('<').next().unwrap_or(path).rsplit("::").next().unwrap_or(path).trim().to_owned();
        let tier = tier.split(['{', ' ']).next().unwrap_or(tier);
        pairs.insert((name(trait_path), name(tier)));
    }
    pairs
}

#[test]
fn every_tier_implements_the_stage_traits_it_derives_from() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut texts = Vec::new();
    for tree in TREES { sources(&root.join(tree), &mut texts) }
    let have = implemented(&texts);
    let debt: BTreeSet<(String, String)> = DEBT.lines().filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once(' ')).map(|(stage_trait, tier)| (stage_trait.to_owned(), tier.trim().to_owned())).collect();
    let mut missing = Vec::new();
    let mut paid = Vec::new();
    for (stage_trait, tiers) in REQUIRED {
        {
            for tier in tiers {
                let pair = ((*stage_trait).to_owned(), (*tier).to_owned());
                match (have.contains(&pair), debt.contains(&pair)) {
                    (false, false) => missing.push(format!("{stage_trait} {tier}")),
                    (true, true) => paid.push(format!("{stage_trait} {tier}")),
                    _ => {}
                }
            }
        }
    }
    assert!(missing.is_empty(), "tiers missing a stage trait (implement it; the debt may not grow): {missing:?}");
    assert!(paid.is_empty(), "implemented now, remove from staged/stage_debt.txt: {paid:?}");
}
