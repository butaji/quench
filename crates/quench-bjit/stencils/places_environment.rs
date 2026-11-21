//! The environment, capture and global-name frame methods of a stencil frame (`Places`): the
//! payload's capture runs, the environment window's storage and the global object's slot, which
//! the staged rows (`staged/{captures,names,global}.rs`) ask the frame for. Each row's operand
//! holes are consumed in the order the stage's `site` calls name them, as the hand rows did.

use crate::heap_rows::{context_word, payload, word_at};
use crate::layout::shared_words::{capture_len, capture_stores, capture_values};
use crate::layout::{CONTEXT_BODY, JS_STEP_ENV_STORAGE, JS_STEP_OBJECT_RECORD_BASE};
use crate::typed_ir::Representation;
use crate::places::{Places};
use crate::stage::StencilStage;
use crate::staged::object::{self, Layout, Slots};
use crate::staged::region::Region;
use crate::staged::{environment, Frame, GlobalFrame, Stage};
use crate::typed_ir::Type;
use crate::words::{hole, HOLE_A};
use crate::bytecode::Register;

type Concrete = StencilStage;

/// The values of the global object's shape a row's layout proof counts: the one slot the row names,
/// which the stage's per-site byte offset selects. The shape itself is held by a dependency (the
/// global layout fact), so the row proves only the record's representation.
const GLOBAL_SLOT_COUNT: usize = 1;

/// The layout of the global object `id` names in the record table at the payload: `None` where the
/// table is absent or the object's values are not behind its extras in the arena.
#[inline(always)]
fn global_layout(stage: &mut Concrete, payload: usize, id: u64) -> Option<Layout<u64, bool>> {
    let records = word_at(payload, JS_STEP_OBJECT_RECORD_BASE) as u64;
    if records == 0 {
        return None;
    }
    let record = object::object_record_at(stage, records, id);
    let inline = object::inline_storage(stage, record);
    object::arena_extras_layout(stage, record, inline, GLOBAL_SLOT_COUNT, ())
}

impl Places {
    /// `Frame::capture_slot`: the depth-0 dense run's slot, from the window's capture word (its values and
    /// length); a root read and deeper scopes are the interpreter's.
    #[inline(always)]
    pub(crate) fn capture_read(&mut self, stage: &mut Concrete, depth: u16, slot: u16, root: bool) -> Option<(u64, Representation)> {
        if depth != 0 || root {
            return None;
        }
        let captures = context_word(self.s.frame, CONTEXT_BODY);
        environment::dense_capture(stage, capture_values(captures), capture_len(captures), slot, ()).map(|word| (word, Representation::Tagged))
    }

    /// `Frame::store_capture`: the direct store into the depth-0 dense run's slot, where the run's
    /// stores are admitted (the window's capture word's stores bit).
    #[inline(always)]
    pub(crate) fn capture_store(&mut self, stage: &mut Concrete, depth: u16, slot: u16, value: u64) -> Option<()> {
        if depth != 0 {
            return None;
        }
        let captures = context_word(self.s.frame, CONTEXT_BODY);
        let slots = (capture_values(captures), capture_len(captures), capture_stores(captures));
        environment::dense_capture_store(stage, slots, slot, value, ())
    }

    /// `Frame::environment`: the environment window's storage word the payload holds, whose slots
    /// are read as tagged words.
    #[inline(always)]
    pub(crate) fn environment_storage(&mut self) -> Option<(u64, Representation)> {
        Some((word_at(payload(&self.s), JS_STEP_ENV_STORAGE), Representation::Tagged))
    }

    /// `Frame::environment_write`: the store of `value` to environment slot `slot` of the window's
    /// storage, where the slot is initialized (`environment::environment_slot_store`), or defined as
    /// the binding's first value with `initialize` (`environment::environment_slot_define`).
    #[inline(always)]
    pub(crate) fn environment_store(&mut self, stage: &mut Concrete, slot: u16, value: u64, initialize: bool) -> Option<()> {
        let storage = word_at(payload(&self.s), JS_STEP_ENV_STORAGE);
        if initialize {
            environment::environment_slot_define(stage, storage, slot, value, ())
        } else {
            environment::environment_slot_store(stage, storage, slot, value, ())
        }
    }
}

impl GlobalFrame<Concrete> for Places {
    /// The global object's slot (the id is hole 1, the byte offset the stage's next site) read into
    /// `destination`, as the lane's global read.
    #[inline(always)]
    fn global_read(&mut self, stage: &mut Concrete, destination: Register, _atom: u32, _site: u16, _ty: Type, _proof: ()) -> Option<()> {
        let layout = global_layout(stage, payload(&self.s), hole::<HOLE_A>() as u64)?;
        let word = object::read_slot(stage, layout, 0, Representation::Tagged);
        Frame::write(self, stage, destination, word, Representation::Tagged);
        Some(())
    }

    /// The store of `value` to the global object's slot (the record id is hole 0, the byte offset the
    /// stage's next site). The caller passes the store gate (`store_gate`) where the value may be a
    /// heap reference.
    #[inline(always)]
    fn global_write(&mut self, stage: &mut Concrete, _source: Register, _atom: u32, _site: u16, value: u64, _initialize: bool) -> Option<()> {
        let layout = global_layout(stage, payload(&self.s), hole::<0>() as u64)?;
        let Slots::Flat(base) = layout.slots else { return None };
        let offset = stage.site(0);
        let address = stage.add_imm(base, offset);
        stage.store_u64(Region::ObjectSlots.mutable(), address, 0, value);
        Some(())
    }
}
