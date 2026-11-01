//! OXC source transforms used by the opt-in HMR loader.

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use oxc_allocator::Allocator;
use oxc_ast::ast::{ArrowFunctionExpression, BindingPattern, Decorator, Function};
use oxc_ast_visit::{walk, Visit};
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_parser::Parser;
use oxc_resolver::{ExtendsField, ResolveOptions, Resolver, TsConfig, TsconfigDiscovery};
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType};
use oxc_syntax::scope::ScopeFlags;
use oxc_transformer::{JsxOptions, JsxRuntime, TransformOptions, Transformer};
use sha1::Digest;

pub(crate) fn needs_transform(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("jsx" | "ts" | "tsx" | "mts" | "cts")
    )
}

pub(crate) struct PreparedSource {
    pub(crate) source: String,
    pub(crate) kind: quench_runtime::SourceKind,
    pub(crate) type_query_positions: Vec<TypeQueryPosition>,
    pub(crate) content_digest: crate::host::hmr::FileContentDigest,
    pub(crate) restart_only: bool,
}

/// Maps one TypeScript parameter checker query onto its generated function in Quench source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TypeQueryPosition {
    pub(crate) function_start: u32,
    pub(crate) parameter_index: u32,
    pub(crate) source_position_utf16: u32,
    pub(crate) source_digest: crate::host::hmr::FileContentDigest,
}

pub(crate) fn transform_with_type_positions(
    path: &Path,
    source: &str,
) -> Result<(String, Vec<TypeQueryPosition>), String> {
    let source_type = SourceType::from_path(path).map_err(|error| error.to_string())?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(format_diagnostics(path, parsed.diagnostics));
    }

    let mut program = parsed.program;
    let mut source_parameters = SourceParameters::default();
    if is_typescript(path) {
        source_parameters.visit_program(&program);
    }
    let options = transform_options(path)?;
    reject_untransformed_decorators(path, &program, options.decorator.legacy)?;
    let semantic = SemanticBuilder::new().build(&program);
    if !semantic.diagnostics.is_empty() {
        return Err(format_diagnostics(path, semantic.diagnostics));
    }

    let transformed = Transformer::new(&allocator, path, &options)
        .build_with_scoping(semantic.semantic.into_scoping(), &mut program);
    if !transformed.diagnostics.is_empty() {
        return Err(format_diagnostics(path, transformed.diagnostics));
    }

    let codegen = Codegen::new()
        .with_options(CodegenOptions {
            source_map_path: is_typescript(path).then(|| path.to_path_buf()),
            ..CodegenOptions::default()
        })
        .build(&program);
    let code = if options.decorator.legacy {
        inline_legacy_decorator_helper(&codegen.code)
    } else {
        codegen.code.clone()
    };
    let type_query_positions = codegen
        .map
        .as_ref()
        .map(|source_map| source_parameters.map_to_generated(source, &codegen.code, source_map))
        .unwrap_or_default();
    Ok((code, type_query_positions))
}

fn has_decorators(program: &oxc_ast::ast::Program<'_>) -> bool {
    #[derive(Default)]
    struct DecoratorPresence(bool);

    impl<'a> Visit<'a> for DecoratorPresence {
        fn visit_decorator(&mut self, decorator: &Decorator<'a>) {
            self.0 = true;
            walk::walk_decorator(self, decorator);
        }
    }

    let mut presence = DecoratorPresence::default();
    presence.visit_program(program);
    presence.0
}

fn reject_untransformed_decorators(
    path: &Path,
    program: &oxc_ast::ast::Program<'_>,
    legacy: bool,
) -> Result<(), String> {
    if !legacy && has_decorators(program) {
        return Err(format!(
            "{}: ECMAScript decorators are not transformed by the pinned OXC transformer",
            path.display()
        ));
    }
    Ok(())
}

