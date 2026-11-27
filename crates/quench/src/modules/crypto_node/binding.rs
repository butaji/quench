//! `globalThis.crypto`'s installation as a host operation: the bootstrap defines the global
//! lazily, so an image whose program never reads `crypto` links no crypto operation and no
//! OpenSSL.

/// Installs `globalThis.crypto` (`crypto_node::install_global`) on first read.
pub(crate) fn install_global_operation(c: &mut quench_runtime::NativeContext<'_>, _: quench_runtime::RootId, _: &[quench_runtime::RootId]) -> Result<quench_runtime::RootId, quench_runtime::RootedError> {
    super::install_global(c)?;
    Ok(c.undefined())
}
