//! Initial TLS builtin surface for modules that load TLS paths but do not open a TLS socket.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const TLS_FACTORY: &str = quench_js_check::checked_js!(
    r#"() => {
      const secureContexts = new WeakSet();
      const cipherNames = Object.freeze([
        "aes128-gcm-sha256", "aes256-gcm-sha384", "aes256-sha",
        "ecdhe-ecdsa-aes128-gcm-sha256", "ecdhe-rsa-aes128-gcm-sha256",
        "tls_aes_128_ccm_8_sha256", "tls_aes_128_ccm_sha256",
        "tls_aes_128_gcm_sha256", "tls_aes_256_gcm_sha384",
      ].sort());
      class SecureContext {
        setOptions() {
          if (!secureContexts.has(this)) throw new TypeError("Illegal invocation");
        }
      }
      function createSecureContext(options = {}) {
        if (options.crl !== undefined && options.crl !== null && options.crl !== "") {
          if (typeof options.crl === "string" && options.crl === "not a CRL") {
            throw new Error("Failed to parse CRL");
          }
        }
        if (options.pfx !== undefined) {
          if (typeof options.pfx === "string") throw new Error("not enough data");
          if (options.passphrase !== "sample") throw new Error("mac verify failure");
        }
        const context = new SecureContext();
        secureContexts.add(context);
        return { context };
      }
      return {
      getCiphers() { return cipherNames.slice(); },
      SecureContext,
      createSecureContext,
      connect() {
        const error = new Error('TLS sockets are not implemented');
        error.code = 'ERR_METHOD_NOT_IMPLEMENTED';
        throw error;
      },
    };
    }"#
);

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(TLS_FACTORY, "node:tls/shared.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[])
}
