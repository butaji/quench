const __quenchDecodeUtf16 = (bytes, final) => {
  const complete = bytes.length - (bytes.length % 2);
  let text = "";
  let index = 0;
  for (; index < complete; index += 2) {
    const code = bytes[index] | (bytes[index + 1] << 8);
    if (code >= 0xd800 && code <= 0xdbff && index + 2 === complete && !final) {
      break;
    }
    text += String.fromCharCode(code);
  }
  const pending = final ? [] : bytes.slice(index);
  return { text, pending };
};
