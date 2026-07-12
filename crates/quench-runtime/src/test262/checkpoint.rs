//! Checkpoint state for the staged test262 runner.
//!
//! Stored as `stage,index` in a plain text file so a run can resume
//! exactly where the last failure stopped it.

use std::fs;

/// Resume position: which stage and which test index within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Checkpoint {
    pub stage: usize,
    pub index: usize,
}

impl Checkpoint {
    /// Load a checkpoint from `path`, or None if missing/malformed.
    pub fn load(path: &str) -> Option<Self> {
        let s = fs::read_to_string(path).ok()?;
        let mut parts = s.trim().split(',');
        let stage = parts.next()?.parse().ok()?;
        let index = parts.next()?.parse().ok()?;
        Some(Self { stage, index })
    }

    /// Persist the checkpoint so the next run resumes here.
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        fs::write(path, format!("{},{}", self.stage, self.index))
    }

    /// Move to the first test of the next stage.
    pub fn advance_stage(&mut self) {
        self.stage += 1;
        self.index = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let path = std::env::temp_dir().join("quench_test262_checkpoint_test");
        let path = path.to_str().unwrap();
        let cp = Checkpoint {
            stage: 7,
            index: 42,
        };
        cp.save(path).unwrap();
        assert_eq!(Checkpoint::load(path), Some(cp));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_file_loads_none() {
        assert_eq!(Checkpoint::load("/nonexistent/.test262_checkpoint"), None);
    }

    #[test]
    fn advance_resets_index() {
        let mut cp = Checkpoint { stage: 3, index: 9 };
        cp.advance_stage();
        assert_eq!(cp, Checkpoint { stage: 4, index: 0 });
    }
}
