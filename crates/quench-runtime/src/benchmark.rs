#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkRecord {
    pub workload: String,
    pub samples: u32,
    pub cycles: u64,
    pub operations: u64,
    pub branches: u64,
    pub allocations: u64,
    pub live_bytes: u64,
    pub rss_bytes: u64,
    pub binary_text_bytes: u64,
    pub generated_loc: u64,
    pub handwritten_loc: u64,
}

impl BenchmarkRecord {
    pub fn cycles_per_operation(&self) -> Option<f64> {
        (self.operations != 0).then(|| self.cycles as f64 / self.operations as f64)
    }
}
