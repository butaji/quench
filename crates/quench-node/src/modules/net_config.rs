//! Node network invocation settings shared by the host and TCP adapter.

pub(crate) const DEFAULT_AUTO_SELECT_FAMILY: bool = true;
pub(crate) const ANY_FAMILY: u8 = 0;
pub(crate) const IPV4_FAMILY: u8 = 4;
pub(crate) const IPV6_FAMILY: u8 = 6;

pub(crate) const DEFAULT_AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT: u64 = 500;
const MIN_AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT: u64 = 10;

pub(crate) fn normalize_auto_select_family_attempt_timeout(timeout_ms: u64) -> u64 {
    timeout_ms.max(MIN_AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT)
}

pub(crate) fn auto_select_family_attempt_timeout_from_exec_argv(
    exec_argv: &[String],
) -> Option<u64> {
    exec_argv.iter().rev().find_map(|argument| {
        let value = argument.strip_prefix("--network-family-autoselection-attempt-timeout=")?;
        value.parse::<u64>().ok()
    })
}

pub(crate) fn auto_select_family_from_exec_argv(exec_argv: &[String]) -> Option<bool> {
    exec_argv
        .iter()
        .rev()
        .find_map(|argument| match argument.as_str() {
            "--no-network-family-autoselection" => Some(false),
            "--network-family-autoselection" => Some(true),
            _ => None,
        })
}
