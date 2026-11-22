//! The one frame's layout: the words of a call in progress that emitted code reads and
//! writes at fixed offsets. Everything else (return linkage, kind) belongs to the lane, which
//! asserts these against the record it lays out (`crate::vm::engine_types::Activation`).

use core::mem::{offset_of, size_of};

/// Bytes in a word: frame offsets are byte counts, context and window indices are word counts.
const WORD_BYTES: usize = size_of::<usize>();

/// The four words every frame starts with, in address order.
#[repr(C)]
#[allow(dead_code)]
struct FramePrefix {
    callee: usize,
    this: usize,
    body: usize,
    window: usize,
}

/// The callee's descriptor (`DirectCallee`), null for a frame the lane made.
pub(crate) const CALLEE: usize = offset_of!(FramePrefix, callee);
/// The callee's receiver, as value bits.
pub(crate) const THIS: usize = offset_of!(FramePrefix, this);
/// The published body (`LaneMethod`) the frame runs, 0 when none.
pub(crate) const BODY: usize = offset_of!(FramePrefix, body);
/// The address of the frame's first register; its registers, then what it keeps, follow.
pub(crate) const WINDOW: usize = offset_of!(FramePrefix, window);
/// The prefix every frame starts with.
pub(crate) const PREFIX_BYTES: usize = size_of::<FramePrefix>();
const _: () = assert!(CALLEE == 0);
const _: () = assert!(THIS == 8);
const _: () = assert!(BODY == 16);
const _: () = assert!(WINDOW == 24);
const _: () = assert!(PREFIX_BYTES == 32);

/// The context words below every window's register 0 (call protocol v2), in address order:
/// the runtime's `register_file::FRAME_CONTEXT` and the b-jit's `FRAME_CONTEXT_WORDS` are this.
/// Counted down from register 0: the receiver, environment, captures, then a plain
/// activation's header (the caller's call record, registers and records, the packed word, the
/// callee), then a body's captures word. A body's receiver and captures are written into its
/// own window by whoever enters it, so each frame carries its own and a call saves and
/// restores nothing.
#[repr(C)]
#[allow(dead_code)]
struct ContextWords {
    body: usize,
    callee: usize,
    packed: usize,
    caller_base: usize,
    caller_registers: usize,
    call: usize,
    capture: usize,
    env: usize,
    this: usize,
}

pub(crate) const CONTEXT_WORDS: usize = size_of::<ContextWords>() / WORD_BYTES;

/// A context field's index, counted down from register 0: the last field is word 1 and the
/// first is `CONTEXT_WORDS`.
const fn context_index(field_offset: usize) -> usize {
    CONTEXT_WORDS - field_offset / WORD_BYTES
}

pub(crate) const CONTEXT_THIS: usize = context_index(offset_of!(ContextWords, this));
pub(crate) const CONTEXT_ENV: usize = context_index(offset_of!(ContextWords, env));
pub(crate) const CONTEXT_CAPTURE: usize = context_index(offset_of!(ContextWords, capture));
pub(crate) const CONTEXT_CALL: usize = context_index(offset_of!(ContextWords, call));
pub(crate) const CONTEXT_CALLER_REGISTERS: usize = context_index(offset_of!(ContextWords, caller_registers));
pub(crate) const CONTEXT_CALLER_BASE: usize = context_index(offset_of!(ContextWords, caller_base));
pub(crate) const CONTEXT_PACKED: usize = context_index(offset_of!(ContextWords, packed));
pub(crate) const CONTEXT_CALLEE: usize = context_index(offset_of!(ContextWords, callee));
/// A body's captures word (`entry_cell::capture_word`): the dense slots' address, whether they
/// admit every direct store, and their count (`shared_words::capture_values`, `capture_stores`,
/// `capture_len` read it).
pub(crate) const CONTEXT_BODY: usize = context_index(offset_of!(ContextWords, body));
const _: () = assert!(CONTEXT_CALLEE + 1 == CONTEXT_BODY && CONTEXT_BODY == CONTEXT_WORDS);
const _: () = assert!(CONTEXT_CALLEE == 8);
const _: () = assert!(CONTEXT_THIS == 1);
const _: () = assert!(CONTEXT_ENV == 2);
const _: () = assert!(CONTEXT_CAPTURE == 3);
const _: () = assert!(CONTEXT_CALL == 4);
const _: () = assert!(CONTEXT_CALLER_REGISTERS == 5);
const _: () = assert!(CONTEXT_CALLER_BASE == 6);
const _: () = assert!(CONTEXT_PACKED == 7);
const _: () = assert!(CONTEXT_WORDS == 9);

/// The packed word: function in bits 0-31, flags from bit `FLAG_BASE`, argc from `ARGC_SHIFT`.
const FUNCTION_BITS: u32 = 32;
const FLAG_BASE: u32 = FUNCTION_BITS;
const FLAG_BITS: u32 = 16;

/// The packed word's flags, each one bit counted up from `FLAG_BASE`.
#[repr(u32)]
#[allow(dead_code)]
enum PackedFlag {
    /// A plain frame.
    Plain = FLAG_BASE,
    /// A plain frame whose callee runs a compiled body (b-jit direct calls).
    Body,
    /// A plain frame a native (u-jit) direct call wrote.
    Native,
    /// The window keeps passed values past its registers (a forwarded `arguments`, values past
    /// the parameters): the side record names the first kept register (one-frame-one-call.md F4).
    Keeps,
    /// The window carries its environment entries and frame locals (`call_environment.rs`).
    Env,
    /// The completion is not `Plain`: the side record holds it.
    Completion,
    /// The caller runs another program: the side record holds the caller's program.
    Crosses,
    /// The window lies over another register file (a generator's): side record R. Reserved;
    /// such frames stay records until step F4's heap-window cut.
    HeapWindow,
    /// The frame owns an eval record (`LaneStack::records`, one-frame-one-call.md F4 (3)): its
    /// side record names it, as a record's `ScriptActivation::record` does.
    Owns,
    /// A Wasm frame: side record W. Reserved for step F6.
    Wasm,
}

