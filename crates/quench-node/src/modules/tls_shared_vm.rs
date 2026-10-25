//! Native TLS configuration, discovery, and certificate name authorization.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) mod bundled;
pub(crate) mod certificate;
pub(crate) mod configuration;
pub(crate) mod contexts;
pub(crate) mod discovery;
pub(crate) mod facade;
pub(crate) mod handle;
pub(crate) mod identity;
pub(crate) mod server;
pub(crate) mod sessions;
pub(crate) mod socket;

/// Build the legacy TLS internal-module facade and emit Node's deprecation
/// warning when the module is first loaded through CommonJS.
pub(crate) fn deprecated_internal_module(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
    module_name: &str,
) -> Result<RootId, RootedError> {
    let emit_warning_key = context.string_rooted("emitWarning");
    let emit_warning = context.get_property_rooted(process, emit_warning_key)?;
    let message = context.string_rooted(&format!(
        "The {module_name} module is deprecated. Use `node:tls` instead."
    ));
    let warning_type = context.string_rooted("DeprecationWarning");
    let code = context.string_rooted("DEP0192");
    context.call_rooted(
        emit_warning,
        process,
        &[message, warning_type, code],
    )?;
    context.object_rooted()
}

// Node's default TLS policy, from src/node_constants.h DEFAULT_CIPHER_LIST_CORE.
pub(crate) const DEFAULT_CIPHER_LIST: &str = concat!(
    "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:",
    "ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:",
    "ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:",
    "DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:",
    "ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:",
    "DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA"
);