/// Validate raw JavaScript without sending its source through OXC code generation.
///
/// HMR uses OXC's AST to reject decorators that Quench's parser accepts but does not
/// execute. Other syntax diagnostics remain owned by the normal Quench compile path.
pub(crate) fn validate_raw_javascript(path: &Path, source: &str) -> Result<(), String> {
    let source_type = SourceType::from_path(path).map_err(|error| error.to_string())?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    reject_untransformed_decorators(path, &parsed.program, false)?;
    Ok(())
}

fn transform_options(path: &Path) -> Result<TransformOptions, String> {
    let mut options = TransformOptions::default();
    if is_jsx(path) {
        options.jsx = JsxOptions::enable();
    }
    if !is_typescript(path) && !is_jsx(path) {
        return Ok(options);
    }
    let config = tsconfig_resolver()
        .find_tsconfig(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let Some(config) = config else {
        let compiler = oxc_resolver::CompilerOptions::default();
        configure_target(&mut options, &compiler, path)?;
        if is_typescript(path) {
            configure_typescript(&mut options, &compiler);
        }
        return Ok(options);
    };
    let compiler = &config.compiler_options;
    configure_target(&mut options, compiler, path)?;
    if is_jsx(path) {
        configure_jsx(&mut options.jsx, compiler, path)?;
    }
    if is_typescript(path) {
        configure_typescript(&mut options, compiler);
    }
    Ok(options)
}

fn configure_target(
    options: &mut TransformOptions,
    compiler: &oxc_resolver::CompilerOptions,
    path: &Path,
) -> Result<(), String> {
    let configured_target = compiler
        .target
        .as_deref()
        .map(str::to_ascii_lowercase)
        // The pinned TSRS TypeScript 7 compiler defaults an omitted target to
        // its latest standard target, independently of module kind.
        .unwrap_or_else(|| "esnext".to_owned());
    let use_define = match compiler.use_define_for_class_fields {
        Some(use_define) => use_define,
        None => class_fields_use_define_by_default(&configured_target).ok_or_else(|| {
            format!(
                "{}: unsupported tsconfig target {}",
                path.display(),
                configured_target
            )
        })?,
    };
    options.assumptions.set_public_class_fields = !use_define;
    options.typescript.remove_class_fields_without_initializer = !use_define;
    // HMR runs at Quench's native JavaScript target. The TypeScript target still determines
    // the class-field default above, but downleveling would add helper-package imports that
    // module execution does not need.
    Ok(())
}

fn class_fields_use_define_by_default(target: &str) -> Option<bool> {
    match target {
        "es3" | "es5" | "es6" | "es7" => Some(false),
        "esnext" => Some(true),
        _ => target
            .strip_prefix("es")
            .and_then(|year| year.parse::<u16>().ok())
            .map(|year| year >= 2022),
    }
}

fn configure_jsx(
    options: &mut JsxOptions,
    compiler: &oxc_resolver::CompilerOptions,
    path: &Path,
) -> Result<(), String> {
    match compiler.jsx.as_deref() {
        Some("react") => options.runtime = JsxRuntime::Classic,
        Some("react-jsxdev") => options.development = true,
        Some("react-jsx" | "preserve" | "react-native") | None => {}
        Some(value) => {
            return Err(format!(
                "{}: unsupported tsconfig jsx mode `{value}`",
                path.display()
            ));
        }
    }
    options
        .import_source
        .clone_from(&compiler.jsx_import_source);
    options.pragma.clone_from(&compiler.jsx_factory);
    options
        .pragma_frag
        .clone_from(&compiler.jsx_fragment_factory);
    Ok(())
}

fn configure_typescript(options: &mut TransformOptions, compiler: &oxc_resolver::CompilerOptions) {
    if let Some(pragma) = &compiler.jsx_factory {
        options.typescript.jsx_pragma = Cow::Owned(pragma.clone());
    }
    if let Some(pragma_fragment) = &compiler.jsx_fragment_factory {
        options.typescript.jsx_pragma_frag = Cow::Owned(pragma_fragment.clone());
    }
    options.typescript.only_remove_type_imports = compiler.verbatim_module_syntax == Some(true)
        || compiler.preserve_value_imports == Some(true)
        || compiler
            .imports_not_used_as_values
            .as_deref()
            .is_some_and(|value| matches!(value, "preserve" | "error"));
    options.decorator.legacy = compiler.experimental_decorators.unwrap_or(false);
    options.decorator.emit_decorator_metadata = compiler.emit_decorator_metadata.unwrap_or(false);
    options.decorator.strict_null_checks = compiler
        .strict_null_checks
        .or(compiler.strict)
        .unwrap_or(true);
}

/// OXC's legacy decorator transform imports a runtime helper. Keep this compiler helper
/// self-contained so an application does not gain an OXC runtime package dependency. The
/// generated helper binding is blanked in place, preserving source-map offsets, and its
/// declaration is appended as a hoisted function.
fn inline_legacy_decorator_helper(source: &str) -> String {
    const PREFIX: &str = "@oxc-project/runtime/helpers/";
    let mut output = String::with_capacity(source.len());
    let mut helpers = Vec::new();
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();
        let module_start = trimmed.find(PREFIX);
        let helper = module_start.and_then(|start| {
            let module = &trimmed[start + PREFIX.len()..];
            let end = module.find(['\"', '\''])?;
            let helper = &module[..end];
            matches!(helper, "decorate" | "decorateMetadata" | "decorateParam")
                .then(|| helper.to_owned())
        });
        let binding = helper.as_ref().and_then(|_| {
            trimmed
                .strip_prefix("import ")
                .and_then(|import| import.split_once(" from "))
                .map(|(binding, _)| binding)
                .or_else(|| {
                    trimmed
                        .strip_prefix("var ")
                        .or_else(|| trimmed.strip_prefix("const "))
                        .and_then(|declaration| declaration.split_once(" = require("))
                        .map(|(binding, _)| binding)
                })
        });
        if let (Some(helper), Some(binding)) = (helper, binding) {
            if !helpers
                .iter()
                .any(|(existing, _): &(String, String)| existing == binding)
            {
                helpers.push((binding.to_owned(), helper));
            }
            output.extend(line.chars().map(|character| {
                if character == '\n' || character == '\r' {
                    character
                } else {
                    ' '
                }
            }));
        } else {
            output.push_str(line);
        }
    }
    for (helper_name, helper) in helpers {
        output.push('\n');
        match helper.as_str() {
            "decorate" => output.push_str(&format!(
                "function {helper_name}(decorators, target, key, descriptor) {{\n\
                var argument_count = arguments.length;\n\
                var result = argument_count < 3 ? target : descriptor === null\n\
                  ? (descriptor = Object.getOwnPropertyDescriptor(target, key))\n\
                  : descriptor;\n\
                var decorator;\n\
                if (typeof Reflect === 'object' && typeof Reflect.decorate === 'function') {{\n\
                  result = Reflect.decorate(decorators, target, key, descriptor);\n\
                }} else {{\n\
                  for (var index = decorators.length - 1; index >= 0; index--) {{\n\
                    decorator = decorators[index];\n\
                    if (decorator) {{\n\
                      result = (argument_count < 3\n\
                        ? decorator(result)\n\
                        : argument_count > 3\n\
                          ? decorator(target, key, result)\n\
                          : decorator(target, key)) || result;\n\
                    }}\n\
                  }}\n\
                }}\n\
                if (argument_count > 3 && result) Object.defineProperty(target, key, result);\n\
                return result;\n\
             }}\n"
            )),
            "decorateMetadata" => output.push_str(&format!(
                "function {helper_name}(key, value) {{\n\
                if (typeof Reflect === 'object' && typeof Reflect.metadata === 'function') {{\n\
                  return Reflect.metadata(key, value);\n\
                }}\n\
             }}\n"
            )),
            "decorateParam" => output.push_str(&format!(
                "function {helper_name}(parameter_index, decorator) {{\n\
                return function (target, key) {{\n\
                  decorator(target, key, parameter_index);\n\
                }};\n\
             }}\n"
            )),
            _ => unreachable!("only recognized decorator helpers are collected"),
        }
    }
    output
}

