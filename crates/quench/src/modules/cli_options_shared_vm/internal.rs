//! Node's `internal/options` facade. Captured state preserves the identities and
//! refresh boundary of the upstream JS module over the same native projections.
use super::{embedder_options, flags, info, values};
use crate::host::{shared_vm::operation, NodeHost};
use quench_runtime::{HostFunctionId, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;
type Projection = fn(&mut Context<'_>, RootId, &[RootId]) -> Outcome;

struct Export {
    name: &'static str,
    function_name: &'static str,
    operation: HostFunctionId,
}

const EXPORTS: &[Export] = &[
    Export {
        name: "getCLIOptionsInfo",
        function_name: "getCLIOptionsInfoFromBinding",
        operation: operation!("cliModuleInfo"),
    },
    Export {
        name: "getOptionValue",
        function_name: "getOptionValue",
        operation: operation!("cliModuleValue"),
    },
    Export {
        name: "getOptionsAsFlagsFromBinding",
        function_name: "getOptionsAsFlagsFromBinding",
        operation: operation!("cliModuleFlags"),
    },
    Export {
        name: "getAllowUnauthorized",
        function_name: "getAllowUnauthorized",
        operation: operation!("cliModuleAllowUnauthorized"),
    },
    Export {
        name: "getEmbedderOptions",
        function_name: "getEmbedderOptions",
        operation: operation!("cliModuleEmbedder"),
    },
    Export {
        name: "generateConfigJsonSchema",
        function_name: "generateConfigJsonSchema",
        operation: operation!("cliModuleSchema"),
    },
    Export {
        name: "parseNodeOptionsEnvVar",
        function_name: "parseNodeOptionsEnvVar",
        operation: operation!("cliParseNodeOptions"),
    },
    Export {
        name: "refreshOptions",
        function_name: "refreshOptions",
        operation: operation!("cliModuleRefresh"),
    },
];

#[derive(Clone, Copy)]
enum Slot {
    Values,
    Info,
    Flags,
    Embedder,
    Warned,
    Process,
}
impl Slot {
    fn key(self) -> &'static str {
        match self {
            Self::Values => "values",
            Self::Info => "info",
            Self::Flags => "flags",
            Self::Embedder => "embedder",
            Self::Warned => "warned",
            Self::Process => "process",
        }
    }
}

pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    let state = c.null_object_rooted()?;
    let process = c
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    c.assign_named_property_rooted(state, Slot::Process.key(), process)?;
    let module = c.object_rooted()?;
    for export in EXPORTS {
        let function = if export.operation == operation!("cliParseNodeOptions") {
            // The JS facade re-exports this exact native binding function.
            let binding = super::binding(c)?;
            c.get_named_property_rooted(binding, export.name)?
        } else {
            c.host_constructor_named_with_data(export.operation, export.function_name, state)?
        };
        c.assign_named_property_rooted(module, export.name, function)?;
    }
    Ok(module)
}

/// These retained projections are observable module values, not lookup accelerators.
fn materialize(c: &mut Context<'_>, slot: Slot, project: Projection) -> Outcome {
    let state = c.host_function_data()?;
    let value = c.get_named_property_rooted(state, slot.key())?;
    if !c
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
    {
        return Ok(value);
    }
    let undefined = c.undefined();
    let value = project(c, undefined, &[])?;
    c.assign_named_property_rooted(state, slot.key(), value)?;
    Ok(value)
}

pub(crate) fn get_info(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    materialize(c, Slot::Info, info)
}
pub(crate) fn get_flags(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    materialize(c, Slot::Flags, flags)
}
pub(crate) fn get_embedder(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    materialize(c, Slot::Embedder, embedder_options)
}
pub(crate) fn get_value(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let values = materialize(c, Slot::Values, values)?;
    let key = args.first().copied().unwrap_or_else(|| c.undefined());
    c.get_property_rooted(values, key)
}
pub(crate) fn refresh(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let state = c.host_function_data()?;
    let undefined = c.undefined();
    c.assign_named_property_rooted(state, Slot::Values.key(), undefined)?;
    Ok(undefined)
}
pub(crate) fn allow_unauthorized(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let state = c.host_function_data()?;
    let process = c.get_named_property_rooted(state, Slot::Process.key())?;
    let env = c.get_named_property_rooted(process, "env")?;
    let value = c.get_named_property_rooted(env, "NODE_TLS_REJECT_UNAUTHORIZED")?;
    let allowed = c.string_text(value)?.as_deref() == Some("0");
    let warned = c.get_named_property_rooted(state, Slot::Warned.key())?;
    if allowed && !c.truthy_rooted(warned)? {
        let yes = c.boolean(true);
        c.assign_named_property_rooted(state, Slot::Warned.key(), yes)?;
        let emit = c.get_named_property_rooted(process, "emitWarning")?;
        let warning = c.string_rooted("Setting the NODE_TLS_REJECT_UNAUTHORIZED environment variable to '0' makes TLS connections and HTTPS requests insecure by disabling certificate verification.");
        c.call_rooted(emit, process, &[warning])?;
    }
    Ok(c.boolean(allowed))
}
pub(crate) fn generate_schema(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    super::schema::generate(c)
}
