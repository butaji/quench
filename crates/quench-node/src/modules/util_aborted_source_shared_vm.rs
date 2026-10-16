//! Scratch integration draft: retain Node's exact async util.aborted policy.
//! This is a declaration projection, not a hand-written policy body. OXC owns
//! its boundary; source byte offsets are never inferred from marker strings.
use crate::host::NodeHost;
use crate::modules::event_constructor_shared_vm::get;
use quench_runtime::{NativeContext, RootId, RootedError};
use oxc_ast::ast::Statement;
use oxc_span::GetSpan;

pub(crate) fn original_aborted(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    context.with_startup_intrinsics(|context| {
        let root = context.host_mut().node_library_root.clone();
        let filename = root.join("lib/internal/abort_controller.js");
        let source = std::fs::read_to_string(&filename).or_else(|error| {
            crate::node_sources::source(&filename, &root)
                .map(str::to_owned).ok_or(error)
        }).map_err(|error| RootedError::host(error.to_string()))?;
        let allocator = oxc_allocator::Allocator::default();
        let parsed = oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::cjs()).parse();
        if !parsed.diagnostics.is_empty() {
            return Err(RootedError::host("configured abort controller source does not parse"));
        }
        let mut declaration = None;
        for statement in &parsed.program.body {
            if let Statement::FunctionDeclaration(function) = statement {
                if function.id.as_ref().is_some_and(|id| id.name == "aborted") {
                    if declaration.is_some() || !function.r#async {
                        return Err(RootedError::host("ambiguous or non-async configured aborted export"));
                    }
                    let span = function.span();
                    declaration = source.get(span.start as usize..span.end as usize);
                }
            }
        }
        let declaration = declaration.ok_or_else(|| RootedError::host("configured aborted declaration missing"))?;
        let commonjs = crate::host::shared_vm::commonjs::node_primordials(context)?
            .ok_or_else(|| RootedError::host("configured Node primordials missing"))?;
        let promise_resolve = get(context, commonjs, "PromiseResolve")?;
        let promise_with_resolvers = get(context, commonjs, "PromiseWithResolvers")?;
        let webidl = crate::host::shared_vm::commonjs::node_library_module(
            context, "internal/abort_controller.js", "internal/webidl.js",
        )?;
        // Existing load adoption installs the original WebIDL interface converter
        // backed by the native signalStates WeakMap. Do not substitute instanceof.
        let converters = get(context, webidl, "converters")?;
        let validators = crate::host::shared_vm::commonjs::node_library_module(
            context, "internal/abort_controller.js", "internal/validators.js",
        )?;
        let validate_object = get(context, validators, "validateObject")?;
        let allow_objects = get(context, validators, "kValidateObjectAllowObjects")?;
        let data = context.host_mut().shared_state().borrow().event_targets_api
            .ok_or_else(|| RootedError::host("EventTarget API missing"))?;
        let weak_key = get(context, data, "weakKey")?;
        let resist_key = get(context, data, "resistKey")?;
        let factory = format!(
            "(function(converters,validateObject,kValidateObjectAllowObjects,PromiseResolve,PromiseWithResolvers,kWeakHandler,kResistStopPropagation) {{ 'use strict';\n{declaration}\nreturn aborted; }})"
        );
        let factory = context.evaluate_script_rooted(&factory, &filename.to_string_lossy())?;
        let undefined = context.undefined();
        context.call_rooted(factory, undefined, &[
            converters, validate_object, allow_objects, promise_resolve,
            promise_with_resolvers, weak_key, resist_key,
        ])
    })
}
