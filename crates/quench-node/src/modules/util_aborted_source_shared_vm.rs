//! Scratch integration draft: retain Node's exact async util.aborted policy.
//! This is a declaration projection, not a hand-written policy body. OXC owns
//! its boundary; source byte offsets are never inferred from marker strings.
use crate::host::NodeHost;
use crate::modules::event_constructor_shared_vm::get;
use oxc_ast::ast::Statement;
use oxc_span::GetSpan;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn original_aborted(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    context.with_startup_intrinsics(|context| {
        let commonjs = crate::host::shared_vm::commonjs::node_primordials(context)?
            .ok_or_else(|| RootedError::host("configured Node primordials missing"))?;
        let promise_resolve = get(context, commonjs, "PromiseResolve")?;
        let promise_with_resolvers = get(context, commonjs, "PromiseWithResolvers")?;
        let webidl = crate::host::shared_vm::commonjs::node_library_module(
            context,
            "internal/abort_controller.js",
            "internal/webidl.js",
        )?;
        // Existing load adoption installs the original WebIDL interface converter
        // backed by the native signalStates WeakMap. Do not substitute instanceof.
        let converters = get(context, webidl, "converters")?;
        let validators = crate::host::shared_vm::commonjs::node_library_module(
            context,
            "internal/abort_controller.js",
            "internal/validators.js",
        )?;
        let validate_object = get(context, validators, "validateObject")?;
        let allow_objects = get(context, validators, "kValidateObjectAllowObjects")?;
        let data = context
            .host_mut()
            .shared_state()
            .borrow()
            .event_targets_api
            .ok_or_else(|| RootedError::host("EventTarget API missing"))?;
        let weak_key = get(context, data, "weakKey")?;
        let resist_key = get(context, data, "resistKey")?;
        bind_original(
            context,
            "aborted",
            true,
            &[
                ("converters", converters),
                ("validateObject", validate_object),
                ("kValidateObjectAllowObjects", allow_objects),
                ("PromiseResolve", promise_resolve),
                ("PromiseWithResolvers", promise_with_resolvers),
                ("kWeakHandler", weak_key),
                ("kResistStopPropagation", resist_key),
            ],
        )
    })
}

/// Load an unchanged configured SDK declaration; OXC owns source boundaries.
/// Binding names are the original lexical imports supplied by native providers.
pub(crate) fn bind_original(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    asynchronous: bool,
    bindings: &[(&str, RootId)],
) -> Result<RootId, RootedError> {
    let root = context.host_mut().node_library_root.clone();
    let filename = root.join("lib/internal/abort_controller.js");
    let source = std::fs::read_to_string(&filename)
        .or_else(|error| {
            crate::node_sources::source(&filename, &root)
                .map(str::to_owned)
                .ok_or(error)
        })
        .map_err(|error| RootedError::host(error.to_string()))?;
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::cjs()).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(RootedError::host(
            "configured abort controller source does not parse",
        ));
    }
    let mut declaration = None;
    for statement in &parsed.program.body {
        if let Statement::FunctionDeclaration(function) = statement {
            if function.id.as_ref().is_some_and(|id| id.name == name) {
                if declaration.is_some() || function.r#async != asynchronous {
                    return Err(RootedError::host(
                        "ambiguous configured abort policy declaration",
                    ));
                }
                let span = function.span();
                declaration = source.get(span.start as usize..span.end as usize);
            }
        }
    }
    let declaration = declaration
        .ok_or_else(|| RootedError::host("configured abort policy declaration missing"))?;

    let names = bindings
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(",");
    let factory = format!("(function({names}) {{ 'use strict';\n{declaration}\nreturn {name}; }})");
    let factory = context.evaluate_script_rooted(&factory, &filename.to_string_lossy())?;
    let values = bindings.iter().map(|(_, value)| *value).collect::<Vec<_>>();
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &values)
}
