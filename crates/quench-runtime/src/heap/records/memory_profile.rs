//! Page coverage derived from record occupancy, without reading dead record storage.
use super::RecordSpace;

impl<T, const CAPACITY: usize> RecordSpace<T, CAPACITY> {
    /// Fully enclosed pages below the allocation high-water mark and whether each contains
    /// any live record bytes. Partial boundary pages are excluded: they can share allocator
    /// metadata or storage outside this range. A record may straddle several pages.
    #[cfg(any(test, target_os = "macos", target_os = "linux"))]
    fn page_coverage(&self, page_bytes: usize) -> (usize, Vec<bool>) {
        assert!(page_bytes.is_power_of_two());
        if !self.is_reserved() { return (0, Vec::new()); }
        let base = self.base.as_ptr() as usize;
        let start = base.next_multiple_of(page_bytes);
        let end = (base + self.next_id as usize * size_of::<T>()) / page_bytes * page_bytes;
        let mut occupied = vec![false; end.saturating_sub(start) / page_bytes];
        self.for_each_live(|id, _| {
            let record_start = base + id as usize * size_of::<T>();
            let record_end = record_start + size_of::<T>();
            if record_start < end && record_end > start {
                let first = record_start.saturating_sub(start) / page_bytes;
                let last = (record_end.min(end) - start).div_ceil(page_bytes);
                occupied[first..last].fill(true);
            }
        });
        (start, occupied)
    }

    pub(in crate::heap) fn report_pages(&self, family: &str, phase: &str) {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            // SAFETY: sysconf reads the host's page size without touching the arena.
            let Ok(page_bytes) = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }) else { return };
            if !page_bytes.is_power_of_two() { return; }
            let (start, occupied) = self.page_coverage(page_bytes);
            let mut residency = vec![0u8; occupied.len()];
            // The low residency bit is the mincore ABI's resident-page flag on both hosts.
            const MINCORE_RESIDENT: u8 = 1;
            // SAFETY: each queried full page lies inside the live arena allocation; mincore
            // writes one byte per page to the equally sized output vector and reads no T.
            let available = occupied.is_empty() || unsafe {
                libc::mincore(start as *mut libc::c_void, occupied.len() * page_bytes,
                    residency.as_mut_ptr().cast()) == 0
            };
            let empty = occupied.iter().filter(|&&live| !live).count();
            let resident = available.then(|| residency.iter().filter(|&&bits| bits & MINCORE_RESIDENT != 0).count());
            let empty_resident = available.then(|| occupied.iter().zip(&residency)
                .filter(|(live, bits)| !**live && **bits & MINCORE_RESIDENT != 0).count());
            let json_count = |count: Option<usize>| count.map_or_else(|| "null".into(), |n| n.to_string());
            eprintln!("{{\"kind\":\"quench-record-pages\",\"phase\":\"{phase}\",\"family\":\"{family}\",\"page_bytes\":{page_bytes},\"full_pages\":{},\"empty_pages\":{empty},\"resident_pages\":{},\"empty_resident_pages\":{}}}",
                occupied.len(), json_count(resident), json_count(empty_resident));
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let _ = (family, phase);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::pacing::PromotionAge;

    #[test]
    fn page_coverage_tracks_straddling_records_and_sweep_without_reading_dead_values() {
        let mut arena: RecordSpace<[u8; 24], 1024> = RecordSpace::default();
        for _ in 0..1024 { arena.try_insert([0; 24]).unwrap(); }
        let (start, full) = arena.page_coverage(4096);
        assert!(full.len() >= 5 && full.iter().all(|&live| live));
        let id = ((start + 4096 - arena.base.as_ptr() as usize) / 24) as u32;
        arena.mark(id);
        arena.sweep(true, PromotionAge::First, true, |_| {});
        let (_, one) = arena.page_coverage(4096);
        let record_start = arena.base.as_ptr() as usize + id as usize * 24;
        for (index, &live) in one.iter().enumerate() {
            let page_start = start + index * 4096;
            assert_eq!(live, record_start < page_start + 4096 && record_start + 24 > page_start);
        }
        arena.clear_marks();
        arena.sweep(true, PromotionAge::First, true, |_| {});
        assert!(arena.page_coverage(4096).1.iter().all(|&live| !live));
    }
}
