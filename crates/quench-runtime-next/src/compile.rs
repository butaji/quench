use std::fmt;

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::FxHashMap;
use std::rc::Rc;

use crate::bytecode::{
    Atom, AtomTable, Constant, DispatchClass, FieldBase, FieldSite, Function as BcFunction, Instr,
    MethodSite, ObjectSite, Op, Operand, Register, ResidualProgram, SET_THIS_REGISTER,
    Superinstruction, WideInstruction,
};

mod arrow;
mod ast;
mod binding_time;
#[cfg(feature = "profile-memory")]
mod capture_profile;
mod class;
mod liveness;
mod locals;
mod numeric;
#[cfg(feature = "profile-memory")]
mod register_profile;
mod rewrite;
mod sequence;
mod string;
mod template;
use ast::FunctionCompiler;

#[derive(Clone, Debug)]
pub struct Diagnostic {
    source: String,
    message: String,
    span: Span,
}

impl Diagnostic {
    pub(crate) fn unsupported(source: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            message: message.into(),
            span: Span::default(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}..{}: {}",
            self.source, self.span.start, self.span.end, self.message
        )
    }
}

pub struct Engine;

impl Engine {
    pub fn specialize(source: &str, name: &str) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode(source, name, SpecializationMode::Enabled)
    }

    /// Compile through the same OXC pipeline without binding-time or opcode
    /// rewrites, providing the generic reference path for differential gates.
    pub fn specialize_unspecialized(
        source: &str,
        name: &str,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode(source, name, SpecializationMode::Disabled)
    }

    fn specialize_with_mode(
        source: &str,
        name: &str,
        mode: SpecializationMode,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        // OXC's default geometric growth keeps several chunks alive during
        // parsing. Source-sized staging starts with one representative chunk
        // and still grows normally for unusually dense syntax.
        let allocator = Allocator::with_capacity(source.len().saturating_mul(6));
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        if !parsed.diagnostics.is_empty() {
            return Err(parsed
                .diagnostics
                .into_iter()
                .map(|error| Diagnostic {
                    source: name.into(),
                    message: error.to_string(),
                    span: Span::default(),
                })
                .collect());
        }
        let program = Compiler::new_with_mode(name, mode).program(&parsed.program);
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("RQJ_MEMORY").is_some() {
            eprintln!(
                "{{\"kind\":\"rqj-oxc-memory\",\"used\":{},\"capacity\":{}}}",
                allocator.used_bytes(),
                allocator.capacity(),
            );
        }
        program
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpecializationMode {
    Enabled,
    Disabled,
}

struct Compiler<'a> {
    source: &'a str,
    mode: SpecializationMode,
    atoms: Vec<Rc<str>>,
    atom_index: FxHashMap<Rc<str>, Atom>,
    constants: Vec<Constant>,
    constant_index: FxHashMap<ConstantKey, u32>,
    functions: Vec<Option<BcFunction>>,
    errors: Vec<Diagnostic>,
    cache_sites: u16,
    method_sites: Vec<MethodSiteSpec>,
    field_sites: Vec<FieldSite>,
    object_sites: Vec<ObjectSite>,
    superinstructions: Vec<Superinstruction>,
}

type MethodSiteSpec = (Atom, u16, Vec<Register>, Option<(Atom, u16)>);

#[derive(Default)]
struct FunctionOptions<'a> {
    defaults: Option<&'a FormalParameters<'a>>,
    async_function: bool,
    instance_fields: Option<&'a [&'a PropertyDefinition<'a>]>,
    super_static: bool,
    rest_override: bool,
    implicit_super: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum ConstantKey {
    Number(u64),
    String(String),
    StringUnits(Vec<u16>),
    BigInt(String),
    Boolean(bool),
    Null,
    Undefined,
}

impl From<&Constant> for ConstantKey {
    fn from(value: &Constant) -> Self {
        match value {
            Constant::Number(value) => Self::Number(value.to_bits()),
            Constant::String(value) => Self::String(value.clone()),
            Constant::StringUnits(value) => Self::StringUnits(value.clone()),
            Constant::BigInt(value) => Self::BigInt(value.clone()),
            Constant::Boolean(value) => Self::Boolean(*value),
            Constant::Null => Self::Null,
            Constant::Undefined => Self::Undefined,
        }
    }
}

