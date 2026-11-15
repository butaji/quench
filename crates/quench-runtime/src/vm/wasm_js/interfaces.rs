//! The Wasm JavaScript API's interface and namespace tables, which `install_webassembly` installs.
use super::*;

/// The interface constructors: name, constructor, prototype methods and accessors.
pub(super) struct Interface {
    pub(super) name: &'static str,
    pub(super) constructor: Native,
    pub(super) statics: &'static [(&'static str, Native)],
    pub(super) methods: &'static [(&'static str, Native)],
    pub(super) getters: &'static [(&'static str, Native, Option<Native>)],
}

pub(super) const INTERFACES: &[Interface] = &[
    Interface {
        name: "Module",
        constructor: Native::WasmModule,
        statics: &[
            ("exports", Native::WasmModuleExports),
            ("imports", Native::WasmModuleImports),
            ("customSections", Native::WasmModuleCustomSections),
        ],
        methods: &[],
        getters: &[],
    },
    Interface {
        name: "Instance",
        constructor: Native::WasmInstance,
        statics: &[],
        methods: &[],
        getters: &[("exports", Native::WasmInstanceExportsGetter, None)],
    },
    Interface {
        name: "Memory",
        constructor: Native::WasmMemory,
        statics: &[],
        methods: &[("grow", Native::WasmMemoryGrow)],
        getters: &[("buffer", Native::WasmMemoryBufferGetter, None)],
    },
    Interface {
        name: "Table",
        constructor: Native::WasmTable,
        statics: &[],
        methods: &[
            ("grow", Native::WasmTableGrow),
            ("get", Native::WasmTableGet),
            ("set", Native::WasmTableSet),
        ],
        getters: &[("length", Native::WasmTableLengthGetter, None)],
    },
    Interface {
        name: "Global",
        constructor: Native::WasmGlobal,
        statics: &[],
        methods: &[("valueOf", Native::WasmGlobalValueOf)],
        getters: &[(
            "value",
            Native::WasmGlobalValueGetter,
            Some(Native::WasmGlobalValueSetter),
        )],
    },
    Interface { name: "Tag", constructor: Native::WasmTag, statics: &[], methods: &[], getters: &[] },
];

pub(super) const NAMESPACE_FUNCTIONS: &[(&str, Native)] = &[
    ("compile", Native::WebAssemblyCompile),
    ("validate", Native::WebAssemblyValidate),
    ("instantiate", Native::WebAssemblyInstantiate),
];

pub(super) fn wasm_source_consumer_name(native: Native) -> &'static str {
    NAMESPACE_FUNCTIONS.iter().find(|(_, operation)| *operation == native).map(|(name, _)| *name)
        .or_else(|| INTERFACES.iter().find(|interface| interface.constructor == native).map(|interface| interface.name))
        .expect("Wasm source consumer has installed API metadata")
}

pub(super) const WASM_ERRORS: &[(&str, Native)] = &[
    ("CompileError", Native::WasmCompileError),
    ("LinkError", Native::WasmLinkError),
    ("RuntimeError", Native::WasmRuntimeError),
];