const TLS_FACTORY: &str = quench_js_check::checked_js!(
    r#"(contextApi, cipherOperation, caCertificatesOperation, identityOperation, connectOperation, constructOperation, infoOperation, startOperation, serverCreateOperation, serverConstructOperation, serverContextOperation, errorOperation, releaseOperation, X509Certificate) => {
      const caCertificatesCache = new Map();
      function convertALPNProtocols(protocols, out) {
        function invalid(value, reason) {
          const error = new TypeError(`The argument 'ALPNProtocols' ${reason}. Received ${value}`);
          error.code = 'ERR_INVALID_ARG_VALUE';
          throw error;
        }
        function encode(protocols) {
          const lengths = new Array(protocols.length);
          let size = 0;
          for (let i = 0; i < protocols.length; i++) {
            const length = Buffer.byteLength(protocols[i]);
            if (length === 0) invalid(protocols[i], `at index ${i} must be a non-empty string`);
            if (length > 255) {
              const error = new RangeError(`The byte length of the protocol at index ${i} exceeds the maximum length. It must be <= 255. Received ${length}`);
              error.code = 'ERR_OUT_OF_RANGE';
              throw error;
            }
            lengths[i] = length;
            size += 1 + length;
          }
          const result = Buffer.allocUnsafe(size);
          let offset = 0;
          for (let i = 0; i < protocols.length; i++) {
            result[offset++] = lengths[i];
            offset += result.write(protocols[i], offset);
          }
          return result;
        }
        function validate(buffer) {
          let offset = 0;
          while (offset < buffer.length) {
            const length = buffer[offset];
            if (length === 0) invalid(buffer, 'must not contain zero-length protocol');
            if (offset + 1 + length > buffer.length) invalid(buffer, 'contains truncated protocol');
            offset += 1 + length;
          }
        }
        if (Array.isArray(protocols)) {
          out.ALPNProtocols = encode(protocols);
        } else if (ArrayBuffer.isView(protocols)) {
          const bytes = Buffer.from(protocols.buffer.slice(
            protocols.byteOffset, protocols.byteOffset + protocols.byteLength));
          validate(bytes);
          out.ALPNProtocols = bytes;
        }
      }
      function Server(options, listener) { return serverConstructOperation(this,new.target,options,listener); }
      Server.prototype[Symbol.for('nodejs.rejection')] = function(error, event, socket) {
        if (event === 'secureConnection') socket.destroy(error);
        else {
          const inherited = Object.getPrototypeOf(Server.prototype)?.[Symbol.for('nodejs.rejection')];
          if (typeof inherited === 'function') {
            return Reflect.apply(inherited, this, [error, event, socket]);
          }
          this.emit('error', error);
        }
      };
      Server.prototype.setSecureContext=function setSecureContext(options){return serverContextOperation(this,options);};
      Server.prototype.setTicketKeys=function setTicketKeys(keys){
        const context = this._sharedCreds?.context;
        if (!context || typeof context.setTicketKeys !== 'function') {
          const error = new Error('TLS server context is unavailable');
          error.code = 'ERR_TLS_INVALID_STATE';
          throw error;
        }
        context.setTicketKeys(keys);
      };
      Server.prototype.addContext=function addContext(servername,context){
        if (typeof servername !== 'string') throw new TypeError('"servername" must be a string');
        const secureContext = context instanceof contextApi.SecureContext
          ? context
          : contextApi.createSecureContext(context);
        this._contexts.push([servername.toLowerCase(), secureContext]);
      };
      function TLSSocket(socket, opts) { return constructOperation(this, new.target, socket, {...opts}); }
      TLSSocket.prototype.getProtocol=function getProtocol(){return infoOperation(this,'protocol');};
      TLSSocket.prototype.getPeerCertificate=function getPeerCertificate(detailed){return infoOperation(this,'peerCertificate',detailed);};
      TLSSocket.prototype.getCertificate=function getCertificate(){return infoOperation(this,'certificate');};
      function toX509Certificate(certificate) {
        if (!certificate || !certificate.raw) return undefined;
        const result = new X509Certificate(certificate.raw);
        const issuer = certificate.issuerCertificate;
        if (issuer && issuer !== certificate) result.issuerCertificate = toX509Certificate(issuer);
        return result;
      }
      TLSSocket.prototype.getPeerX509Certificate=function getPeerX509Certificate(){
        return toX509Certificate(this.getPeerCertificate(true));
      };
      TLSSocket.prototype.getX509Certificate=function getX509Certificate(){
        return toX509Certificate(this.getCertificate());
      };
      TLSSocket.prototype.getCipher=function getCipher(){return infoOperation(this,'cipher');};
      TLSSocket.prototype.getSession=function getSession(){return infoOperation(this,'session');};
      TLSSocket.prototype.isSessionReused=function isSessionReused(){return infoOperation(this,'sessionReused');};
      TLSSocket.prototype.destroySoon=function destroySoon(){
        if(this.writable)this.end();
        if(this.writableFinished)this.destroy();
        else this.once('finish',this.destroy);
      };
      TLSSocket.prototype._start=function(){return startOperation(this);};
      TLSSocket.prototype._tlsError=function(err){return errorOperation(this,err);};
      TLSSocket.prototype._releaseControl=function(){return releaseOperation(this);};
      'use strict';
      return {
      getCiphers: (0, () => cipherOperation()),
      convertALPNProtocols,
      getCACertificates: function getCACertificates(type = 'default') {
        if (type === 'bundled') return this.rootCertificates;
        if (caCertificatesCache.has(type)) return caCertificatesCache.get(type);
        const args = process.execArgv || [];
        const disabled = args.includes('--no-use-system-ca');
        const useSystemCA = !disabled && (args.includes('--use-system-ca') ||
          args.includes('--use-openssl-ca') || process.env.NODE_USE_SYSTEM_CA === '1');
        const certificates = caCertificatesOperation(type, useSystemCA);
        caCertificatesCache.set(type, certificates);
        return certificates;
      },
      setDefaultCACertificates: function setDefaultCACertificates(certs) {
        caCertificatesOperation('setDefault', certs);
        caCertificatesCache.delete('default');
      },
      checkServerIdentity: function checkServerIdentity(hostname, cert) { return identityOperation(hostname, cert); },
      SecureContext: contextApi.SecureContext,
      createSecureContext: contextApi.createSecureContext,
      TLSSocket,Server,
      createServer:function createServer(options,listener){return serverCreateOperation(options,listener);},
      connect: function connect(...args) {
        const socket = connectOperation(...args);
        // If the peer closes before the TLS handshake completes, Node reports
        // this as a TLS ECONNRESET and carries the original connection
        // endpoint fields through from the normalized connect options.
        // Without this listener the generic socket EOF error loses `port` and
        // `localAddress`, which callers use to diagnose failed TLS handshakes.
        socket.prependListener('end', function onConnectEnd() {
          if (this._hadError || this._secureEstablished) return;
          this._hadError = true;
          const options = this._tlsOptions;
          const error = Object.assign(new Error(
            'Client network socket disconnected before secure TLS connection was established'
          ), { code: 'ECONNRESET' });
          error.path = options.path;
          error.host = options.host;
          error.port = options.port;
          error.localAddress = options.localAddress;
          this.destroy(error);
        });
        // The transport can surface a reset directly instead of delivering
        // the peer EOF as an `end` event. Preserve the same connect metadata
        // Node's onConnectEnd attaches to that error path.
        socket.prependListener('error', function onConnectError(error) {
          if (!this._secureEstablished && error?.code === 'ECONNRESET' && !this._hadError) {
            this._hadError = true;
            const options = this._tlsOptions;
            error.path = options.path;
            error.host = options.host;
            error.port = options.port;
            error.localAddress = options.localAddress;
          }
        });
        return socket;
      },
    };
    }"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    // The library's crypto wrapper must finish before TLS policy imports it.
    let crypto = crate::host::shared_vm::commonjs::crypto_module(context)?;
    let x509_certificate = socket::get(context, crypto, "X509Certificate")?;
    let factory = context.evaluate_script_rooted(TLS_FACTORY, "node:tls/shared.js")?;
    let undefined = context.undefined();
    let api = facade::install(context)?;
    handle::registry(context)?;
    let policy = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "tls.js",
        "internal/tls/secure-context.js",
        "configSecureContext",
    )?;
    let key = context.string_rooted("standardConfigure");
    context.define_data_property_rooted(api, key, policy, false, false, false)?;
    let patterns = context.evaluate_script_rooted(
        quench_js_check::checked_js!("({trailingDot:()=>/[.]$/,upperCase:()=>/[A-Z]/g})"),
        "node:tls/identity-patterns.js",
    )?;
    let patterns_key = context.string_rooted("identityPatterns");
    context.define_data_property_rooted(api, patterns_key, patterns, false, false, false)?;
    let lower = context.host_function(crate::host::shared_vm::operation("tlsLowercaseName"))?;
    let lower_factory = context.evaluate_script_rooted(
        quench_js_check::checked_js!("(lower)=>function toLowerCase(c){return lower(c);}"),
        "node:tls/lowercase-facade.js",
    )?;
    let lower_callback = context.call_rooted(lower_factory, undefined, &[lower])?;
    let lower_key = context.string_rooted("identityLowercase");
    context.define_data_property_rooted(api, lower_key, lower_callback, false, false, false)?;
    let net = crate::host::shared_vm::commonjs::net_module(context)?;
    let net_key = context.string_rooted("identityNet");
    context.define_data_property_rooted(api, net_key, net, false, false, false)?;
    let binding = facade::binding(context)?;
    let key = context.string_rooted("SecureContext");
    let constructor = context.get_property_rooted(binding, key)?;
    let key = context.string_rooted("contextConstructor");
    context.define_data_property_rooted(api, key, constructor, true, true, true)?;
    let ciphers = context.host_function(crate::host::shared_vm::operation("tlsGetCiphers"))?;
    let ca_certificates =
        context.host_function(crate::host::shared_vm::operation("tlsGetCACertificates"))?;
    let identity =
        context.host_function(crate::host::shared_vm::operation("tlsCheckServerIdentity"))?;
    let connect = context.host_function(crate::host::shared_vm::operation("tlsSocketConnect"))?;
    let construct =
        context.host_function(crate::host::shared_vm::operation("tlsSocketConstruct"))?;
    let info = context.host_function(crate::host::shared_vm::operation("tlsSocketInfo"))?;
    let start = context.host_function(crate::host::shared_vm::operation("tlsSocketStart"))?;
    let create_server =
        context.host_function(crate::host::shared_vm::operation("tlsServerCreate"))?;
    let construct_server =
        context.host_function(crate::host::shared_vm::operation("tlsServerConstruct"))?;
    let server_context =
        context.host_function(crate::host::shared_vm::operation("tlsServerSetContext"))?;
    let error = context.host_function(crate::host::shared_vm::operation("tlsSocketError"))?;
    let release =
        context.host_function(crate::host::shared_vm::operation("tlsSocketReleaseControl"))?;
    let module = context.call_rooted(
        factory,
        undefined,
        &[
            api,
            ciphers,
            ca_certificates,
            identity,
            connect,
            construct,
            info,
            start,
            create_server,
            construct_server,
            server_context,
            error,
            release,
            x509_certificate,
        ],
    )?;
    let tls_key = context.string_rooted("TLSSocket");
    let tls = context.get_property_rooted(module, tls_key)?;
    let socket_key = context.string_rooted("Socket");
    let base = context.get_property_rooted(net, socket_key)?;
    let key = context.string_rooted("prototype");
    let tls_proto = context.get_property_rooted(tls, key)?;
    let base_proto = context.get_property_rooted(base, key)?;
    context.set_prototype_rooted(tls_proto, base_proto)?;
    context.set_prototype_rooted(tls, base)?;
    let key = context.string_rooted("socketConstructor");
    context.define_data_property_rooted(api, key, tls, false, false, false)?;
    let server_key = context.string_rooted("Server");
    let server = context.get_property_rooted(module, server_key)?;
    let base_server = context.get_property_rooted(net, server_key)?;
    let key = context.string_rooted("prototype");
    let server_proto = context.get_property_rooted(server, key)?;
    let base_proto = context.get_property_rooted(base_server, key)?;
    context.set_prototype_rooted(server_proto, base_proto)?;
    context.set_prototype_rooted(server, base_server)?;
    let key = context.string_rooted("serverConstructor");
    context.define_data_property_rooted(api, key, server, false, false, false)?;
    let cli_options = context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .clone();
    // Match Node's TLS CLI precedence: the earliest (lowest) enabled minimum
    // wins, while the highest enabled maximum wins.
    let minimum = [
        ("--tls-min-v1.0", "TLSv1"),
        ("--tls-min-v1.1", "TLSv1.1"),
        ("--tls-min-v1.2", "TLSv1.2"),
        ("--tls-min-v1.3", "TLSv1.3"),
    ]
    .into_iter()
    .find_map(|(flag, version)| cli_options.enabled(flag).then_some(version))
    .unwrap_or("TLSv1.2");
    let maximum = [("--tls-max-v1.3", "TLSv1.3"), ("--tls-max-v1.2", "TLSv1.2")]
        .into_iter()
        .find_map(|(flag, version)| cli_options.enabled(flag).then_some(version))
        .unwrap_or("TLSv1.3");
    let ciphers = cli_options
        .string_value("--tls-cipher-list")
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_CIPHER_LIST);
    for (name, text) in [
        ("DEFAULT_MIN_VERSION", minimum),
        ("DEFAULT_MAX_VERSION", maximum),
        ("DEFAULT_ECDH_CURVE", "auto"),
        ("DEFAULT_CIPHERS", ciphers),
    ] {
        let key = context.string_rooted(name);
        let value = context.string_rooted(text);
        context.set_property_rooted(module, key, value, module)?;
    }
    bundled::install(context, module)?;
    let key = context.string_rooted("module");
    context.set_property_rooted(api, key, module, api)?;
    Ok(module)
}