pub(crate) const PLAIN_FLAG: u64 = 1 << PackedFlag::Plain as u32;
pub(crate) const BODY_FLAG: u64 = 1 << PackedFlag::Body as u32;
pub(crate) const NATIVE_FLAG: u64 = 1 << PackedFlag::Native as u32;
/// The side-record flags (one-frame-one-call.md F4): a frame with any of them has one entry on
/// the lane stack's side records (`LaneStack::sides`), innermost last; a frame with none
/// completes plainly from its header alone.
pub(crate) const KEEPS_FLAG: u64 = 1 << PackedFlag::Keeps as u32;
pub(crate) const ENV_FLAG: u64 = 1 << PackedFlag::Env as u32;
pub(crate) const COMPLETION_FLAG: u64 = 1 << PackedFlag::Completion as u32;
pub(crate) const CROSSES_FLAG: u64 = 1 << PackedFlag::Crosses as u32;
pub(crate) const OWNS_FLAG: u64 = 1 << PackedFlag::Owns as u32;
pub(crate) const SIDE_FLAGS: u64 = KEEPS_FLAG | ENV_FLAG | COMPLETION_FLAG | CROSSES_FLAG | OWNS_FLAG;
pub(crate) const ARGC_SHIFT: u32 = FLAG_BASE + FLAG_BITS;
const _: () = assert!(PLAIN_FLAG == 1 << 32);
const _: () = assert!(BODY_FLAG == 1 << 33);
const _: () = assert!(NATIVE_FLAG == 1 << 34);
const _: () = assert!(ARGC_SHIFT == 48);
const _: () = assert!(KEEPS_FLAG == 1 << 35 && ENV_FLAG == 1 << 36 && COMPLETION_FLAG == 1 << 37 && CROSSES_FLAG == 1 << 38);
const _: () = assert!(OWNS_FLAG == 1 << 40);
const _: () = assert!((PackedFlag::Wasm as u32) < ARGC_SHIFT, "the flags fit below argc");

/// The plain activations' words (`PlainFrames`), in address order: depth, innermost window,
/// program.
#[repr(C)]
#[allow(dead_code)]
struct PlainFrameWords {
    depth: usize,
    registers: usize,
    program: usize,
}

pub(crate) const PLAIN_DEPTH_AT: usize = offset_of!(PlainFrameWords, depth);
pub(crate) const PLAIN_REGISTERS_AT: usize = offset_of!(PlainFrameWords, registers);
pub(crate) const PLAIN_PROGRAM_AT: usize = offset_of!(PlainFrameWords, program);
const _: () = assert!(PLAIN_DEPTH_AT == 0);
const _: () = assert!(PLAIN_REGISTERS_AT == 8);
const _: () = assert!(PLAIN_PROGRAM_AT == 16);

/// An activation record's size and where its kind tag lies (`Activation`). The b-jit stencil
/// crate cannot name the runtime's `Activation`, so these stay literal; `dispatch_fast.rs`
/// asserts them against `size_of::<Activation>()` and `KIND_OFFSET`.
pub(crate) const ACTIVATION_BYTES: usize = 128;
pub(crate) const ACTIVATION_KIND_OFFSET: usize = 72;
/// The tag of a native direct call's frame (`quench_bjit::shared_words::KIND_NATIVE`, asserted equal by the runtime).
pub(crate) const KIND_NATIVE: u8 = 0;

/// The Wasm stitched frame's header words, before its registers. The entry writes them and the
/// rows read them: the result word is the pointer the call's result goes to, and the linear
/// memory's base and length are what the memory rows read (the call row reloads the pins from
/// them). `quench-bjit`'s stencil crate and lib mount this file, so both read these constants.
#[repr(C)]
#[allow(dead_code)]
pub(crate) struct WasmFrameHeader {
    /// The result pointer, the first header word.
    result: usize,
    /// The linear memory's base address.
    base: usize,
    /// The linear memory's length.
    length: usize,
}

/// The result pointer's word index in the Wasm frame header.
pub(crate) const WASM_RESULT_WORD: usize = offset_of!(WasmFrameHeader, result) / WORD_BYTES;
/// The linear memory base's word index in the Wasm frame header.
pub(crate) const WASM_BASE_WORD: usize = offset_of!(WasmFrameHeader, base) / WORD_BYTES;
/// The linear memory length's word index in the Wasm frame header.
pub(crate) const WASM_LENGTH_WORD: usize = offset_of!(WasmFrameHeader, length) / WORD_BYTES;
/// The Wasm frame header's size in words: the first register's word index, one past the header.
pub(crate) const WASM_HEADER_WORDS: usize = size_of::<WasmFrameHeader>() / WORD_BYTES;
const _: () = assert!(WASM_RESULT_WORD == 0);
const _: () = assert!(WASM_BASE_WORD == 1);
const _: () = assert!(WASM_LENGTH_WORD == 2);
const _: () = assert!(WASM_HEADER_WORDS == 3);
