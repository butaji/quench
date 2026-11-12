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

/// Exact adjacent-opcode counts from a trace, by n (index `n - MIN_NGRAM`).
#[derive(Default)]
pub struct TraceGrams {
    pub length: u64,
    pub unigrams: HashMap<u16, u64>,
    grams: Vec<HashMap<u64, u64>>,
}

fn pack(opcodes: &[u16]) -> u64 {
    opcodes.iter().fold(0, |packed, &opcode| (packed << OPCODE_BITS) | u64::from(opcode))
}

impl TraceGrams {
    pub fn from_opcodes(opcodes: &[u16]) -> Self {
        let mut trace = Self { length: opcodes.len() as u64, grams: vec![HashMap::new(); MAX_NGRAM - MIN_NGRAM + 1], ..Self::default() };
        for &opcode in opcodes {
            *trace.unigrams.entry(opcode).or_insert(0) += 1;
        }
        for n in MIN_NGRAM..=MAX_NGRAM {
            for window in opcodes.windows(n) {
                *trace.grams[n - MIN_NGRAM].entry(pack(window)).or_insert(0) += 1;
            }
        }
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
        self.grams
            .get(opcodes.len().wrapping_sub(MIN_NGRAM))
            .and_then(|grams| grams.get(&pack(opcodes)))
            .copied()
            .unwrap_or(0)
    }
}

pub struct Weights {
    pub source: Source,
    /// Total executed instructions the shares are taken of.
    pub total: f64,
    /// Executions per static instruction of each opcode.
    per_instruction: HashMap<u16, f64>,
    /// Dynamic executions of each opcode.
    pub dynamic: HashMap<u16, f64>,
    pub trace: Option<TraceGrams>,
}

fn static_counts(corpus: &[FlowFunction]) -> HashMap<u16, u64> {
    let mut counts = HashMap::new();
    for instruction in corpus.iter().flat_map(|function| &function.instructions) {
        *counts.entry(instruction.opcode).or_insert(0) += 1;
    }
    counts
}

fn opcode_ids(corpus: &[FlowFunction]) -> HashMap<&'static str, u16> {
    corpus.iter().flat_map(|function| &function.instructions).map(|i| (i.name, i.opcode)).collect()
}

impl Weights {
    pub fn build(corpus: &[FlowFunction], census: Option<HashMap<String, u64>>, trace: Option<TraceGrams>) -> Self {
        let statics = static_counts(corpus);
        let (source, dynamic, total) = if let Some(trace) = &trace {
            let dynamic = trace.unigrams.iter().map(|(&op, &n)| (op, n as f64)).collect();
            (Source::Trace, dynamic, trace.length as f64)
        } else if let Some(census) = census {
            let ids = opcode_ids(corpus);
            let dynamic: HashMap<u16, f64> =
                census.iter().filter_map(|(name, &n)| Some((*ids.get(name.as_str())?, n as f64))).collect();
            (Source::Census, dynamic, census.values().sum::<u64>() as f64)
        } else {
            (Source::Static, statics.iter().map(|(&op, &n)| (op, n as f64)).collect(), statics.values().sum::<u64>() as f64)
        };
        let per_instruction = statics
            .iter()
            .map(|(&op, &n)| (op, dynamic.get(&op).copied().unwrap_or(0.0) / n as f64))
            .collect();
        Self { source, total, per_instruction, dynamic, trace }
    }

    /// Estimated executions of one static instruction.
    pub fn instruction(&self, opcode: u16) -> f64 {
        self.per_instruction.get(&opcode).copied().unwrap_or(0.0)
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
