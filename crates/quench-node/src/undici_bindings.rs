/// The name undici's module wrapper binds `byteStringEnd` to: the patched bundle calls it
/// (`node_source_patches.rs`) and the loader passes it (`modules::webidl_shared_vm`).
macro_rules! byte_string_end_binding {
    () => {
        "__quenchByteStringEnd"
    };
}
