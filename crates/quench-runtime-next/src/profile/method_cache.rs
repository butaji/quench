use super::Profile;

impl Profile {
    pub fn method_cache_clear(&mut self, reason: usize, entries: usize) {
        self.method_cache_cleared[reason] += entries as u64;
    }

    pub fn method_cache_refill(&mut self, reason: Option<usize>, same_target: bool) {
        self.method_cache_refills[reason.map_or(0, |value| value + 1)] += 1;
        if let Some(reason) = reason {
            self.method_cache_same_targets[reason] += u64::from(same_target);
        }
    }
}
