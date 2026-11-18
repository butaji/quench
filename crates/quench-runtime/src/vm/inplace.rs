//! C-shaped bodies of builtins that complete in place (no guest code, no raised error): each
//! reads its receiver's record once and does what the C equivalent does, or answers `None`
//! and leaves the call to the general native. Tried from `Vm::in_place_native_value`.
mod array_build;
mod ascii_text;
