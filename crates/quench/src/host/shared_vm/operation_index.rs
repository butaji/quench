//! The host-operation catalog's compile-time projections: name order, ABI hash, the
//! operations that run beside activations, and name-to-identity resolution.

use super::OPERATION_DESCRIPTORS;
use quench_runtime::HostFunctionId;
use quench_runtime::property_names::same_text;

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

pub(super) const fn operation_beside_activations(name: &str) -> bool {
    let mut index = 0;
    while index < BESIDE_ACTIVATIONS.len() {
        if same_text(BESIDE_ACTIVATIONS[index], name) { return true; }
        index += 1;
    }
    false
}

/// Resolve host-owned literal names at Rust binding time, before any VM exists.
pub(crate) const fn static_operation(name: &str) -> HostFunctionId {
    let mut position = 0;
    while position < OPERATION_DESCRIPTORS.len() {
        if same_text(OPERATION_DESCRIPTORS[position].name, name) {
            assert!(position <= u32::MAX as usize);
            return HostFunctionId(position as u32);
        }
        position += 1;
    }
    panic!("unregistered Node operation")
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

/// The rows an operation declares in the binding table (`[row KIND(...)]`, `host_row!`), one
/// constructor per `HostRow` kind, naming operations by their literal names.
pub(super) mod host_rows {
    use quench_runtime::staged::host_rows::HostRow;

    /// `HostRow::PlainText`: past the first `bound` arguments, strings alone are first written
    /// by operation `plain`.
    pub(in crate::host::shared_vm) const fn plain_text(bound: u8, plain: &str) -> HostRow {
        HostRow::PlainText { bound, plain: super::static_operation(plain).0 }
    }
}