fn parsed_pfx(
    context: &mut NativeContext<'_, NodeHost>,
    raw: RootId,
    pass: RootId,
) -> Result<openssl::pkcs12::ParsedPkcs12_2, RootedError> {
    let bytes = pfx_bytes(context, raw)?;
    let pass = if context.truthy_rooted(pass)? {
        let bytes = if let Some(text) = context.string_text(pass)? {
            text.into_bytes()
        } else if let Some(bytes) = context.view_bytes_rooted(pass) {
            bytes
        } else {
            let error = context.type_error_rooted("Pass phrase must be a buffer")?;
            let key = context.string_rooted("code");
            let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
            context.define_data_property_rooted(error, key, code, true, true, true)?;
            return Err(context.throw(error));
        };
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        bytes[..end].to_vec()
    } else {
        Vec::new()
    };
    let archive = archive_from_bio(&bytes).map_err(|error| parser_error(context, error))?;
    let parsed = parse_archive(&archive, &pass).map_err(|error| parser_error(context, error))?;
    if parsed.pkey.is_none() {
        return Err(operation_error(
            context,
            "Unable to load private key from PFX data",
        ));
    }
    if parsed.cert.is_none() {
        return Err(operation_error(
            context,
            "Unable to load certificate from PFX data",
        ));
    }
    Ok(parsed)
}

