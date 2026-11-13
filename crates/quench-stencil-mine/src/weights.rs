//! Dynamic weights: executions per static instruction from the dispatch census (or the
//! opcode trace's unigram counts), exact n-gram counts from the trace when given.
use crate::model::{MAX_NGRAM, MIN_NGRAM};
use quench_runtime::FlowFunction;
use std::collections::HashMap;

/// Bytes of one opcode in the `profile-trace` file (`profile.rs`: little-endian `u32`).
const TRACE_OPCODE_BYTES: usize = 4;
const OPCODE_BITS: u32 = u16::BITS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// No dynamic input: every static instruction counts once.
    Static,
    Census,
    Trace,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Census => "census",
            Self::Trace => "trace",
        }
    }
}

/// Values by opcode in one flat vector indexed by the opcode; an opcode never set holds the
/// default.
#[derive(Clone, Default)]
pub struct ByOpcode<T>(Vec<T>);

impl<T: Copy + Default> ByOpcode<T> {
    pub fn get(&self, opcode: u16) -> T {
        self.0.get(usize::from(opcode)).copied().unwrap_or_default()
    }

    fn slot(&mut self, opcode: u16) -> &mut T {
        let index = usize::from(opcode);
        if index >= self.0.len() {
            self.0.resize(index + 1, T::default());
        }
        &mut self.0[index]
    }

    fn entries(&self) -> impl Iterator<Item = (u16, T)> + '_ {
        self.0.iter().enumerate().map(|(opcode, &value)| (opcode as u16, value))
    }
}

/// Exact adjacent-opcode counts from a trace, by n (index `n - MIN_NGRAM`): each n's packed
/// windows as `(window, count)` runs sorted by window.
#[derive(Default)]
pub struct TraceGrams {
    pub length: u64,
    unigrams: ByOpcode<u64>,
    grams: Vec<Vec<(u64, u64)>>,
}

fn pack(opcodes: &[u16]) -> u64 {
    opcodes.iter().fold(0, |packed, &opcode| (packed << OPCODE_BITS) | u64::from(opcode))
}

/// `(key, count)` runs of the sorted keys.
fn count_runs(mut keys: Vec<u64>) -> Vec<(u64, u64)> {
    keys.sort_unstable();
    let mut runs: Vec<(u64, u64)> = Vec::new();
    for key in keys {
        match runs.last_mut() {
            Some((last, count)) if *last == key => *count += 1,
            _ => runs.push((key, 1)),
        }
    }
    runs
}

impl TraceGrams {
    pub fn from_opcodes(opcodes: &[u16]) -> Self {
        let mut trace = Self { length: opcodes.len() as u64, ..Self::default() };
        for &opcode in opcodes {
            *trace.unigrams.slot(opcode) += 1;
        }
        trace.grams = (MIN_NGRAM..=MAX_NGRAM).map(|n| count_runs(opcodes.windows(n).map(pack).collect())).collect();
        trace
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let opcodes: Vec<u16> = bytes
            .chunks_exact(TRACE_OPCODE_BYTES)
            .filter_map(|chunk| u16::try_from(u32::from_le_bytes(chunk.try_into().ok()?)).ok())
            .collect();
        Self::from_opcodes(&opcodes)
    }

    pub fn gram(&self, opcodes: &[u16]) -> u64 {
        let key = pack(opcodes);
        let runs = self.grams.get(opcodes.len().wrapping_sub(MIN_NGRAM));
        runs.and_then(|runs| runs.binary_search_by_key(&key, |&(window, _)| window).ok().map(|run| runs[run].1)).unwrap_or(0)
    }
}

pub struct Weights {
    pub source: Source,
    /// Total executed instructions the shares are taken of.
    pub total: f64,
    /// Executions per static instruction of each opcode.
    per_instruction: ByOpcode<f64>,
    /// Dynamic executions of each opcode.
    pub dynamic: ByOpcode<f64>,
    pub trace: Option<TraceGrams>,
}

fn static_counts(corpus: &[FlowFunction]) -> ByOpcode<u64> {
    let mut counts = ByOpcode::default();
    for instruction in corpus.iter().flat_map(|function| &function.instructions) {
        *counts.slot(instruction.opcode) += 1;
    }
    counts
}

fn opcode_ids(corpus: &[FlowFunction]) -> HashMap<&'static str, u16> {
    corpus.iter().flat_map(|function| &function.instructions).map(|i| (i.name, i.opcode)).collect()
}

fn as_dynamic(counts: &ByOpcode<u64>) -> ByOpcode<f64> {
    ByOpcode(counts.0.iter().map(|&n| n as f64).collect())
}

impl Weights {
    pub fn build(corpus: &[FlowFunction], census: Option<HashMap<String, u64>>, trace: Option<TraceGrams>) -> Self {
        let statics = static_counts(corpus);
        let (source, dynamic, total) = if let Some(trace) = &trace {
            (Source::Trace, as_dynamic(&trace.unigrams), trace.length as f64)
        } else if let Some(census) = census {
            let ids = opcode_ids(corpus);
            let mut dynamic = ByOpcode::default();
            for (name, &n) in &census {
                if let Some(&opcode) = ids.get(name.as_str()) {
                    *dynamic.slot(opcode) = n as f64;
                }
            }
            (Source::Census, dynamic, census.values().sum::<u64>() as f64)
        } else {
            (Source::Static, as_dynamic(&statics), statics.0.iter().sum::<u64>() as f64)
        };
        let mut per_instruction = ByOpcode::default();
        for (opcode, n) in statics.entries().filter(|&(_, n)| n > 0) {
            *per_instruction.slot(opcode) = dynamic.get(opcode) / n as f64;
        }
        Self { source, total, per_instruction, dynamic, trace }
    }

    /// Estimated executions of one static instruction.
    pub fn instruction(&self, opcode: u16) -> f64 {
        self.per_instruction.get(opcode)
    }
}

/// Sums the `counts` of every census line in `text` (the runtime prints one JSON object per
/// process to stderr, possibly among other output).
pub fn parse_census(text: &str) -> HashMap<String, u64> {
    let mut totals: HashMap<String, u64> = HashMap::new();
    for line in text.lines().filter(|line| line.contains(CENSUS_KIND)) {
        let Some(json) = line.find('{').and_then(|start| serde_json::from_str::<serde_json::Value>(&line[start..]).ok()) else {
            continue;
        };
        for (name, count) in json["counts"].as_object().into_iter().flatten() {
            *totals.entry(name.clone()).or_insert(0) += count.as_u64().unwrap_or(0);
        }
    }
    totals
}

pub const CENSUS_KIND: &str = "quench-dispatch-opcode-census-all";
