//! `internalBinding('crypto')` as a host operation, apart from the module install the bootstrap
//! always links: an image whose program reaches no crypto operation links no TLS or OpenSSL.
crate::host::shared_vm::binding_operation!(binding_operation, super::super::tls_shared_vm::facade::binding);

/// `globalThis.crypto`'s installation as a host operation: the bootstrap links it only where
/// the program reaches crypto, so an image with no crypto operation links no OpenSSL.
pub(crate) fn install_global_operation(c: &mut quench_runtime::NativeContext<'_>, _: quench_runtime::RootId, _: &[quench_runtime::RootId]) -> Result<quench_runtime::RootId, quench_runtime::RootedError> {
    super::install_global(c)?;
    Ok(c.undefined())
}