impl<'a> Compiler<'a> {
    fn new_with_mode(source: &'a str, mode: SpecializationMode) -> Self {
        Self {
            source,
            mode,
            atoms: vec![],
            atom_index: FxHashMap::default(),
            constants: vec![],
            constant_index: FxHashMap::default(),
            functions: vec![],
            errors: vec![],
            cache_sites: 0,
            method_sites: vec![],
            field_sites: vec![],
            object_sites: vec![],
            superinstructions: vec![],
        }
    }

    fn program(mut self, program: &Program<'_>) -> Result<ResidualProgram, Vec<Diagnostic>> {
        self.compile_function(
            None,
            &[],
            &program.body,
            &[],
            None,
            FunctionOptions {
                defaults: None,
                async_function: false,
                instance_fields: None,
                super_static: false,
                rest_override: false,
                implicit_super: false,
            },
        );
        if !self.errors.is_empty() {
            return Err(self.errors);
        }
        let mut functions: Vec<_> = self.functions.into_iter().map(Option::unwrap).collect();
        if self.mode == SpecializationMode::Enabled {
            binding_time::apply(&mut functions);
            Self::apply_rewrites(
                &mut functions,
                &mut self.field_sites,
                &mut self.superinstructions,
            );
        }
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("RQJ_MEMORY").is_some() {
            capture_profile::report(&functions);
        }
        for function in &mut functions {
            function.dispatch = if self.mode == SpecializationMode::Enabled {
                Self::dispatch_class(&function.code, &function.wide)
            } else {
                DispatchClass::General
            };
            if function.dispatch == DispatchClass::Numeric {
                let live = liveness::analyze(
                    function,
                    &self.method_sites,
                    &self.field_sites,
                    &self.superinstructions,
                );
                numeric::apply(function, live.as_deref());
            }
        }
        let register_roots = liveness::derive(
            &mut functions,
            &self.method_sites,
            &self.field_sites,
            &self.superinstructions,
        );
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("RQJ_MEMORY").is_some() {
            register_profile::report(
                &functions,
                &self.method_sites,
                &self.field_sites,
                &self.superinstructions,
            );
        }
        for function in &mut functions {
            function.code.shrink_to_fit();
            function.handlers.shrink_to_fit();
        }
        let (mut method_sites, mut method_arguments) =
            Self::flatten_method_sites(self.method_sites);
        let atoms = AtomTable::from_rcs(&self.atoms);
        self.constants.shrink_to_fit();
        functions.shrink_to_fit();
        method_sites.shrink_to_fit();
        method_arguments.shrink_to_fit();
        self.field_sites.shrink_to_fit();
        let program = ResidualProgram {
            specialized: self.mode == SpecializationMode::Enabled,
            atoms,
            constants: self.constants,
            functions,
            cache_sites: self.cache_sites,
            method_sites,
            method_arguments,
            field_sites: self.field_sites,
            object_sites: self.object_sites,
            superinstructions: self.superinstructions,
            register_roots,
        };
        #[cfg(feature = "profile-aggregate")]
        debug_assert!(crate::profile::instruction_word_domains_fit(&program));
        Ok(program)
    }

    fn atom(&mut self, text: &str) -> Atom {
        if let Some(atom) = self.atom_index.get(text) {
            return *atom;
        }
        let atom = self.atoms.len() as Atom;
        let text: Rc<str> = Rc::from(text);
        self.atoms.push(Rc::clone(&text));
        self.atom_index.insert(text, atom);
        atom
    }

    fn constant(&mut self, value: Constant) -> u32 {
        let key = ConstantKey::from(&value);
        if let Some(index) = self.constant_index.get(&key) {
            return *index;
        }
        let index = self.constants.len() as u32;
        self.constants.push(value);
        self.constant_index.insert(key, index);
        index
    }

    fn constant_run(&mut self, values: Vec<Constant>) -> u32 {
        let start = self.constants.len() as u32;
        for value in values {
            let index = self.constants.len() as u32;
            self.constant_index
                .entry(ConstantKey::from(&value))
                .or_insert(index);
            self.constants.push(value);
        }
        start
    }

