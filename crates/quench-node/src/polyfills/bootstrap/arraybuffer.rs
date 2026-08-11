//! Polyfill: `arraybuffer`

pub const JS: &str = quench_js_check::checked_js!(r#"const __nodeBufferIsArrayBuffer = (value) => {
  try {
    Object.getOwnPropertyDescriptor(
      ArrayBuffer.prototype,
      "byteLength",
    ).get.call(value);
    return true;
  } catch {
    return false;
  }
};
"#);
