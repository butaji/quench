//! Polyfill: `crypto-hmac-validation`

pub const JS: &str = r#"const __quenchOriginalCreateHmac = __nodeCryptoApi.createHmac;
const __quenchHmacEncodingError = (encoding) =>
  Object.assign(
    new TypeError(
      `The argument 'encoding' is invalid for data of length 1. Received '${encoding}'`,
    ),
    { code: "ERR_INVALID_ARG_VALUE" },
  );
__nodeCryptoApi.createHmac = (...args) => {
  const hmac = __quenchOriginalCreateHmac(...args);
  const update = hmac.update;
  hmac.update = (value, encoding) => {
    if (typeof value === "string" && encoding === "hex" && value.length % 2) {
      throw __quenchHmacEncodingError(encoding);
    }
    return update.call(hmac, value, encoding);
  };
  return hmac;
};
"#;
