//! The auxiliary bit of a constant-marked object. Its own file, so the stencil build includes it
//! unchanged (`quench-bjit/stencils/facade.rs`): the staged rows test the mark as the VM does.

/// An object whose values code holds as constants. No code sets it now: the own-constant hints
/// that marked an object (`Heap::mark_own_constant`) are gone, so the bit stays clear and the
/// clears on the write path (`Object::note_constant_write`) are no-ops. Where it was set, the
/// first write to any value or any change of the object's layout or storage cleared it and retired
/// that code. It is part of the raw auxiliary word and so of the layout
/// id, so every guard that compares the raw word (a lane hint, a generated row's recorded
/// shape, `formed_plain_start`) misses a marked object and leaves to the VM, whose stores clear
/// it; the Rust-side shapes (`shape`, `exact_auxiliary`, `ordinary_shape`) leave it out, so a
/// marked object keeps its shape tables and cache entries. Unlike extras it is cleared once:
/// a written object returns to every fast store.
pub const CONSTANT_FORM: u32 = 1 << 25;
