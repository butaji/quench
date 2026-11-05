//! Compile-time name order for the immutable host-operation declarations.

use quench_runtime::HostFunctionDescriptor;

/// Stable bottom-up merge sort. Scratch storage exists only during constant
/// evaluation; the image retains one u16 per operation and no copied names.
pub(super) const fn sorted_indices<const N: usize>(descriptors: &[HostFunctionDescriptor]) -> [u16; N] {
    assert!(N == descriptors.len(), "operation index must cover the descriptor table");
    assert!(N <= u16::MAX as usize + 1, "Node binding index fits u16");
    let mut indices = [0; N];
    let mut position = 0;
    while position < N {
        indices[position] = position as u16;
        position += 1;
    }
    let mut scratch = [0; N];
    let mut width = 1;
    while width < N {
        let mut start = 0;
        while start < N {
            let middle = if start + width < N { start + width } else { N };
            let end = if middle + width < N { middle + width } else { N };
            let (mut left, mut right, mut output) = (start, middle, start);
            while output < end {
                if left < middle && (right == end || name_before_or_equal(
                    descriptors[indices[left] as usize].name,
                    descriptors[indices[right] as usize].name,
                )) {
                    scratch[output] = indices[left];
                    left += 1;
                } else {
                    scratch[output] = indices[right];
                    right += 1;
                }
                output += 1;
            }
            start = end;
        }
        indices = scratch;
        width *= 2;
    }
    indices
}

const fn name_before_or_equal(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    let mut position = 0;
    while position < left.len() && position < right.len() {
        if left[position] != right[position] { return left[position] < right[position]; }
        position += 1;
    }
    left.len() <= right.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_order_matches_stable_string_sort() {
        const NAMES: &[&str] = &["z", "aa", "a", "", "λ", "a", "é", "aaa", "a"];
        const DESCRIPTORS: [HostFunctionDescriptor; NAMES.len()] = {
            let mut descriptors = [HostFunctionDescriptor { name: "", length: 0, global: None }; NAMES.len()];
            let mut i = 0;
            while i < NAMES.len() { descriptors[i].name = NAMES[i]; i += 1; }
            descriptors
        };
        const ORDER: [u16; DESCRIPTORS.len()] = sorted_indices(&DESCRIPTORS);
        let mut expected: Vec<u16> = (0..DESCRIPTORS.len() as u16).collect();
        expected.sort_by_key(|&index| DESCRIPTORS[index as usize].name);
        assert_eq!(ORDER.as_slice(), expected);
        assert!(sorted_indices::<0>(&[]).is_empty());
        assert_eq!(sorted_indices::<1>(&DESCRIPTORS[..1]), [0]);
    }
}
