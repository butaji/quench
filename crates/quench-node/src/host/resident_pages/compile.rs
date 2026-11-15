use super::*;

/// The host's action at a compilation's phase boundaries (`set_compile_phase_boundary`), each
/// at most once per its interval (`LOAD_RELEASE_INTERVAL`). Code generation pages code and data out
/// (`release_read_only_code_and_data`); compilations that follow it quickly (functions a
/// program builds at run time with `new Function`, `eval` or `vm`) run the same parser and
/// code generator, which a release would only refault, and each release reads the whole
/// mapping table (7% of the `@testing-library/dom` npm scenario's instructions). First calls
/// and lane derivations release nothing: a run's code refaulted after each release, and
/// JetStream 3 without those releases faulted about half as often, ran 5-12% less CPU on its
/// startup tests (prismjs-startup-es6 0.43 -> 0.38 s, crypto 0.24 -> 0.21) and peaked at most
/// 1-2 MB higher (Babylon 12.9 -> 14.6 MB), 2026-11-07.
pub fn at_compile_phase(phase: quench_runtime::CompilePhase) {
    match phase {
        // The entry's analysis and code generation release whatever the pace says
        // (`EntryCompile`); other compilations' analyses run on the parser's pages.
        quench_runtime::CompilePhase::Analysis => {
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            if EntryCompile::Analysis.advance() {
                release_read_only_code_and_data();
            }
        }
        quench_runtime::CompilePhase::CodeGeneration => {
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            if EntryCompile::CodeGeneration.advance() {
                return release_read_only_code_and_data();
            }
            release_once_per_interval(CompileRelease::CodeGeneration, release_read_only_code_and_data)
        }
        quench_runtime::CompilePhase::Encoding => {
            release_once_per_interval(CompileRelease::Encoding, release_read_only_code_and_data)
        }
    }
}

/// The release before a kept program loads (`Host::kept_compiled_program`), at most once per
/// `LOAD_RELEASE_INTERVAL`. A bootstrap loads its builtin modules from the compile cache
/// with no compilation boundary between them, so the code each module's set-up ran stayed
/// resident beside the next one's: V8-v7 Richards start-up peaked at 4.0 MB, 3.2 MB with
/// these releases and the two before the bootstrap (`execute_shared_on_worker`).
pub fn release_at_module_load() {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if release_due() {
        release_read_only_pages();
    }
}

/// The host's action once a compilation offers its program to the compile cache
/// (`Host::keep_compiled_program`): the compilation is over, and the program runs from its
/// kept form, so the parser's, code generator's and encoder's pages leave before its decode
/// and run fault theirs. A cold run compiles its entry (V8-v7 MemoryFirst cold first run,
/// bolted static x86_64, 9 interleaved runs, 2026-10-25: median max RSS Richards 2700 ->
/// 2448 KiB, DeltaBlue 2880 -> 2604, Crypto 3060 -> 2928, RayTrace 2852 -> 2784); a warm
/// run keeps nothing and never reaches it.
pub fn after_program_kept() {
    release_once_per_interval(CompileRelease::Kept, release_read_only_code_and_data);
}

/// The compile-phase releases timed apart from each other (`at_compile_phase`).
#[derive(Clone, Copy)]
enum CompileRelease {
    CodeGeneration,
    Encoding,
    Kept,
}

impl CompileRelease {
    #[cfg_attr(not(all(target_os = "linux", target_arch = "x86_64")), allow(dead_code))]
    const COUNT: usize = 3;
}

/// The process's own memory counts in pages, space-separated (proc(5)).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const STATM_PATH: &str = "/proc/self/statm";

/// Room for `STATM_PATH`'s seven decimal counts and their separators.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const STATM_BYTES: usize = 128;

/// The field of `STATM_PATH` that counts resident file-backed (and shared) pages.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const STATM_RESIDENT_SHARED_FIELD: usize = 2;

/// The field of `STATM_PATH` that counts all resident pages.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const STATM_RESIDENT_FIELD: usize = 1;

/// The bytes of resident file-backed pages (`STATM_PATH`), read through one descriptor kept
/// open: a budget check runs at each paced compile phase.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn resident_file_bytes() -> Option<usize> {
    resident_statm_bytes(STATM_RESIDENT_SHARED_FIELD)
}

/// The bytes of `STATM_PATH`'s page count at `field`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn resident_statm_bytes(field: usize) -> Option<usize> {
    use std::os::unix::fs::FileExt;
    static STATM: std::sync::OnceLock<Option<std::fs::File>> = std::sync::OnceLock::new();
    let file = STATM.get_or_init(|| std::fs::File::open(STATM_PATH).ok()).as_ref()?;
    let mut text = [0; STATM_BYTES];
    let length = file.read_at(&mut text, 0).ok()?;
    let pages = std::str::from_utf8(&text[..length])
        .ok()?
        .split_ascii_whitespace()
        .nth(field)?
        .parse::<usize>()
        .ok()?;
    pages.checked_mul(page_size())
}

/// Resident file pages below this share (`1 / divisor`) of the resident set are not worth
/// releasing: a release saves at most their bytes and costs the refaults of the code the run
/// touches again. A run whose heap is a few megabytes keeps code at a large share (V8-v7, file
/// pages about 1.3-2 MB of 3-5 MB) and keeps releasing; a run whose heap dwarfs its code
/// (JetStream's tests, 20-160 MB) no longer refaults its working code tens of times per
/// iteration (web-ssr: about 80 releases and 21K refaults per iteration).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const FILE_PAGES_RELEASE_SHARE_DIVISOR: usize = 4;

