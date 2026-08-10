//! Polyfill: `externalizable-strings`

pub const JS: &str = r#"globalThis.createExternalizableString = (value) => String(value);
globalThis.createExternalizableTwoByteString = (value) => String(value);
globalThis.externalizeString = () => undefined;
globalThis.isOneByteString = (value) => /^[\x00-\xff]*$/.test(String(value));
"#;
