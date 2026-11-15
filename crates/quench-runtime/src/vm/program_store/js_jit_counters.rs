use super::*;

#[derive(Default)]
pub(super) struct JsJitCounters {
    pub(super) source_name: Option<Box<str>>,
    pub(super) function_name: Option<Box<str>>,
    pub(super) parameter_count: u16,
    pub(super) instruction_count: u32,
    pub(super) guard_conditional_branches: u32,
    pub(super) guard_typeof_ops: u32,
    pub(super) guard_strict_comparisons: u32,
    pub(super) guard_relational_comparisons: u32,
    pub(super) entries: u64,
    pub(super) lane_entries: u64,
    pub(super) numeric_entries: u64,
    pub(super) promotions: u64,
    pub(super) compile_attempts: u64,
    pub(super) compile_successes: u64,
    pub(super) rejection_reason: Option<&'static str>,
    pub(super) compile_ns: u64,
    pub(super) native_entries: u64,
    pub(super) field_cache_probes: u64,
    pub(super) field_cache_hits: u64,
    pub(super) field_cache_sites: FxHashMap<u16, (u64, u64)>,
    pub(super) field_store_cache_probes: u64,
    pub(super) field_store_cache_hits: u64,
    pub(super) field_return_candidate_entries: u64,
    pub(super) field_return_fixed_this_entries: u64,
    pub(super) field_return_fast_entries: u64,
    pub(super) field_increment_candidate_entries: u64,
    pub(super) field_increment_fixed_this_entries: u64,
    pub(super) field_increment_fast_entries: u64,
    pub(super) captured_store_fast_entries: u64,
    pub(super) numeric_find_fast_entries: u64,
    pub(super) fallback_entries: u64,
    pub(super) interpreter_completions: u64,
    pub(super) interpreter_inclusive_ns: u64,
    pub(super) opcode_counts: Vec<(u16, u32)>,
    pub(super) call_sites: Vec<JsJitCallSiteCounters>,
    pub(super) code_bytes: usize,
}

#[derive(Default)]
pub(super) struct JsJitCallSiteCounters {
    pub(super) pc: u32,
    pub(super) opcode: u16,
    pub(super) attempts: u64,
    pub(super) direct_target: Option<(u32, u32)>,
    pub(super) direct_target_calls: u64,
    pub(super) alternate_target_calls: u64,
    pub(super) other_target_calls: u64,
    pub(super) outcomes: [u64; JsJitCallOutcome::COUNT],
    pub(super) sampled_calls: u64,
    pub(super) sampled_call_ns: [u64; JsJitCallOutcome::COUNT],
}

#[cfg(feature = "native-execution")]
impl JsJitCallSiteCounters {
    /// Counts one call by where it went: the site's first direct target, a later call to it,
    /// a call to another direct target, or a target that is not a direct callee.
    pub(super) fn note_target(&mut self, direct_target: Option<(u32, u32)>) {
        match direct_target {
            Some(target) => match self.direct_target {
                None => {
                    self.direct_target = Some(target);
                    self.direct_target_calls = self.direct_target_calls.saturating_add(1);
                }
                Some(first) if first == target => {
                    self.direct_target_calls = self.direct_target_calls.saturating_add(1);
                }
                Some(_) => {
                    self.alternate_target_calls = self.alternate_target_calls.saturating_add(1);
                }
            },
            None => {
                self.other_target_calls = self.other_target_calls.saturating_add(1);
            }
        }
    }
}

#[cfg(feature = "native-execution")]
pub(super) const JS_JIT_CALL_DIAGNOSTIC_SAMPLE_INTERVAL: u64 = 256;

#[derive(Clone, Copy)]
#[repr(usize)]
pub(in crate::vm) enum JsJitCallOutcome {
    Continue = 0,
    TailCall = 1,
    PushFrame = 2,
    Return = 3,
    Await = 4,
    Yield = 5,
    Error = 6,
}

impl JsJitCallOutcome {
    pub(super) const COUNT: usize = Self::Error as usize + 1;

    pub(super) const ALL: [Self; Self::COUNT] = [
        Self::Continue,
        Self::TailCall,
        Self::PushFrame,
        Self::Return,
        Self::Await,
        Self::Yield,
        Self::Error,
    ];

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::TailCall => "tail_call",
            Self::PushFrame => "push_frame",
            Self::Return => "return",
            Self::Await => "await",
            Self::Yield => "yield",
            Self::Error => "error",
        }
    }
}

pub(super) const JS_JIT_MAX_FUNCTIONS: usize = 64;
// Tier observations include dynamically generated functions across all retained
// programs. Web-SSR exposes more than 1,300 functions in one VM; keep a fixed
// metadata bound large enough to observe that workload without changing the
// independent compiled-artifact or executable-memory budgets.
pub(super) const JS_JIT_MAX_TRACKED_FUNCTIONS: usize = 2048;
pub(super) const JS_JIT_MAX_DIAGNOSTIC_FUNCTIONS: usize = 4096;
pub(super) const JS_JIT_MAX_REJECTED_FUNCTIONS: usize = 4096;
pub(super) const JS_JIT_REJECTION_DIAGNOSTIC_ENTRIES: u64 = 32;
// Executable mappings were once page-rounded. A 64 KiB cap then admitted only
// sixteen tiny JS artifacts on 4 KiB systems, starving later high-frequency
// helpers; the function-count cap still bounds artifact proliferation.
pub(super) const JS_JIT_MAX_MAPPED_BYTES: usize = 512 * 1024;