fn is_jsx(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("jsx" | "tsx")
    )
}

fn tsconfig_resolver() -> Resolver {
    Resolver::new(ResolveOptions {
        tsconfig: Some(TsconfigDiscovery::Auto),
        ..ResolveOptions::default()
    })
}

/// Config files participate in the loaded module graph as restart-only dependencies.
pub(crate) fn tsconfig_watch_paths(path: &Path) -> Vec<PathBuf> {
    if !is_typescript(path) && !is_jsx(path) {
        return Vec::new();
    }
    // `TsconfigDiscovery::Auto` checks `tsconfig.json` from the source directory
    // through every ancestor. Track missing candidates too: creating a config,
    // or changing one that did not previously claim this file, can change the
    // transform selected for an already-loaded module.
    let mut paths = path
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .map(|directory| directory.join("tsconfig.json"))
        .collect::<Vec<_>>();
    let mut visited_configs = std::collections::HashSet::new();
    let extends_resolver = tsconfig_extends_resolver();
    // Any ancestor candidate may become the selected project after a config edit.
    // Its extends and references can live outside the source's ancestor chain.
    for candidate in paths.clone() {
        if candidate.is_file() {
            collect_tsconfig_paths(
                &candidate,
                &mut paths,
                &mut visited_configs,
                &extends_resolver,
            );
        }
    }
    let resolver = tsconfig_resolver();
    if let Ok(Some(config)) = resolver.find_tsconfig(path) {
        collect_tsconfig_paths(
            config.path(),
            &mut paths,
            &mut visited_configs,
            &extends_resolver,
        );
    }
    paths.sort();
    paths.dedup();
    paths
}

