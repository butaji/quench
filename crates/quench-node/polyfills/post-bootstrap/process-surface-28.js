{
  if (globalThis.process) {
    const features = (globalThis.process.features ||= {});
    features.cached_builtins ??= true;
    features.debug ??= false;
    features.ipv6 ??= true;
    features.openssl_is_boringssl ??= false;
    features.quic ??= false;
    features.require_module ??= true;
    features.tls ??= true;
    features.tls_alpn ??= true;
    features.tls_ocsp ??= true;
    features.tls_sni ??= true;
    features.typescript ??= "strip";
    features.uv ??= true;
  }
}
