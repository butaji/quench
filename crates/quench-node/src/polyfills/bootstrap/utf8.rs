//! Polyfill: `utf8`

pub const JS: &str = r#"const __quenchUtf8Width = (first) =>
  first < 0xc2 || first > 0xf4 ? 1 : first < 0xe0 ? 2 : first < 0xf0 ? 3 : 4;
const __quenchUtf8ContinuationCode = (bytes, index, width) => {
  let code = bytes[index] & (width === 2 ? 0x1f : width === 3 ? 0xf : 0x7);
  for (let offset = 1; offset < width; offset++) {
    const byte = bytes[index + offset];
    if (byte < 0x80 || byte > 0xbf) return undefined;
    code = (code << 6) | (byte & 0x3f);
  }
  return code;
};
const __quenchUtf8ValidCode = (code, width) =>
  !(
    (width === 2 && code < 0x80) ||
    (width === 3 && code < 0x800) ||
    (code >= 0xd800 && code <= 0xdfff) ||
    code > 0x10ffff
  );
const __quenchUtf8CodePoint = (bytes, index, width) => {
  const code = __quenchUtf8ContinuationCode(bytes, index, width);
  if (code === undefined || !__quenchUtf8ValidCode(code, width)) {
    return undefined;
  }
  return code;
};
const __quenchUtf8InvalidAdvance = (bytes, index, width) => {
  if (__quenchUtf8ContinuationCode(bytes, index, width) !== undefined) return 1;
  let next = index + 1;
  while (next < index + width && bytes[next] >= 0x80 && bytes[next] <= 0xbf) {
    next++;
  }
  return next - index;
};
const __quenchUtf8IncompleteAdvance = (bytes, index) => {
  let next = index + 1;
  while (next < bytes.length && bytes[next] >= 0x80 && bytes[next] <= 0xbf) {
    next++;
  }
  return next < bytes.length ? next : undefined;
};
const __quenchDecodeUtf8 = (bytes, final) => {
  let output = "";
  let index = 0;
  while (index < bytes.length) {
    const width = __quenchUtf8Width(bytes[index]);
    if (index + width > bytes.length && !final) {
      const invalid = __quenchUtf8IncompleteAdvance(bytes, index);
      if (invalid === undefined) break;
      output += "\ufffd";
      index = invalid;
      continue;
    }
    if (index + width > bytes.length) {
      output += "\ufffd";
      break;
    }
    if (width === 1) {
      output += bytes[index] < 0x80
        ? String.fromCharCode(bytes[index])
        : "\ufffd";
      index++;
      continue;
    }
    const code = __quenchUtf8CodePoint(bytes, index, width);
    if (code === undefined) {
      output += "\ufffd";
      index += __quenchUtf8InvalidAdvance(bytes, index, width);
    } else {
      output += String.fromCodePoint(code);
      index += width;
    }
  }
  return { text: output, pending: bytes.slice(index) };
};
"#;
