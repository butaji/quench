//! Native incremental Sign/Verify handles for unchanged SDK sig.js.
//! Each weak identity owns one EVP digest context. Finalization consumes it;
//! no message chunks, private DER, or guest roots are retained here.
use super::{
    crypto_asymmetric::KeyKind,
    crypto_asymmetric_operations::SignatureOptions,
    crypto_signature_state::{Error, State as Signature},
};
use crate::host::NodeHost;
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime};
type Context<'a> = NativeContext<'a, NodeHost>;
#[derive(Default)]
pub(crate) struct State {
    handles: Vec<Handle>,
}
struct Handle {
    identity: HostWeakRoot,
    value: Signature,
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
fn coded(c: &mut Context<'_>, code: &str, message: &str, ty: bool) -> RootedError {
    let root = if ty {
        c.type_error_rooted(message)
    } else {
        c.error_rooted(message)
    };
    match root {
        Ok(root) => {
            let key = c.string_rooted("code");
            let value = c.string_rooted(code);
            match c.define_data_property_rooted(root, key, value, true, true, true) {
                Ok(_) => c.throw(root),
                Err(error) => error,
            }
        }
        Err(error) => error,
    }
}
fn error(c: &mut Context<'_>, error: Error) -> RootedError {
    match error {
        Error::UnknownDigest => coded(c, "ERR_CRYPTO_INVALID_DIGEST", "Invalid digest", true),
        Error::NotInitialized => coded(c, "ERR_CRYPTO_INVALID_STATE", "Not initialised", false),
        Error::UnsupportedOneShot => coded(
            c,
            "ERR_CRYPTO_UNSUPPORTED_OPERATION",
            "Unsupported crypto operation",
            false,
        ),
        Error::Digest(super::crypto_digest::DigestError::OpenSsl(stack))
        | Error::Key(super::crypto_asymmetric::KeyError::OpenSsl(stack)) => {
            super::crypto_error_shared_vm::decorated(c, stack)
        }
        Error::Key(super::crypto_asymmetric::KeyError::Unsupported("Malformed signature")) => {
            coded(
                c,
                "ERR_CRYPTO_OPERATION_FAILED",
                "Malformed signature",
                false,
            )
        }
        _ => coded(
            c,
            "ERR_CRYPTO_OPERATION_FAILED",
            "Signature operation failed",
            false,
        ),
    }
}
fn index(c: &mut Context<'_>, receiver: RootId) -> Result<usize, RootedError> {
    let shared = c.host_mut().shared_state();
    let identities = shared
        .borrow()
        .crypto_signatures
        .handles
        .iter()
        .map(|handle| handle.identity)
        .collect::<Vec<_>>();
    for (index, identity) in identities.into_iter().enumerate() {
        if let Some(value) = c.resolve_host_weak_root(identity) {
            if c.strict_equal_rooted(value, receiver)? {
                return Ok(index);
            }
        }
    }
    Err(coded(
        c,
        "ERR_INVALID_THIS",
        "Invalid native signature receiver",
        true,
    ))
}
pub(crate) fn new(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    let dead = shared
        .borrow()
        .crypto_signatures
        .handles
        .iter()
        .enumerate()
        .filter_map(|(index, handle)| {
            c.resolve_host_weak_root(handle.identity)
                .is_none()
                .then_some(index)
        })
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let handle = shared.borrow_mut().crypto_signatures.handles.remove(index);
        c.release_host_weak_root(handle.identity);
    }
    let identity = c.host_weak_root(receiver)?;
    shared.borrow_mut().crypto_signatures.handles.push(Handle {
        identity,
        value: Signature::default(),
    });
    Ok(receiver)
}
pub(crate) fn init(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let index = index(c, receiver)?;
    let name = c.to_string(args[0])?;
    let shared = c.host_mut().shared_state();
    let result = shared.borrow_mut().crypto_signatures.handles[index]
        .value
        .initialize(&name);
    result.map_err(|failure| error(c, failure))?;
    Ok(c.undefined())
}
pub(crate) fn update(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let index = index(c, receiver)?;
    let bytes = super::crypto_hash_shared_vm::input(c, args[0], args.get(1).copied())?;
    let shared = c.host_mut().shared_state();
    let result = shared.borrow_mut().crypto_signatures.handles[index]
        .value
        .update(&bytes);
    result.map_err(|failure| error(c, failure))?;
    Ok(c.undefined())
}
fn options(
    c: &mut Context<'_>,
    args: &[RootId],
    offset: usize,
) -> Result<SignatureOptions, RootedError> {
    let mut number = |index| -> Result<Option<i32>, RootedError> {
        let Some(root) = args.get(index).copied() else {
            return Ok(None);
        };
        if c.rooted_value(root)
            .is_some_and(|value| value.is_undefined())
        {
            return Ok(None);
        }
        Ok(Some(c.to_number(root)? as i32))
    };
    let padding = number(offset)?.map(openssl::rsa::Padding::from_raw);
    let salt_length = number(offset + 1)?;
    let ieee_p1363 =
        number(offset + 2)? == Some(super::crypto_signature_request::Encoding::P1363 as i32);
    Ok(SignatureOptions {
        padding,
        salt_length,
        ieee_p1363,
    })
}
pub(crate) fn sign(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let index = index(c, receiver)?;
    let target = c.number(2.0);
    let input = [target, args[0], args[1], args[2], args[3], args[4]];
    let key = super::crypto_asymmetric_shared_vm::import_input(c, KeyKind::Private, &input)?;
    let options = options(c, args, 5)?;
    let shared = c.host_mut().shared_state();
    let result = shared.borrow_mut().crypto_signatures.handles[index]
        .value
        .sign(&key, &options);
    let bytes = result.map_err(|failure| error(c, failure))?;
    super::buffer_shared_vm::from_bytes(c, &bytes)
}
pub(crate) fn verify(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let index = index(c, receiver)?;
    let target = c.number(1.0);
    let input = [target, args[0], args[1], args[2], args[3], args[4]];
    let key = super::crypto_asymmetric_shared_vm::import_input(c, KeyKind::Public, &input)?;
    let signature = c
        .bytes_prefix_rooted(args[5], usize::MAX)
        .ok_or_else(|| RootedError::host("Native signature is not a BufferSource"))?;
    let options = options(c, args, 6)?;
    let shared = c.host_mut().shared_state();
    let result = shared.borrow_mut().crypto_signatures.handles[index]
        .value
        .verify(&key, &signature, &options);
    let result = result.map_err(|failure| error(c, failure))?;
    Ok(c.boolean(result))
}
pub(crate) fn install(c: &mut Context<'_>, binding: RootId) -> Result<(), RootedError> {
    for (name, final_name, final_operation) in [
        ("Sign", "sign", "cryptoNativeSignFinal"),
        ("Verify", "verify", "cryptoNativeVerifyFinal"),
    ] {
        let constructor = c.host_constructor(crate::host::shared_vm::operation(
            "cryptoNativeSignatureNew",
        ))?;
        let text = c.string_rooted(name);
        let property = c.string_rooted("name");
        c.define_data_property_rooted(constructor, property, text, false, false, true)?;
        let prototype = c.object_rooted()?;
        for (name, operation) in [
            ("init", "cryptoNativeSignatureInit"),
            ("update", "cryptoNativeSignatureUpdate"),
            (final_name, final_operation),
        ] {
            let function =
                c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
            put(c, prototype, name, function)?
        }
        let property = c.string_rooted("constructor");
        c.define_data_property_rooted(prototype, property, constructor, true, false, true)?;
        let property = c.string_rooted("prototype");
        c.define_data_property_rooted(constructor, property, prototype, true, false, false)?;
        put(c, binding, name, constructor)?
    }
    Ok(())
}
pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let dead = shared
        .borrow()
        .crypto_signatures
        .handles
        .iter()
        .enumerate()
        .filter_map(|(index, handle)| {
            (!runtime.host_weak_root_is_live(handle.identity)).then_some(index)
        })
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let handle = shared.borrow_mut().crypto_signatures.handles.remove(index);
        runtime.release_host_weak_root(handle.identity);
    }
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let handles = std::mem::take(&mut shared.borrow_mut().crypto_signatures.handles);
    for handle in handles {
        runtime.release_host_weak_root(handle.identity);
    }
}
