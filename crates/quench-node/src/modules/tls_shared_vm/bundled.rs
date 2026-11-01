//! The public bundled trust list and SSL contexts share immutable PEM product data.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

pub(crate) fn install(c: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    let data = c.null_object_rooted()?;
    let getter =
        c.host_constructor_with_data(crate::host::shared_vm::operation("tlsBundledRoots"), data)?;
    let key = c.string_rooted("name");
    let name = c.string_rooted("cacheBundledRootCertificates");
    c.define_data_property_rooted(getter, key, name, false, false, true)?;
    let descriptor = c.null_object_rooted()?;
    let key = c.string_rooted("get");
    c.define_data_property_rooted(descriptor, key, getter, true, true, true)?;
    let key = c.string_rooted("enumerable");
    let yes = c.boolean(true);
    c.define_data_property_rooted(descriptor, key, yes, true, true, true)?;
    let key = c.string_rooted("rootCertificates");
    c.define_property_descriptor_rooted(module, key, descriptor)?;
    Ok(())
}
pub(crate) fn bundled(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let key = c.string_rooted("bundled");
    let descriptor = c.own_property_descriptor_rooted(data, key)?;
    if c.is_object_rooted(descriptor)? {
        let value = c.string_rooted("value");
        return c.get_property_rooted(descriptor, value);
    }
    let mut certificates = Vec::new();
    for pem in include_str!("bundled-roots.pem").split_inclusive("-----END CERTIFICATE-----") {
        let pem = pem.trim();
        if !pem.is_empty() {
            certificates.push(c.string_rooted(pem));
        }
    }
    let array = c.array_rooted(&certificates)?;
    // This is a fresh ordinary array. Freeze its actual own descriptors without
    // consulting mutable guest Object/Array constructors or prototype methods.
    for (index, certificate) in certificates.iter().enumerate() {
        let index = c.string_rooted(&index.to_string());
        c.define_data_property_rooted(array, index, *certificate, false, true, false)?;
    }
    let length = c.number(certificates.len() as f64);
    let length_key = c.string_rooted("length");
    c.define_data_property_rooted(array, length_key, length, false, false, false)?;
    c.prevent_extensions_rooted(array)?;
    c.define_data_property_rooted(data, key, array, false, false, false)?;
    Ok(array)
}
