//! Polyfill: `crypto-tail`

pub const JS: &str = quench_js_check::checked_js!(r#"const __createNodeCrypto = () => __nodeCryptoApi;
let __nodeCryptoInstance;
globalThis.__nodeCrypto = new Proxy(
  {},
  {
    get: (_, key) => {
      __nodeCryptoInstance ||= __createNodeCrypto();
      return (
        __nodeCryptoInstance[key] ||
        (key === "pseudoRandomBytes" && globalThis.__nodeCryptoRandomBytes)
      );
    },
    ownKeys: () =>
      Reflect.ownKeys((__nodeCryptoInstance ||= __createNodeCrypto())),
    getOwnPropertyDescriptor: (_, key) => ({
      enumerable: !["pseudoRandomBytes", "prng", "rng"].includes(key),
      configurable: true,
      value: (__nodeCryptoInstance ||= __createNodeCrypto())[key]
    })
  }
);
"#);
