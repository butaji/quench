//! Internal helpers extracted from `path_win32_extra`.
/// Walk both paths in lock-step and record the last matching `\`
/// separator position; stop at first divergence.
pub(crate) fn common_prefix_len(
    f: &[char],
    t: &[char],
    from_start: usize,
    to_start: usize,
    length: usize,
) -> (isize, usize) {
    let mut last_common_sep: isize = -1;
    let mut i = 0usize;
    while i < length {
        if f[from_start + i] != t[to_start + i] {
            break;
        }
        if f[from_start + i] == '\\' {
            last_common_sep = i as isize;
        }
        i += 1;
    }
    (last_common_sep, i)
}
