//! Node network invocation settings shared by the host and TCP adapter.

pub(crate) const DEFAULT_AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT: u64 = 2500;
const MIN_AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT: u64 = 10;

pub(crate) fn normalize_auto_select_family_attempt_timeout(timeout_ms: u64) -> u64 {
    timeout_ms.max(MIN_AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT)
}

pub(crate) fn auto_select_family_attempt_timeout_from_exec_argv(
    exec_argv: &[String],
) -> Option<u64> {
    exec_argv.iter().find_map(|argument| {
        let value = argument.strip_prefix("--network-family-autoselection-attempt-timeout=")?;
        let timeout_ms = value.parse::<u64>().ok()?;
        Some(normalize_auto_select_family_attempt_timeout(timeout_ms))
    })
}