fn collect_tsconfig_paths(
    path: &Path,
    paths: &mut Vec<PathBuf>,
    visited: &mut std::collections::HashSet<PathBuf>,
    extends_resolver: &Resolver,
) {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(path.clone()) {
        return;
    }
    paths.push(path.clone());
    let Ok(source) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(config) = TsConfig::parse(true, &path, &path, source) else {
        return;
    };
    let extends = match config.extends {
        Some(ExtendsField::Single(specifier)) => vec![specifier],
        Some(ExtendsField::Multiple(specifiers)) => specifiers,
        None => Vec::new(),
    };
    for specifier in extends {
        if let Some(parent) = resolve_extended_tsconfig(extends_resolver, &path, &specifier) {
            collect_tsconfig_paths(&parent, paths, visited, extends_resolver);
        }
    }
    for reference in config.references {
        let Some(reference_config) = path
            .parent()
            .and_then(|directory| tsconfig_candidate(directory.join(reference.path)))
        else {
            continue;
        };
        collect_tsconfig_paths(&reference_config, paths, visited, extends_resolver);
    }
}

fn resolve_extended_tsconfig(
    resolver: &Resolver,
    config: &Path,
    specifier: &str,
) -> Option<PathBuf> {
    resolver
        .resolve(config.parent()?, specifier)
        .ok()
        .map(|resolution| resolution.path().to_path_buf())
}

fn tsconfig_extends_resolver() -> Resolver {
    Resolver::new(ResolveOptions {
        tsconfig: None,
        condition_names: vec!["node".into(), "import".into()],
        extensions: vec![".json".into()],
        main_files: vec!["tsconfig".into()],
        ..ResolveOptions::default()
    })
}

fn tsconfig_candidate(path: PathBuf) -> Option<PathBuf> {
    let candidates = if path.extension().is_some() {
        vec![path]
    } else {
        vec![
            path.with_extension("json"),
            path.join("tsconfig.json"),
            path,
        ]
    };
    candidates.into_iter().find(|candidate| candidate.is_file())
}

