//! Polyfill: `punycode`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithPunycode = globalThis.require;
const __pcBase = 36,
  __pcTMin = 1,
  __pcTMax = 26,
  __pcSkew = 38,
  __pcDamp = 700,
  __pcInitialBias = 72,
  __pcInitialN = 128;
const __pcDigit = (d) =>
  d < 26 ? String.fromCharCode(97 + d) : String.fromCharCode(48 + d - 26);
const __pcValue = (c) => (c >= 97 ? c - 97 : c - 22);
const __pcAdapt = (delta, num, first) => {
  delta = first ? Math.floor(delta / __pcDamp) : delta >> 1;
  delta += Math.floor(delta / num);
  let k = 0;
  while (delta > ((__pcBase - __pcTMin) * __pcTMax) >> 1) {
    delta = Math.floor(delta / (__pcBase - __pcTMin));
    k += __pcBase;
  }
  return (
    k + Math.floor(((__pcBase - __pcTMin + 1) * delta) / (delta + __pcSkew))
  );
};
const __pcEncodeValue = (delta, h, b, out, bias) => {
  let q = delta;
  for (let k = __pcBase;; k += __pcBase) {
    const t = k <= bias ? __pcTMin : k >= bias + __pcTMax ? __pcTMax : k - bias;
    if (q < t) break;
    out.push(__pcDigit(t + ((q - t) % (__pcBase - t))));
    q = Math.floor((q - t) / (__pcBase - t));
  }
  out.push(__pcDigit(q));
  return __pcAdapt(delta, h + 1, h === b);
};
const __pcEncode = (input) => {
  const code = Array.from(input, (c) => c.codePointAt(0));
  let n = __pcInitialN,
    delta = 0,
    bias = __pcInitialBias,
    out = code.filter((c) => c < 128).map((c) => String.fromCharCode(c));
  let h = out.length,
    b = h;
  if (b) out.push("-");
  while (h < code.length) {
    let m = Infinity;
    for (const c of code) if (c >= n && c < m) m = c;
    delta += (m - n) * (h + 1);
    n = m;
    for (const c of code) {
      if (c < n) delta++;
      if (c === n) {
        bias = __pcEncodeValue(delta, h, b, out, bias);
        delta = 0;
        h++;
      }
    }
    delta++;
    n++;
  }
  return out.join("");
};
const __pcDecode = (input) => {
  let n = 128,
    i = 0,
    bias = 72,
    out = [];
  const dash = input.lastIndexOf("-");
  for (let j = 0; j < (dash < 0 ? 0 : dash); j++) out.push(input.charCodeAt(j));
  let index = dash < 0 ? 0 : dash + 1;
  while (index < input.length) {
    let old = i,
      w = 1;
    for (let k = 36;; k += 36) {
      const digit = __pcValue(input.charCodeAt(index++));
      i += digit * w;
      const t = k <= bias ? 1 : k >= bias + 26 ? 26 : k - bias;
      if (digit < t) break;
      w *= 36 - t;
    }
    const len = out.length + 1;
    bias = __pcAdapt(i - old, len, old === 0);
    n += Math.floor(i / len);
    i %= len;
    out.splice(i++, 0, n);
  }
  return String.fromCodePoint(...out);
};
const __quenchPunycode = {
  version: "2.1.0",
  toASCII: (value) =>
    String(value)
      .split(".")
      .map((label) =>
        label.split("").every((c) => c.charCodeAt(0) < 128)
          ? label
          : "xn--" + __pcEncode(label)
      )
      .join("."),
  toUnicode: (value) =>
    String(value)
      .split(".")
      .map((label) =>
        label.startsWith("xn--") ? __pcDecode(label.slice(4)) : label
      )
      .join("."),
  ucs2: {
    decode: (value) => Array.from(value, (c) => c.codePointAt(0)),
    encode: (value) => String.fromCodePoint(...value),
  },
  encode: __pcEncode,
  decode: __pcDecode,
};
globalThis.require = (specifier) =>
  String(specifier).replace(/^node:/, "") === "punycode"
    ? __quenchPunycode
    : __quenchOriginalRequireWithPunycode(specifier);
"#);
