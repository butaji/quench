//! Optional census of semantic-operation fallbacks reached from native images.

use crate::bytecode::{Op, WideInstruction};
use std::{collections::HashMap, sync::{Mutex, Once, atomic::{AtomicU64, Ordering}}};

static COUNTS: [AtomicU64; Op::COUNT] = [const { AtomicU64::new(0) }; Op::COUNT];
static REPORT: Once = Once::new();
const SITE_SAMPLE_INTERVAL: u64 = 4096;
const REPORTED_SITES: usize = 50;
static SAMPLE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
type SiteKey = (u32, u32, usize, usize, &'static str);
static SITES: Mutex<Option<HashMap<SiteKey, (u64, String)>>> = Mutex::new(None);

pub(super) fn record(opcode: usize) {
    if let Some(counter) = COUNTS.get(opcode) {
        REPORT.call_once(|| {
            // SAFETY: the report function and counters live until process exit.
            unsafe { libc::atexit(report) };
        });
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) fn record_site(
    program: u32, function: u32, pc: usize, instruction: WideInstruction, name: Option<&str>,
    classify: impl FnOnce() -> &'static str,
) {
    let sequence = SAMPLE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    // Rotate the sampled position between blocks to avoid aliasing a fixed loop period.
    if (sequence ^ (sequence / SITE_SAMPLE_INTERVAL)) % SITE_SAMPLE_INTERVAL != 0 {
        return;
    }
    let mut sites = SITES.lock().expect("native operation site census");
    let site = sites.get_or_insert_with(HashMap::new)
        .entry((program, function, pc, instruction.op() as usize, classify()))
        .or_insert_with(|| (0, format!("{} {instruction:?}", name.unwrap_or("<anonymous>"))));
    site.0 += 1;
}

extern "C" fn report() {
    let mut counts: Vec<_> = COUNTS.iter().enumerate()
        .map(|(opcode, count)| (opcode, count.load(Ordering::Relaxed)))
        .filter(|(_, count)| *count != 0)
        .collect();
    counts.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (opcode, count) in counts {
        eprintln!("QUENCH_NATIVE_OPERATION {} {count}", Op::NAMES[opcode]);
    }
    if let Some(sites) = SITES.lock().expect("native operation site census").take() {
        let mut reasons: HashMap<(usize, &'static str), u64> = HashMap::new();
        for ((_, _, _, opcode, reason), (count, _)) in &sites {
            *reasons.entry((*opcode, *reason)).or_default() += count;
        }
        let mut reasons: Vec<_> = reasons.into_iter().collect();
        reasons.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(*count));
        for ((opcode, reason), count) in reasons {
            eprintln!("QUENCH_NATIVE_REASON {} {reason} {count}", Op::NAMES[opcode]);
        }
        let mut sites: Vec<_> = sites.into_iter().collect();
        sites.sort_unstable_by_key(|(_, (count, _))| std::cmp::Reverse(*count));
        for ((program, function, pc, opcode, reason), (count, label)) in sites.into_iter().take(REPORTED_SITES) {
            eprintln!("QUENCH_NATIVE_SITE {program} {function} {pc} {} {count} {reason} {label}", Op::NAMES[opcode]);
        }
    }
}