/// Whether the resident file pages are a large enough share of the resident set to release
/// (`FILE_PAGES_RELEASE_SHARE_DIVISOR`); `true` when either count cannot be read.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn file_pages_weigh() -> bool {
    match (resident_file_bytes(), resident_statm_bytes(STATM_RESIDENT_FIELD)) {
        (Some(file), Some(resident)) => file * FILE_PAGES_RELEASE_SHARE_DIVISOR >= resident,
        _ => true,
    }
}

/// When each compile-phase release last ran, indexed by `CompileRelease`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
static LAST_COMPILE_RELEASES: std::sync::Mutex<[Option<std::time::Instant>; CompileRelease::COUNT]> =
    std::sync::Mutex::new([None; CompileRelease::COUNT]);

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn release_once_per_interval(release: CompileRelease, action: fn()) {
    let Ok(mut releases) = LAST_COMPILE_RELEASES.lock() else {
        return;
    };
    let last = &mut releases[release as usize];
    if last.is_some_and(|last| last.elapsed() < LOAD_RELEASE_INTERVAL) {
        return;
    }
    *last = Some(std::time::Instant::now());
    drop(releases);
    action();
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn release_once_per_interval(_: CompileRelease, _: fn()) {}

/// Releases before the entry's compilation, the start-up peak, and lets its own code
/// generation boundary release again whatever the release pace says: only the
/// parser's and compiler's pages, not the bootstrap's, need be resident across it, and the
/// parser's not across its code generation. A bootstrap compilation within the interval had
/// held that boundary (V8-v7 max RSS, entry compiled with the bootstrap cached, bolted static
/// x86_64, 2026-10-22: Crypto 3100-3276 -> 2704-3052 KiB, RayTrace 2996 -> 2764, Richards
/// 2812 -> 2484).
pub fn release_before_entry_compile() {
    phase_peak("start-up");
    release_read_only_code_and_data();
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    ENTRY_COMPILE.store(EntryCompile::Analysis as u8, std::sync::atomic::Ordering::Relaxed);
}

/// The main entry is compiled; the default policy keeps the code its run touches (`CODE_STAYS_RESIDENT`).
pub fn release_after_entry_compile() {
    release_read_only_code_and_data();
    release_main_text_at_phase_boundary();
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    let memory_first = crate::shared_run::process_memory_policy() == Some(quench_runtime::MemoryPolicy::MemoryFirst);
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    CODE_STAYS_RESIDENT.store(!memory_first, std::sync::atomic::Ordering::Relaxed);
}

/// Set once the main entry has compiled under the default policy: start-up's releases drop the bootstrap's code
/// (about 5 MB), and the run's own, which refaulted what it runs again, wait for `RUN_CODE_BUDGET_BYTES`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(super) static CODE_STAYS_RESIDENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Resident file bytes a run keeps before releasing them (x86_64, 2026-11-08, minor faults and max RSS against
/// releasing throughout: richards 11.9K -> 5.2K, 12 MB both; Air 23.6K -> 9.3K, 21 MB; octane-code-load 15K ->
/// 8.6K, 17 -> 18 MB; npm fastify 32.8K -> 14.7K, 34 -> 31 MB; vite 35K -> 17K, 43 -> 35 MB). At 10 MB Air
/// refaulted 18K; at 16 MB fastify peaked at 36 MB.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const RUN_CODE_BUDGET_BYTES: usize = 12 << 20;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn run_code_stays() -> bool {
    CODE_STAYS_RESIDENT.load(std::sync::atomic::Ordering::Relaxed) && resident_file_bytes().is_some_and(|bytes| bytes < RUN_CODE_BUDGET_BYTES)
}

/// Retire the previous phase's resident main-image translations. Code needed by
/// the next phase refaults at the same addresses from the same backing object.
/// Other platforms retain their existing phase-specific release policy.
pub fn release_main_text_at_phase_boundary() {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    darwin::release_main_text();
}

/// The entry compilation's boundaries still due to release, in order (`ENTRY_COMPILE`). The
/// parser's pages leave before the analysis as the analyser's do before code generation: a
/// cold entry's parse and analysis had run without a release, and their code (870 KB of file
/// pages) stayed resident beside the parse tree and the analysis tables at the run's peak
/// (V8-v7 Crypto, entry not cached, memory-first, 2026-10-29: 1860 KiB of heap beside
/// 916 KiB of file pages, against a warm run's 2724 KiB peak).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(Clone, Copy)]
#[repr(u8)]
enum EntryCompile {
    /// No entry compilation is under way.
    Done,
    /// The entry's analysis boundary has yet to come.
    Analysis,
    /// The entry's code generation boundary has yet to come.
    CodeGeneration,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl EntryCompile {
    /// Passes the entry compilation's boundary `self`: whether it was still due. Code
    /// generation is due from either open state, so a compilation without an analysis
    /// boundary still releases there.
    fn advance(self) -> bool {
        let (due, next): (&[Self], Self) = match self {
            Self::Analysis => (&[Self::Analysis], Self::CodeGeneration),
            Self::CodeGeneration => (&[Self::Analysis, Self::CodeGeneration], Self::Done),
            Self::Done => (&[], Self::Done),
        };
        let ordering = std::sync::atomic::Ordering::Relaxed;
        due.iter().any(|&open| ENTRY_COMPILE.compare_exchange(open as u8, next as u8, ordering, ordering).is_ok())
    }
}

/// The entry compilation's state (`EntryCompile`), opened before it starts
/// (`release_before_entry_compile`).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
static ENTRY_COMPILE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(EntryCompile::Done as u8);
