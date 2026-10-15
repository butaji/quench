//! Native TLS configuration, discovery, and certificate name authorization.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) mod certificate;
pub(crate) mod configuration;
pub(crate) mod contexts;
pub(crate) mod discovery;
pub(crate) mod facade;
pub(crate) mod identity;
pub(crate) mod handle;
pub(crate) mod server;
pub(crate) mod sessions;
pub(crate) mod socket;

// Node's default TLS policy, from src/node_constants.h DEFAULT_CIPHER_LIST_CORE.
const DEFAULT_CIPHER_LIST: &str = concat!(
    "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:",
    "ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:",
    "ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:",
    "DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:",
    "ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:",
    "DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA"
);

const TLS_FACTORY: &str = quench_js_check::checked_js!(
    r#"(contextApi, cipherOperation, identityOperation, connectOperation, constructOperation, infoOperation, startOperation, serverCreateOperation, serverConstructOperation, serverContextOperation, errorOperation, releaseOperation) => {
      function Server(options, listener) { return serverConstructOperation(this,new.target,options,listener); }
      Server.prototype.setSecureContext=function setSecureContext(options){return serverContextOperation(this,options);};
      function TLSSocket(socket, opts) { return constructOperation(this, new.target, socket, {...opts}); }
      TLSSocket.prototype.getProtocol=function getProtocol(){return infoOperation(this,'protocol');};
      TLSSocket.prototype.getPeerCertificate=function getPeerCertificate(detailed){return infoOperation(this,'peerCertificate',detailed);};
      TLSSocket.prototype.isSessionReused=function isSessionReused(){return infoOperation(this,'sessionReused');};
      TLSSocket.prototype._start=function(){return startOperation(this);};
      TLSSocket.prototype._tlsError=function(err){return errorOperation(this,err);};
      TLSSocket.prototype._releaseControl=function(){return releaseOperation(this);};
      'use strict';
      return {
      getCiphers: (0, () => cipherOperation()),
      checkServerIdentity: function checkServerIdentity(hostname, cert) { return identityOperation(hostname, cert); },
      SecureContext: contextApi.SecureContext,
      createSecureContext: contextApi.createSecureContext,
      TLSSocket,Server,
      createServer:function createServer(options,listener){return serverCreateOperation(options,listener);},
      connect: function connect(...args) { return connectOperation(...args); },
    };
    }"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(TLS_FACTORY, "node:tls/shared.js")?;
    let undefined = context.undefined();
    let api = facade::install(context)?;
    handle::registry(context)?;
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
    for (name, text) in [
        ("DEFAULT_MIN_VERSION", "TLSv1.2"),
        ("DEFAULT_MAX_VERSION", "TLSv1.3"),
        ("DEFAULT_ECDH_CURVE", "auto"),
        ("DEFAULT_CIPHERS", DEFAULT_CIPHER_LIST),
    ] {
        let key = context.string_rooted(name);
        let value = context.string_rooted(text);
        context.set_property_rooted(module, key, value, module)?;
    }
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
    fn d2i_PKCS12_bio(
        input: *mut openssl_sys::BIO,
        output: *mut *mut openssl_sys::PKCS12,
    ) -> *mut openssl_sys::PKCS12;
}

fn archive_from_bio(bytes: &[u8]) -> Result<openssl::pkcs12::Pkcs12, openssl::error::ErrorStack> {
    use foreign_types::ForeignType;
    openssl::init();
    let length = i32::try_from(bytes.len()).map_err(|_| openssl::error::ErrorStack::get())?;
    // BIO borrows the input until decoding completes; all early exits free it.
    unsafe {
        let input = openssl_sys::BIO_new_mem_buf(bytes.as_ptr().cast(), length);
        if input.is_null() {
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
