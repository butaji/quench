//! Polyfill: `externalizable-strings`

pub const JS: &str = quench_js_check::checked_js!(r#"globalThis.createExternalizableString = (value) => String(value);
globalThis.createExternalizableTwoByteString = (value) => String(value);
globalThis.externalizeString = () => undefined;
globalThis.isOneByteString = (value) => /^[\x00-\xff]*$/.test(String(value));
"#);
