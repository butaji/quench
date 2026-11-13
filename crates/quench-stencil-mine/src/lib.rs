//! Mines real workloads for the bytecode supernodes worth giving a stencil: straight-line
//! n-grams and dataflow trees, weighted by the dynamic opcode census or trace.
pub mod bits;
pub mod blocks;
pub mod dataflow;
pub mod inputs;
pub mod miner;
pub mod model;
pub mod ngram;
pub mod rank;
pub mod recommend;
pub mod report;
pub mod trees;
pub mod variants;
pub mod weights;

#[cfg(test)]
mod tests;