pub(crate) fn prepare(path: &Path) -> Result<Option<PreparedSource>, String> {
    if !matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("cjs" | "cts" | "js" | "jsx" | "json" | "mjs" | "mts" | "ts" | "tsx")
    ) {
        return Ok(None);
    }
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let content_digest = sha1::Sha1::digest(&bytes).into();
    let source = crate::host::shared_vm::commonjs::source_text(&bytes);
    let source = source.strip_prefix('\u{feff}').unwrap_or(&source);
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        if is_tsconfig_file(path) {
            return Ok(Some(PreparedSource {
                source: String::new(),
                kind: quench_runtime::SourceKind::Script,
                type_query_positions: Vec::new(),
                content_digest,
                restart_only: true,
            }));
        }
        if let Err(error) = serde_json::from_str::<serde_json::Value>(source) {
            if TsConfig::parse(true, path, path, source.to_owned()).is_err() {
                return Err(format!("{}: {error}", path.display()));
            }
        }
        return Ok(Some(PreparedSource {
            source: source.to_owned(),
            kind: quench_runtime::SourceKind::Script,
            type_query_positions: Vec::new(),
            content_digest,
            restart_only: false,
        }));
    }

    let (source, type_query_positions) = if needs_transform(path) {
        transform_with_type_positions(path, source)?
    } else {
        let source_type = SourceType::from_path(path).map_err(|error| error.to_string())?;
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, source_type).parse();
        if !parsed.diagnostics.is_empty() {
            return Err(format_diagnostics(path, parsed.diagnostics));
        }
        reject_untransformed_decorators(path, &parsed.program, false)?;
        (source.to_owned(), Vec::new())
    };
    let kind = crate::host::shared_vm::commonjs::hmr_source_kind(path, &source)?;
    if kind == quench_runtime::SourceKind::Module {
        if let Err(error) = std::str::from_utf8(&bytes) {
            return Err(format!("module {} is not UTF-8: {error}", path.display()));
        }
    }
    Ok(Some(PreparedSource {
        source,
        kind,
        type_query_positions,
        content_digest,
        restart_only: false,
    }))
}

pub(crate) fn is_tsconfig_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    (name.starts_with("tsconfig") || name.starts_with("jsconfig"))
        && path
            .extension()
            .is_some_and(|extension| extension == "json")
}

pub(crate) fn source_file_is_missing(path: &Path) -> bool {
    matches!(
        std::fs::metadata(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    )
}

fn is_typescript(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "mts" | "cts")
    )
}

#[derive(Default)]
struct SourceParameters {
    functions: Vec<SourceFunctionParameters>,
}

struct SourceFunctionParameters {
    function_start: u32,
    function_end: u32,
    parameters: Vec<SourceParameter>,
}

struct SourceParameter {
    index: u32,
    start: u32,
}

impl SourceParameters {
    fn record_function(
        &mut self,
        function_start: u32,
        function_end: u32,
        parameters: &oxc_ast::ast::FormalParameters<'_>,
    ) {
        let simple_parameters = parameters
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, parameter)| {
                matches!(parameter.pattern, BindingPattern::BindingIdentifier(_)).then_some(
                    SourceParameter {
                        index: index as u32,
                        start: parameter.pattern.span().start,
                    },
                )
            })
            .collect::<Vec<_>>();
        if !simple_parameters.is_empty() {
            self.functions.push(SourceFunctionParameters {
                function_start,
                function_end,
                parameters: simple_parameters,
            });
        }
    }

    fn map_to_generated(
        &self,
        source: &str,
        generated: &str,
        source_map: &oxc_sourcemap::SourceMap<'_>,
    ) -> Vec<TypeQueryPosition> {
        let source_lines = LineIndex::new(source);
        let generated_lines = LineIndex::new(generated);
        let tokens = source_map.get_tokens().collect::<Vec<_>>();
        let source_digest = sha1::Sha1::digest(source.as_bytes()).into();
        let mut positions = Vec::new();
        for function in &self.functions {
            let Some(function_start) = mapped_byte_offset(
                &tokens,
                source,
                generated,
                &source_lines,
                &generated_lines,
                function.function_start,
                function.function_end,
            ) else {
                continue;
            };
            for parameter in &function.parameters {
                positions.push(TypeQueryPosition {
                    function_start,
                    parameter_index: parameter.index,
                    source_position_utf16: source_lines.utf16_offset(source, parameter.start),
                    source_digest,
                });
            }
        }
        positions
    }
}