fn material_bytes(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<Vec<u8>, RootedError> {
    if let Some(text) = context.string_text(value)? {
        return Ok(text.into_bytes());
    }
    if let Some(bytes) = context.view_bytes_rooted(value) {
        return Ok(bytes);
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(
        context,
        value,
        name,
        "of type string or an instance of Buffer, TypedArray, or DataView",
    )?)
}

fn pfx_bytes(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<Vec<u8>, RootedError> {
    if let Some(text) = context.string_text(value)? {
        return Ok(text.into_bytes());
    }
    context
        .view_bytes_rooted(value)
        .ok_or_else(|| operation_error(context, "Unable to load PFX certificate"))
}

pub(crate) fn operation_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
) -> RootedError {
    match context.error_rooted(message) {
        Ok(error) => {
            let key = context.string_rooted("code");
            let code = context.string_rooted("ERR_CRYPTO_OPERATION_FAILED");
            match context.define_data_property_rooted(error, key, code, true, true, true) {
                Ok(_) => context.throw(error),
                Err(error) => error,
            }
        }
        Err(error) => error,
    }
}

fn parser_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: openssl::error::ErrorStack,
) -> RootedError {
    let reason = error
        .errors()
        .first()
        .and_then(|error| error.reason())
        .unwrap_or("Unable to load PFX certificate");
    match context.error_rooted(reason) {
        Ok(error) => context.throw(error),
        Err(error) => error,
    }
}

