use super::*;

/// A collection boundary's release at a new high of the resident set. Resident code pages
/// count toward a run's peak only while the resident set is at its high: below it they cost
/// nothing, and at it the code touched since the last release (start-up's, the first calls',
/// the harness's) adds to the heap's high. So a collection that finds the resident set at a
/// new high releases the code when its file pages exceed what the run refaulted after the
/// last release (read at the second collection after it) by `NEW_HIGH_FILE_MARGIN_BYTES`: a
/// run whose heap stays below its high, or whose code is all working set, releases nothing
/// more. Collections are the run's own work clock, so the release follows allocation, not the
/// work-clock pace (`RUN_CODE_RELEASE_WORK`), which a run that peaks within its first 250 ms
/// of collections never reaches: JetStream 3's small tests peaked with their start-up code
/// resident (no-JIT, x86_64, 5 runs each, 2026-11-15, max RSS MB: Air 20.3 -> 18.3,
/// validatorjs 21.1 -> 18.8, Sunspider 29.0 -> 26.3, bigint-noble 19.3 -> 17.1, proxy-mobx
/// 17.2 -> 15.8, Babylon 16.1 -> 15.1, prismjs 20.4 -> 19.3, tsf 18.0 -> 16.9; V8-v7 regexp
/// 11.9 -> 11.2, navier-stokes 6.2 -> 5.9, other rows within 0.1; scores within noise).
/// Releasing at every second collection instead cost bigint-noble 15% of its score, and at
/// collections 1, 2, 4, 8 ... left prismjs's late peak with its working set resident.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
struct NewHigh {
    /// The largest resident set a collection boundary has seen.
    resident: std::sync::atomic::AtomicUsize,
    /// The file pages the run refaulted after the last release.
    refaulted: std::sync::atomic::AtomicUsize,
    collections_since_release: std::sync::atomic::AtomicUsize,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(super) static NEW_HIGH: NewHigh = NewHigh {
    resident: std::sync::atomic::AtomicUsize::new(0),
    refaulted: std::sync::atomic::AtomicUsize::new(0),
    collections_since_release: std::sync::atomic::AtomicUsize::new(0),
};

/// File pages above those refaulted since the last release that make a release at a new high
/// worth its refaults (`NewHigh`). At 1 MiB V8-v7's runs, whose code is 1-2 MB, kept code
/// the paced release had dropped (earley-boyer 9.3 -> 10.2 MB without the pace).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const NEW_HIGH_FILE_MARGIN_BYTES: usize = 256 << 10;

/// The collection after a release whose file pages are the baseline the run refaults against
/// (`NewHigh::refaulted`).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const REFAULT_BASELINE_COLLECTION: usize = 2;

/// Function records built on first use (`at_record_built`) between two new-high checks.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const RECORDS_PER_NEW_HIGH_CHECK: u32 = 16;

/// A function's record was built on its first use: every `RECORDS_PER_NEW_HIGH_CHECK`th
/// applies the new-high release (`NewHigh`), for runs that grow without collections. Code
/// joining the run is the work this follows, so the check falls at the same points of every
/// run: a Wasm program's lowered code and linear memory grow outside the collected heap, and
/// JetStream 3's sqlite3 ran 3 s without a collection with its start-up code resident
/// (no-JIT, x86_64, 2026-11-15, max RSS MB). The test is the collections' own, so a run whose
/// resident set stays below its high, or whose code is all working set, releases nothing; a
/// clock thread that released on every tick had peaked higher, refaulting its working set
/// within each interval.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn at_record_built() {
    static BUILT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let built = BUILT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if built % RECORDS_PER_NEW_HIGH_CHECK == 0
        && CODE_STAYS_RESIDENT.load(std::sync::atomic::Ordering::Relaxed)
        && NEW_HIGH.release_due()
    {
        release_read_only_pages_weighed(false);
    }
}

/// A function's record was built on its first use; nothing to release here.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn at_record_built() {}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl NewHigh {
    fn release_due(&self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        let (Some(resident), Some(file)) = (resident_statm_bytes(STATM_RESIDENT_FIELD), resident_file_bytes()) else {
            return false;
        };
        // A collection boundary or a check at built records (`at_record_built`).
        if self.collections_since_release.fetch_add(1, Relaxed) + 1 == REFAULT_BASELINE_COLLECTION {
            self.refaulted.store(file, Relaxed);
        }
        if resident < self.resident.load(Relaxed) {
            return false;
        }
        self.resident.store(resident, Relaxed);
        file >= self.refaulted.load(Relaxed) + NEW_HIGH_FILE_MARGIN_BYTES
    }
}