impl<'a> Visit<'a> for SourceParameters {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        if function.body.is_some() {
            self.record_function(function.span.start, function.span.end, &function.params);
        }
        walk::walk_function(self, function, flags);
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        self.record_function(arrow.span.start, arrow.span.end, &arrow.params);
        walk::walk_arrow_function_expression(self, arrow);
    }
}

struct LineIndex {
    byte_starts: Vec<usize>,
    utf16_starts: Vec<u32>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut byte_starts = vec![0];
        let mut utf16_starts = vec![0];
        let mut utf16_offset = 0;
        for (byte, character) in text.char_indices() {
            utf16_offset += character.len_utf16() as u32;
            if character == '\n' {
                byte_starts.push(byte + 1);
                utf16_starts.push(utf16_offset);
            }
        }
        Self {
            byte_starts,
            utf16_starts,
        }
    }

    fn line_column(&self, text: &str, byte_offset: u32) -> Option<(u32, u32)> {
        let byte_offset = byte_offset as usize;
        let line = self
            .byte_starts
            .partition_point(|start| *start <= byte_offset)
            .checked_sub(1)?;
        let column = text
            .get(self.byte_starts[line]..byte_offset)?
            .encode_utf16()
            .count();
        Some((line as u32, column as u32))
    }

    fn utf16_offset(&self, text: &str, byte_offset: u32) -> u32 {
        let byte_offset = byte_offset as usize;
        let line = self
            .byte_starts
            .partition_point(|start| *start <= byte_offset)
            .saturating_sub(1);
        self.utf16_starts[line]
            + text[self.byte_starts[line]..byte_offset]
                .encode_utf16()
                .count() as u32
    }

    fn byte_offset(&self, text: &str, line: u32, column: u32) -> Option<u32> {
        let start = *self.byte_starts.get(line as usize)?;
        let suffix = text.get(start..)?;
        let mut utf16_column = 0;
        for (relative_byte, character) in suffix.char_indices() {
            if utf16_column == column {
                return u32::try_from(start + relative_byte).ok();
            }
            if character == '\n' || utf16_column + character.len_utf16() as u32 > column {
                return None;
            }
            utf16_column += character.len_utf16() as u32;
        }
        (utf16_column == column)
            .then_some(suffix.len() + start)
            .and_then(|offset| u32::try_from(offset).ok())
    }
}

fn mapped_byte_offset(
    tokens: &[oxc_sourcemap::Token],
    source: &str,
    generated: &str,
    source_lines: &LineIndex,
    generated_lines: &LineIndex,
    source_offset: u32,
    source_end: u32,
) -> Option<u32> {
    let (source_line, source_column) = source_lines.line_column(source, source_offset)?;
    let token = tokens
        .iter()
        .filter(|token| token.get_source_id() == Some(0))
        .filter(|token| token.get_src_line() == source_line)
        .filter_map(|token| {
            let offset =
                source_lines.byte_offset(source, token.get_src_line(), token.get_src_col())?;
            (offset >= source_offset && offset < source_end).then_some((token, offset))
        })
        .min_by_key(|(_, offset)| offset.abs_diff(source_offset))
        .map(|(token, _)| token)?;
    let token_source =
        source_lines.byte_offset(source, token.get_src_line(), token.get_src_col())?;
    let (_, token_source_column) = source_lines.line_column(source, token_source)?;
    let generated_column =
        i64::from(token.get_dst_col()) + i64::from(source_column) - i64::from(token_source_column);
    let generated_column = u32::try_from(generated_column).ok()?;
    generated_lines.byte_offset(generated, token.get_dst_line(), generated_column)
}

fn format_diagnostics(
    path: &Path,
    diagnostics: impl IntoIterator<Item = impl std::fmt::Display>,
) -> String {
    let details = diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    format!("{}: {details}", path.display())
}