    fn cache_site(&mut self) -> u16 {
        let site = self.cache_sites;
        self.cache_sites = self
            .cache_sites
            .checked_add(1)
            .expect("property cache limit");
        site
    }

    fn flatten_method_sites(sites: Vec<MethodSiteSpec>) -> (Vec<MethodSite>, Vec<Register>) {
        let argument_capacity = sites.iter().map(|site| site.2.len()).sum();
        let mut arguments = Vec::with_capacity(argument_capacity);
        let metadata = sites
            .into_iter()
            .map(|(atom, cache, values, receiver_path)| {
                let argument_start = arguments.len() as u32;
                let argument_count = values.len() as u16;
                arguments.extend(values);
                MethodSite {
                    atom,
                    cache,
                    argument_start,
                    argument_count,
                    receiver_path,
                }
            })
            .collect();
        (metadata, arguments)
    }

    fn reject(&mut self, span: Span, message: impl Into<String>) {
        self.errors.push(Diagnostic {
            source: self.source.into(),
            message: message.into(),
            span,
        });
    }

    fn compile_function(
        &mut self,
        name: Option<&str>,
        params: &[String],
        body: &[Statement<'_>],
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
        options: FunctionOptions<'_>,
    ) -> u32 {
        let id = self.functions.len() as u32;
        self.functions.push(None);
        let params: Vec<Atom> = params.iter().map(|name| self.atom(name)).collect();
        let mut locals = params.clone();
        self.collect_locals(body, &mut locals);
        let mut function = FunctionCompiler::new(
            self,
            locals,
            scopes.to_vec(),
            id,
            options.super_static,
            options.async_function,
        );
        if let Some(defaults) = options.defaults {
            function.emit_parameter_bindings(defaults);
        }
        function.emit_hoisted(body);
        if options.implicit_super {
            function.emit_implicit_super();
        }
        if let Some(fields) = options.instance_fields {
            function.emit_instance_fields(fields);
        }
        function.statements(body);
        let undefined = function.literal(Constant::Undefined);
        function.emit(Op::Return, undefined, 0, 0, 0);
        let captures_locals = function
            .code
            .iter()
            .any(|instruction| instruction.op() == Op::MakeClosure)
            || function
                .wide
                .iter()
                .any(|instruction| instruction.op() == Op::MakeClosure);
        if captures_locals {
            for instruction in &mut function.code {
                let op = match instruction.op() {
                    Op::LoadLocal => Op::LoadEnvLocal,
                    Op::StoreLocal => Op::StoreEnvLocal,
                    other => other,
                };
                instruction.set_op(op);
            }
            for instruction in &mut function.wide {
                let op = match instruction.op() {
                    Op::LoadLocal => Op::LoadEnvLocal,
                    Op::StoreLocal => Op::StoreEnvLocal,
                    other => other,
                };
                instruction.set_op(op);
            }
        }
        let result = BcFunction {
            parent,
            name: name.map(|value| function.owner.atom(value)),
            params: params.len() as u16,
            rest: options.rest_override
                || options.defaults.is_some_and(|value| value.rest.is_some()),
            is_async: options.async_function,
            locals: function.locals.len() as u16,
            code: function.code,
            wide: function.wide,
            registers: function.max_reg,
            dispatch: DispatchClass::General,
            handlers: function.handlers,
            register_root_offset: u32::MAX,
        };
        self.functions[id as usize] = Some(result);
        id
    }

    fn apply_rewrites(
        functions: &mut [BcFunction],
        field_sites: &mut Vec<FieldSite>,
        superinstructions: &mut Vec<Superinstruction>,
    ) {
        for function in functions {
            if function.wide.is_empty() {
                rewrite::apply(function, field_sites, superinstructions);
            }
        }
    }

    fn dispatch_class(code: &[Instr], wide: &[WideInstruction]) -> DispatchClass {
        if !wide.is_empty() {
            return DispatchClass::General;
        }
        let binaries = code
            .iter()
            .filter(|instruction| instruction.op() == Op::Binary)
            .count();
        if code.len() >= 32 && binaries * 3 >= code.len() {
            DispatchClass::Numeric
        } else {
            DispatchClass::General
        }
    }
}

#[cfg(test)]
mod tests;
