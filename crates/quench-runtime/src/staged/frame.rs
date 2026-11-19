//! The one frame's layout: the words of a call in progress that emitted code reads and
//! writes at fixed offsets. Everything else (return linkage, kind) belongs to the lane, which
//! asserts these against the record it lays out (`vm::compiled::ShadowFrame`).

/// The callee's descriptor (`DirectCallee`), null for a frame the lane made.
pub(crate) const CALLEE: usize = 0;
/// The callee's receiver, as value bits.
pub(crate) const THIS: usize = 8;
/// The published body (`LaneMethod`) the frame runs, 0 when none.
pub(crate) const BODY: usize = 16;
/// The address of the frame's first register; its registers, then what it keeps, follow.
pub(crate) const WINDOW: usize = 24;
/// The prefix every frame starts with.
pub(crate) const PREFIX_BYTES: usize = 32;

/// The context words below every window's register 0 (call protocol v2): the runtime's
/// `register_file::FRAME_CONTEXT` and the b-jit's `FRAME_CONTEXT_WORDS` are this.
pub(crate) const CONTEXT_WORDS: usize = 9;
/// Each context word, counted down from register 0: the receiver, environment, captures,
/// then a plain activation's header (the caller's call record, registers and records, the
/// packed word, the callee), then a body's captures word. A body's receiver and captures
/// are written into its own window by whoever enters it, so each frame carries its own
/// and a call saves and restores nothing.
pub(crate) const CONTEXT_THIS: usize = 1;
pub(crate) const CONTEXT_ENV: usize = 2;
pub(crate) const CONTEXT_CAPTURE: usize = 3;
pub(crate) const CONTEXT_CALL: usize = 4;
pub(crate) const CONTEXT_CALLER_REGISTERS: usize = 5;
pub(crate) const CONTEXT_CALLER_BASE: usize = 6;
pub(crate) const CONTEXT_PACKED: usize = 7;
pub(crate) const CONTEXT_CALLEE: usize = 8;
/// A body's captures word (`entry_cell::capture_word`): the dense slots' address, whether they
/// admit every direct store, and their count (`shared_words::capture_values`, `capture_stores`,
/// `capture_len` read it).
pub(crate) const CONTEXT_BODY: usize = 9;
const _: () = assert!(CONTEXT_CALLEE + 1 == CONTEXT_BODY && CONTEXT_BODY == CONTEXT_WORDS);
/// The packed word: function in bits 0-31, flags in 32-47, argc from `ARGC_SHIFT`.
pub(crate) const PLAIN_FLAG: u64 = 1 << 32;
/// A plain frame whose callee runs a compiled body (b-jit direct calls).
pub(crate) const BODY_FLAG: u64 = 1 << 33;
/// A plain frame a native (u-jit) direct call wrote.
pub(crate) const NATIVE_FLAG: u64 = 1 << 34;
pub(crate) const ARGC_SHIFT: u32 = 48;
/// The plain activations' words (`PlainFrames`): depth, innermost window, program.
pub(crate) const PLAIN_DEPTH_AT: usize = 0;
pub(crate) const PLAIN_REGISTERS_AT: usize = 8;
pub(crate) const PLAIN_PROGRAM_AT: usize = 16;
/// An activation record's size and where its kind tag lies (`Activation`).
pub(crate) const ACTIVATION_BYTES: usize = 128;
pub(crate) const ACTIVATION_KIND_OFFSET: usize = 72;
/// The tag of a native direct call's frame (`quench_bjit::shared_words::KIND_NATIVE`, asserted equal by the runtime).
pub(crate) const KIND_NATIVE: u8 = 0;
