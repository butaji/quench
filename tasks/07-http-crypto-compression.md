# Stage 07 — HTTP, TLS, crypto, and compression

Complete HTTP client/server agents, options, pooling, aborts, trailers, HTTP/2 protocol semantics, HTTPS/TLS transport and secure servers, then crypto hashes/HMAC/ciphers/WebCrypto/key validation and async zlib. Preserve security errors and byte-level behavior; do not delegate HTTPS to plain HTTP once TLS is required.

Run upstream http/https/http2/tls/crypto/zlib fixtures, WPT fetch/WebCrypto/compression supplements, and focused stages 2208–2352, 2551–2564, 2590, 2602, 2612. Acceptance: protocol lifecycle, certificates/options, algorithm validation, streaming, async ordering, and failure codes match Node.
