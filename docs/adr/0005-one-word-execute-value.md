# ADR 0005: One-word execute values

- Status: Accepted
- Date: 2026-08-23
- Implements: Task 101; ADR 0003 rules 81-83

## Starting execute path

At the start of this change, `Value` is a 34-variant, 32-byte Rust enum.
Numbers and primitive tags are inline; `String(String)` and `BigInt(String)`
own variable-sized payloads directly; identity-bearing values own `Rc`
payloads. Registers, activation records, generator arguments, environments,
object slots, and array elements store `Value`. The live register stack is a
`Vec<Value>`, and `read_register` clones its selected element. Consequently a
`Move` of an inline string copies string bytes and sequential register access
fits only two values in a 64-byte cache line.

`tagged_value.rs` contains an isolated 8-byte NaN-box prototype, but the module
is not declared by `lib.rs` and therefore is not compiled into the runtime.
Its tests mention scalar `Value` adapters that do not exist. It also declares
its own index-plus-generation `HeapRef`, separate from the canonical
`identity::HeapRef` used by `HeapArena`. No live register, slot, element,
completion, call frame, or dispatch handler stores `TaggedValue`.

The compact instruction stream from ADR 0004 fetches 8-byte `Instruction`
records, but its operands still address the 32-byte `Value` register stack.
`Machine::RegisterWindow`, `vm::Activation`, and all ordinary execution entry
points expose `Vec<Value>`.

## Decision

One macro declaration owns each tag number and generated tag accessor.
Handwritten code remains only for numeric canonicalization and heap lifetime
behavior. Compile-time contracts enforce an 8-byte, 8-byte-aligned execute
word.

The canonical execute representation will be `TaggedValue`. Immediate
numbers, small integers, booleans, null, and undefined live in the word.
Object, array, and function words contain their existing `Rc` payload pointer
directly. The pointer is at least 8-byte aligned, so removing three proven-zero
bits fits the remaining address in the 45-bit payload. Less-common heap values
use the same encoding with one aligned `Rc<Value>` boundary allocation.

Register moves copy one word and retain one heap reference when necessary.
Decoding clones the heap-owned semantic value only at a boundary requiring
`Value`; `Move` never clones a `String`. Slow operations consume the same
tagged words as fast operations, so there is no shadow register vector.

## Ownership, lifecycle, and invalid states

Each heap word owns one strong reference. Copy and destruction are the only
retain/release events, and the tag determines the exact Rust payload type.
An unaligned or unrepresentable pointer, unknown tag, or noncanonical
singleton payload is invalid. Pointer compression is lossless: no address bits
other than the three alignment facts are discarded.

An earlier measured prototype used an index-plus-generation arena. Richards
retired 141.4B instructions because every heap read required a `RefCell`
borrow, generation check, and slot lookup. Direct typed payloads reduced that
to 136.3B while retaining the same one-word register contract.

Host APIs may accept and return `Value` at the boundary during migration, but
the conversion is an edge operation: the active machine does not retain both
a `Value` register vector and a tagged shadow vector.

## Measurement contract

Evidence must show `size_of::<TaggedValue>() == 8`, eight execute words per
64-byte line, no string clone on register `Move`, preserved JavaScript output,
and before/after wall time, retired instructions, peak RSS, and V8-v7 score
from the `bench-throughput` profile.
