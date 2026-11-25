//! A direct call's native frame as one row every tier runs (one-frame-one-call.md F5): the push and the pop
//! the b-jit's call row and the u-jit's lane frame share, over the lane's state words.

use super::protocol::{push_body_header, set_plain, BodyHeader, LaneTops};
use super::layout_offsets::{JS_STEP_BODY_TOP, JS_STEP_LANE_VALUES_TOP, JS_STEP_PLAIN_DEPTH};
use super::region::Region;
use super::{word_bytes, Stage};

/// Where the lane's state words are, as a row reads them: words read once (`LaneTops`, the b-jit's admission),
/// or reads of the dispatch payload at each use (`PayloadTops`, the u-jit's), so a specializer keeps no
/// address live across a call.
pub trait LaneWords<S: Stage> {
    fn values_at(&self, stage: &mut S) -> S::Word;
    fn plain_at(&self, stage: &mut S) -> S::Word;
    fn body_top_at(&self, stage: &mut S) -> S::Word;
}

impl<S: Stage> LaneWords<S> for LaneTops<S::Word> {
    #[inline(always)]
    fn values_at(&self, _stage: &mut S) -> S::Word { self.values_at }
    #[inline(always)]
    fn plain_at(&self, _stage: &mut S) -> S::Word { self.plain_at }
    #[inline(always)]
    fn body_top_at(&self, _stage: &mut S) -> S::Word { self.body_top_at }
}

/// The lane's state words read from the dispatch `payload` where each is used.
#[derive(Clone, Copy)]
pub struct PayloadTops<W> {
    pub payload: W,
}

impl<S: Stage> LaneWords<S> for PayloadTops<S::Word> {
    #[inline(always)]
    fn values_at(&self, stage: &mut S) -> S::Word { stage.load_u64(Region::Dispatch.mutable(), self.payload, JS_STEP_LANE_VALUES_TOP as i32) }
    #[inline(always)]
    fn plain_at(&self, stage: &mut S) -> S::Word { stage.load_u64(Region::Dispatch.mutable(), self.payload, JS_STEP_PLAIN_DEPTH as i32) }
    #[inline(always)]
    fn body_top_at(&self, stage: &mut S) -> S::Word { stage.load_u64(Region::Dispatch.mutable(), self.payload, JS_STEP_BODY_TOP as i32) }
}

/// Moves the values' top to `values`, through the word `tops` addresses.
#[inline(always)]
pub fn set_values<S: Stage>(stage: &mut S, tops: &impl LaneWords<S>, values: S::Word) {
    let at = tops.values_at(stage);
    stage.store_u64(Region::Dispatch.mutable(), at, 0, values);
}

/// The body table's top word moves to `depth`: one entry per plain depth (the running body of each frame).
#[inline(always)]
pub fn set_body_top<S: Stage>(stage: &mut S, tops: &impl LaneWords<S>, depth: S::Word) {
    let at = tops.body_top_at(stage);
    stage.store_u64(Region::Dispatch.mutable(), at, 0, depth);
}

/// A direct call's native frame, as every tier pushes it: the callee's header below its `window`, its body and
/// descriptor at `index` (the frame count before the push) in the lane's body and descriptor tables, the plain
/// frames one deeper with `window` innermost, and the values' top at `next`. The receiver, captures, environment
/// and capture words are the caller's.
#[derive(Clone, Copy)]
pub struct NativeFrame<W> {
    pub window: W,
    pub header: BodyHeader<W>,
    pub body: W,
    pub descriptor: W,
    pub index: W,
    pub next: W,
}

/// Where a native frame's push writes past the lane's state words: the body and descriptor tables, and the plain
/// depth word's address and the depth before the push.
#[derive(Clone, Copy)]
pub struct NativePlaces<W> {
    pub bodies: W,
    pub descriptors: W,
    pub plain_at: W,
    pub depth: W,
}

/// Pushes `frame`: the one native-frame push of the b-jit's call row and the u-jit's lane frame. The header goes
/// first; `places` answers where the rest goes, which a specializer loads only then, so its addresses are not
/// live across the header's stores.
#[inline(always)]
pub fn push_native_frame<S: Stage>(stage: &mut S, tops: &impl LaneWords<S>, frame: NativeFrame<S::Word>,
    places: impl FnOnce(&mut S) -> NativePlaces<S::Word>) {
    push_body_header(stage, frame.window, frame.header);
    let NativePlaces { bodies, descriptors, plain_at, depth } = places(stage);
    let offset = stage.mul_imm(frame.index, word_bytes(1));
    let body_at = stage.add(bodies, offset);
    stage.store_u64(Region::Frame.mutable(), body_at, 0, frame.body);
    let descriptor_at = stage.add(descriptors, offset);
    stage.store_u64(Region::Frame.mutable(), descriptor_at, 0, frame.descriptor);
    let depth = stage.add_imm(depth, 1_u64);
    set_plain(stage, plain_at, depth, frame.window);
    let top = stage.add_imm(frame.index, 1_u64);
    set_body_top(stage, tops, top);
    set_values(stage, tops, frame.next);
}

/// Pops the native frame a call pushed at `window` and `index`: where its header is still plain and NATIVE (the
/// plain depth is not 0, then the packed word keeps `frame::NATIVE_FLAG`), the plain frames return to `restore`
/// (the depth and innermost window, `None` for one less than the depth and the header's caller registers); else
/// Rust observed the frame (`LaneStack::spill_plain`, `native_frames::link_header`) and `observed`, the cold path,
/// pops it where Rust keeps it (`LaneStack::pop_observed_header`). The body top returns to `index` and the values'
/// top to `window`.
#[inline(always)]
pub fn pop_native_frame<S: Stage>(stage: &mut S, tops: &impl LaneWords<S>, window: S::Word, index: S::Word,
    restore: Option<(S::Word, S::Word)>, observed: impl FnOnce(&mut S)) {
    use super::frame::{CONTEXT_CALLER_REGISTERS, CONTEXT_PACKED, NATIVE_FLAG, PLAIN_DEPTH_AT};
    let at = |index: usize| -((index * core::mem::size_of::<u64>()) as i32);
    let plain_at = tops.plain_at(stage);
    let depth = stage.load_u64(Region::Payload.mutable(), plain_at, PLAIN_DEPTH_AT as i32);
    let plain = stage.ne_imm(depth, 0_u64);
    stage.effect_if_both(plain, #[inline(always)] |stage| {
        let packed = stage.load_u64(Region::Frame.mutable(), window, at(CONTEXT_PACKED));
        let native = stage.and_imm(packed, NATIVE_FLAG);
        stage.ne_imm(native, 0_u64)
    }, #[inline(always)] |stage| {
        let (depth, registers) = match restore {
            Some(restore) => restore,
            None => {
                let registers = stage.load_u64(Region::Frame.mutable(), window, at(CONTEXT_CALLER_REGISTERS));
                (stage.add_imm(depth, u64::MAX), registers)
            }
        };
        set_plain(stage, plain_at, depth, registers);
    }, observed);
    set_body_top(stage, tops, index);
    set_values(stage, tops, window);
}
