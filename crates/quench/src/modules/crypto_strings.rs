//! UTF-8 string representation at Node's OpenSSL C API boundary.
/// Node Utf8Value comparisons and OpenSSL string arguments stop at the first nul.
pub(crate) fn null_terminated_prefix(value: &str) -> &str {
    value.split_once('\0').map_or(value, |(prefix, _)| prefix)
}