// Node uses the BIO decoder, whose bounded-input errors differ from d2i's
// slice decoder. Keep the same parser entry point rather than rewriting errors.
unsafe extern "C" {
    #[cfg(quench_openssl_provider)]
    fn BIO_s_secmem() -> *const openssl_sys::BIO_METHOD;
    fn d2i_PKCS12_bio(
        input: *mut openssl_sys::BIO,
        output: *mut *mut openssl_sys::PKCS12,
    ) -> *mut openssl_sys::PKCS12;
}

fn archive_from_bio(bytes: &[u8]) -> Result<openssl::pkcs12::Pkcs12, openssl::error::ErrorStack> {
    use foreign_types::ForeignType;
    openssl::init();
    let length = i32::try_from(bytes.len()).map_err(|_| openssl::error::ErrorStack::get())?;
    // Keep the BIO and its backing bytes alive until decoding; free on every exit.
    unsafe {
        // Node LoadBIO owns a writable secure-memory BIO and writes the
        // supplied bytes into it. A borrowed empty BIO has different EOF/error
        // behavior on OpenSSL3.5, so preserve the actual input BIO state too.
        #[cfg(quench_openssl_provider)]
        let input = openssl_sys::BIO_new(BIO_s_secmem());
        #[cfg(not(quench_openssl_provider))]
        let input = openssl_sys::BIO_new_mem_buf(bytes.as_ptr().cast(), length);
        if input.is_null() {
            return Err(openssl::error::ErrorStack::get());
        }
        #[cfg(quench_openssl_provider)]
        if openssl_sys::BIO_write(input, bytes.as_ptr().cast(), length) != length {
            openssl_sys::BIO_free_all(input);
            return Err(openssl::error::ErrorStack::get());
        }
        let archive = d2i_PKCS12_bio(input, std::ptr::null_mut());
        openssl_sys::BIO_free_all(input);
        if archive.is_null() {
            Err(openssl::error::ErrorStack::get())
        } else {
            Ok(openssl::pkcs12::Pkcs12::from_ptr(archive))
        }
    }
}

fn parse_archive(
    archive: &openssl::pkcs12::Pkcs12,
    password: &[u8],
) -> Result<openssl::pkcs12::ParsedPkcs12_2, openssl::error::ErrorStack> {
    use foreign_types::ForeignType;
    let mut password = password.to_vec();
    password.push(0);
    let mut key = std::ptr::null_mut();
    let mut cert = std::ptr::null_mut();
    let mut ca = std::ptr::null_mut();
    // PKCS12_parse takes a C password; ownership of each nonnull result
    // transfers once to its Rust RAII wrapper, including failure cleanup.
    let success = unsafe {
        openssl_sys::PKCS12_parse(
            archive.as_ptr(),
            password.as_ptr().cast(),
            &mut key,
            &mut cert,
            &mut ca,
        )
    };
    let parsed = unsafe {
        openssl::pkcs12::ParsedPkcs12_2 {
            pkey: if key.is_null() {
                None
            } else {
                Some(openssl::pkey::PKey::from_ptr(key))
            },
            cert: if cert.is_null() {
                None
            } else {
                Some(openssl::x509::X509::from_ptr(cert))
            },
            ca: if ca.is_null() {
                None
            } else {
                Some(openssl::stack::Stack::from_ptr(ca))
            },
        }
    };
    if success == 1 {
        Ok(parsed)
    } else {
        Err(openssl::error::ErrorStack::get())
    }
}
