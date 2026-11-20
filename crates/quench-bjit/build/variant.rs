//! One CPU variant's table. The fixed rows come from their shard as before. Each generated row's key is read from its
//! shard's IR (`jobs::ir_keys`); a row whose key has a cache entry (`row_cache`) keeps that entry, and every other row
//! is compiled in a fresh shard, split by the same weights and families as the shards are, then cached under its key.

use super::{Arch, Counts, Family, Instance, ObjectRow, Rows, Stencil, instance_file, shards};
use crate::{jobs, row_cache};
use jobs::Shard;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

/// What every variant shares: the output directory, the shards (`parts`, each with its family, the fixed rows' first),
/// the generated rows in instance order, and each family's sources key.
pub struct Plan<'a> {
    pub out: &'a Path,
    pub parts: &'a [(Family, &'a [Instance])],
    pub instances: &'a [Instance],
    pub keys: &'a [(Family, u128)],
}

impl Plan<'_> {
    fn sources(&self, family: Family) -> u128 {
        self.keys.iter().find(|(of, _)| *of == family).map(|(_, key)| *key).expect("a family key")
    }
}

/// The name a row's macro invocation (`number_row!(name, ...)`) defines.
fn row_name(text: &str) -> &str {
    text.split_once('(').expect("a row macro").1.split(',').next().expect("a row name").trim()
}

/// The table of one CPU variant: its rows sorted by name, and what `checks` counted over them.
pub fn table(plan: &Plan<'_>, cpu: Option<&'static str>, (arch, read): (Arch, fn(&[u8]) -> Vec<ObjectRow>)) -> (Rows, Counts) {
    let name = jobs::cpu_name(cpu);
    let cache = plan.out.join("row_cache").join(name);
    fs::create_dir_all(&cache).unwrap();
    let ir: Vec<Shard> = plan
        .parts
        .iter()
        .enumerate()
        .skip(1)
        .map(|(shard, (family, _))| jobs::ir_job(plan.out, &format!("ir_{name}_{shard}"), *family, cpu, plan.sources(*family), &instance_file(plan.out, shard)))
        .collect();
    let ir_refs: Vec<&Shard> = ir.iter().collect();
    let key_of: HashMap<String, u128> = jobs::pool(&ir_refs, jobs::ir_keys).into_iter().flatten().collect();
    assert_eq!(key_of.len(), plan.instances.len(), "every generated row is defined by exactly one shard");
    let (mut stencils, missing) = split_cached(&cache, plan.instances, &key_of);
    let fixed = jobs::shard_job(plan.out, &format!("rows_{name}_{}", super::FIXED_ROWS_SHARD), Family::Fixed, cpu, plan.sources(Family::Fixed), &instance_file(plan.out, super::FIXED_ROWS_SHARD));
    let fresh = fresh_jobs(plan, name, cpu, &missing);
    let mut compile: Vec<&Shard> = vec![&fixed];
    compile.extend(fresh.iter());
    let compiled = jobs::objects(&compile, arch, read);
    for stencil in &compiled {
        if let Some(&key) = key_of.get(&stencil.name) {
            row_cache::write_entry(&cache, key, stencil.framed, stencil.unchained, &stencil.line);
        }
    }
    row_cache::prune(&cache, &key_of.values().copied().collect::<HashSet<u128>>());
    stencils.extend(compiled);
    let counts = stencils.iter().fold(Counts::default(), |sum, s| Counts { framed: sum.framed + usize::from(s.framed), unchained: sum.unchained + usize::from(s.unchained) });
    let rows = shards::merge([stencils.into_iter().map(|s| (s.name, s.line)).collect::<Vec<(String, String)>>()]);
    (rows, counts)
}

/// The generated rows with a cache entry under their key, and the rest (in instance order).
fn split_cached<'i>(cache: &Path, instances: &'i [Instance], key_of: &HashMap<String, u128>) -> (Vec<Stencil>, Vec<&'i Instance>) {
    let mut cached = Vec::new();
    let mut missing = Vec::new();
    for instance in instances {
        let name = row_name(&instance.0);
        let key = *key_of.get(name).unwrap_or_else(|| panic!("row {name} is defined by no shard"));
        match row_cache::read_entry(cache, key) {
            Some((framed, unchained, line)) => cached.push(Stencil { name: name.to_string(), line, framed, unchained }),
            None => missing.push(instance),
        }
    }
    (cached, missing)
}

/// Fresh shards for the rows without an entry: each family's, split by weight as the shards are, written beside the
/// build as their instance files.
fn fresh_jobs(plan: &Plan<'_>, name: &str, cpu: Option<&'static str>, missing: &[&Instance]) -> Vec<Shard> {
    let mut made = Vec::new();
    for family in [Family::Js, Family::Wasm] {
        let rows: Vec<Instance> = missing.iter().filter(|row| row.2 == family).map(|row| (*row).clone()).collect();
        if rows.is_empty() {
            continue;
        }
        for part in shards::split(&rows, |(_, weight, _)| *weight) {
            let index = made.len();
            let instances = plan.out.join(format!("fresh_{name}_{index}.rs"));
            fs::write(&instances, part.iter().map(|(row, _, _)| row.as_str()).collect::<String>()).unwrap();
            made.push(jobs::shard_job(plan.out, &format!("rows_{name}_fresh{index}"), family, cpu, plan.sources(family), &instances));
        }
    }
    made
}
