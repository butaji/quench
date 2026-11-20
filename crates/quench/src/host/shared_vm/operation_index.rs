//! The host-operation catalog's compile-time projections: name order, ABI hash, the
//! operations that run beside activations, and name-to-identity resolution.

use super::OPERATION_DESCRIPTORS;
use quench_runtime::{HostFunctionDescriptor, HostFunctionId};

/// Stable bottom-up merge sort. Scratch storage exists only during constant
/// evaluation; the image retains one u16 per operation and no copied names.
const fn sorted_indices<const N: usize>(descriptors: &[HostFunctionDescriptor]) -> [u16; N] {
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

const ABI_HASH_OFFSET: u64 = 0xcbf29ce484222325;
const ABI_HASH_PRIME: u64 = 0x100000001b3;
const ABI_FIELD_END: u8 = u8::MAX;

pub(super) const fn operation_abi(scope: &str, name: &str, length: u64, callback: &str) -> u64 {
    let mut hash = ABI_HASH_OFFSET;
    let fields = [scope, name, callback];
    let mut field = 0;
    while field < fields.len() {
        let bytes = fields[field].as_bytes();
        let mut byte = 0;
        while byte < bytes.len() {
            hash = (hash ^ bytes[byte] as u64).wrapping_mul(ABI_HASH_PRIME);
            byte += 1;
        }
        hash = (hash ^ ABI_FIELD_END as u64).wrapping_mul(ABI_HASH_PRIME);
        field += 1;
    }
    (hash ^ length).wrapping_mul(ABI_HASH_PRIME)
}

pub(super) const fn bootstrap_abi(words: &[u64]) -> u64 {
    let mut hash = ABI_HASH_OFFSET;
    let mut word = 0;
    while word < words.len() {
        hash = (hash ^ words[word]).wrapping_mul(ABI_HASH_PRIME);
        word += 1;
    }
    hash
}

pub(super) const fn operation_bits<const WORDS: usize>(policies: &[bool]) -> [u64; WORDS] {
    let mut words = [0; WORDS];
    let mut index = 0;
    while index < policies.len() {
        if policies[index] { words[index / u64::BITS as usize] |= 1 << (index % u64::BITS as usize); }
        index += 1;
    }
    words
}

/// String equality during constant evaluation.
const fn same_name(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut byte = 0;
    while byte < left.len() && left[byte] == right[byte] { byte += 1; }
    byte == left.len()
}

pub(super) const fn operation_beside_activations(name: &str) -> bool {
    let mut index = 0;
    while index < BESIDE_ACTIVATIONS.len() {
        if same_name(BESIDE_ACTIVATIONS[index], name) { return true; }
        index += 1;
    }
    false
}

/// Resolve host-owned literal names at Rust binding time, before any VM exists.
pub(crate) const fn static_operation(name: &str) -> HostFunctionId {
    let mut position = 0;
    while position < OPERATION_DESCRIPTORS.len() {
        if same_name(OPERATION_DESCRIPTORS[position].name, name) {
            assert!(position <= u32::MAX as usize);
            return HostFunctionId(position as u32);
        }
        position += 1;
    }
    panic!("unregistered Node operation")
}

/// The host function registered as `name`. Hosts resolve operations per request (an
/// incoming message's stream methods, an accepted socket's handle), so the names are
/// indexed once, by a binary search, rather than scanned among every binding. Each name
/// registers exactly one function (`operation_names_are_unique`).
pub(crate) fn operation(name: &str) -> HostFunctionId {
    // Resolve immutable declaration order during compilation, without first-use
    // allocation, sorting or synchronization. Each entry is only a binding index.
    static INDEX: [u16; OPERATION_DESCRIPTORS.len()] = sorted_indices(OPERATION_DESCRIPTORS);
    let descriptors = OPERATION_DESCRIPTORS;
    let index = &INDEX;
    let first = index.partition_point(|&position| descriptors[usize::from(position)].name < name);
    let position = index
        .get(first)
        .copied()
        .filter(|&position| descriptors[usize::from(position)].name == name)
        .unwrap_or_else(|| panic!("unregistered Node operation: {name}"));
    HostFunctionId(u32::from(position))
}

/// The host operations that run beside a lane's frameless activations
/// (`host_function_runs_beside_activations`).
pub(super) const BESIDE_ACTIVATIONS: &[&str] = &[
    "emitterListenerCount",
    "emitterEmitListeners",
    "bufferBindingByteLengthUtf8",
    "bufferBindingAsciiSlice",
    "bufferBindingBase64Slice",
    "bufferBindingBase64urlSlice",
    "bufferBindingHexSlice",
    "bufferBindingLatin1Slice",
    "bufferBindingUcs2Slice",
    "bufferBindingUtf8Slice",
    "bufferBindingUtf8Write",
    "utilTypeCheck",
    "nextTick",
    "asyncResourceInit",
    "asyncResourceEnterScope",
    "asyncResourceExitScope",
    "stringDecoderDecode",
    "markAsUncloneable",
    "byteStringEnd",
    "headerFields",
    // Socket, timer, decoder and listener bookkeeping a request or message makes several
    // times (ws, undici, esbuild and Fastify package censuses): none reads the frame stack
    // itself, and the guest code or errors any of them reaches take frames first
    // (`Vm::materialize_deferred_lane`).
    "netSocketAccountWrite",
    "netSocketRead",
    "netSocketWrite",
    "netSocketReference",
    "setImmediate",
    "clearTimeout",
    "textDecoderDecode",
    "encodingUtf8",
    "emitterOnce",
    "emitterAddListener",
    "emitterRemoveListener",
    "primordialsCopy",
    "childProcessReference",
];

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
