//! The environment, capture and global-name frame methods of a stencil frame (`Places`): the
//! payload's capture runs, the environment window's storage and the global object's slot, which
//! the staged rows (`staged/{captures,names,global}.rs`) ask the frame for. Each row's operand
//! holes are consumed in the order the stage's `site` calls name them, as the hand rows did.

use crate::heap_rows::{context_word, payload, word_at};
use crate::layout::shared_words::{capture_len, capture_stores, capture_values};
use crate::layout::{CONTEXT_BODY, JS_STEP_ENV_STORAGE};
use crate::typed_ir::Representation;
use crate::places::{Places};
use crate::stage::StencilStage;
use crate::staged::environment;

type Concrete = StencilStage;

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
