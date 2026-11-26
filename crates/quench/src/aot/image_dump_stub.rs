//! The image dump's non-Linux form: an image dump reads and maps ELF state at fixed addresses
//! (`image_dump`), which only a Linux image has. Every entry here is the shape `image_dump`
//! has on Linux, so the staging code is one program on both.

/// The environment variable a dumping run reads (`image_dump::DUMP_VARIABLE`).
pub(crate) const DUMP_VARIABLE: &str = "QUENCH_IMAGE_DUMP";

/// A dump's pages (`image_dump::Dump`); never made here.
#[derive(Default)]
pub(crate) struct Dump {
    pub(crate) runs: Vec<(u64, Vec<u8>, bool)>,
}

/// The saved run this process dumps: none outside Linux.
pub(crate) fn requested_wait() -> Option<usize> {
    None
}

/// Starts the dump: nothing outside Linux.
pub fn begin() {}

/// Ends the dump: never reached outside Linux (`requested_wait` names none).
pub(crate) fn write(_: usize, _: *mut u8) -> ! {
    panic!("image dumps are a Linux feature")
}

/// The dump `finish` makes: none outside Linux.
pub(crate) fn finish(
    _: &[u8], _: &[u8], _: &[(u64, u64)], _: &[(u64, u64)], _: &dyn Fn(u64, usize) -> Vec<u8>, _: &dyn Fn(u64, usize) -> Vec<u8>,
) -> Result<Dump, String> {
    Err("image dumps are a Linux feature".into())
}

/// Appends the dumps to an image: nothing outside Linux.
pub(crate) fn append(_: &std::path::Path, _: &[Dump]) -> Result<(), String> {
    Ok(())
}

/// The saved run's runtime, mapped from a dump: none outside Linux.
pub(crate) fn adopt(_: usize) -> Option<*mut u8> {
    None
}
