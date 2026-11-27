//! One concept, one definition (AGENTS.md redundancy laws): each engine concept outside opcode
//! meaning (value word tests, number conversions, string order, typeof, decoders) has one home,
//! and every tier calls it. Two measures join the shrink-only list in `oversized.txt`:
//! - `concept_twin <concept>`: definitions of the concept outside its homes, found by the names
//!   its definitions take or by the code that spells it out (whitespace-free token text). The
//!   stencil crate's copies, which cannot import the runtime and are held to it by layout tests,
//!   are homes.
//! - `concept_runs <analysis>`: production call sites that run a program analysis from scratch
//!   (a liveness solve, a successor table, a back-edge scan). Each is a recomputation of a fact
//!   that should be derived once per function and read everywhere else.
//!
//! - The projection layers (`call-row`, `inliner`, `specializer`, `hand-clif`, `hand-stencil`, `stitch`):
//!   the interpreter defines WHAT, b-jit (rustc/LLVM) and u-jit (Cranelift) define HOW and share
//!   every layer but the last, so a second call path, inliner or specializer, or semantics
//!   lowered by hand for one backend, is a twin.
//!
//! A merge that removes a twin or a run shrinks the measure; a new one fails the lint.

use std::{fs, path::PathBuf};

use quote::ToTokens;
use syn::visit::Visit;

use crate::shape::{Measures, is_test};

/// How a definition of a concept is recognized.
enum Mark {
    /// A function item of this name.
    Function(&'static str),
    /// This token text (whitespace removed) in a production item.
    Spelled(&'static str),
    /// A function item whose name contains one of these parts.
    Named(&'static [&'static str]),
}

/// A concept, the files that may define it, and how its definitions are recognized.
struct Concept {
    name: &'static str,
    homes: &'static [&'static str],
    marks: &'static [Mark],
}

/// The stencil sources, which compile without the runtime and so keep checked copies.
const STENCILS: &str = "quench-bjit/stencils/";
/// This lint's own sources, which spell every mark as data.
const LINT_SOURCES: &str = "quench-lint/src/";

const CONCEPTS: &[Concept] = &[
    Concept {
        name: "value-tag-test",
        homes: &["quench-runtime/src/value.rs", "quench-runtime/src/staged/value.rs", STENCILS],
        marks: &[Mark::Spelled("ValueLayout::INT_TAG"), Mark::Spelled("ValueLayout::TAG_MASK"), Mark::Spelled("ValueLayout::HEAP_TAG")],
    },
    Concept {
        name: "to-int32",
        homes: &["quench-runtime/src/value/number_conversion.rs", "quench-runtime/src/lib.rs", "quench-runtime/src/staged/numeric.rs", "quench-runtime-engine/src/compiled/object_slots/emit.rs", STENCILS],
        marks: &[Mark::Function("to_int32"), Mark::Function("to_uint32"), Mark::Function("number_to_u32"), Mark::Function("number_to_i32"), Mark::Function("value_to_int32")],
    },
    Concept {
        name: "to-uint8-clamp",
        homes: &["quench-runtime/src/value/number_conversion.rs", "quench-runtime/src/staged/", "quench-runtime-engine/src/compiled/object_slots/emit.rs", STENCILS],
        marks: &[Mark::Function("clamp_u8"), Mark::Function("uint8_clamped_from_value"), Mark::Function("number_to_uint8_clamp"), Mark::Spelled("round_ties_even()asu8")],
    },
    Concept {
        name: "string-order",
        homes: &["quench-runtime/src/vm/wtf16"],
        marks: &[Mark::Spelled(".units().cmp(")],
    },
    Concept {
        name: "typeof-classifier",
        homes: &["quench-runtime/src/staged/value.rs", "quench-runtime/src/bytecode/typeof_names.rs", STENCILS],
        marks: &[Mark::Spelled("Value::TRUE.to_bits()&ValueLayout::TAG_MASK"), Mark::Function("typeof_name_index")],
    },
    Concept {
        name: "length-int32-guard",
        homes: &["quench-runtime/src/staged/value.rs"],
        marks: &[Mark::Function("length_value")],
    },
    Concept {
        name: "math-imul",
        homes: &["quench-runtime/src/staged/int_math.rs"],
        marks: &[Mark::Function("int_multiply"), Mark::Spelled("asi32).wrapping_mul(")],
    },
    Concept {
        name: "packed-instruction-decode",
        homes: &["quench-runtime/src/bytecode/control_flow.rs", "quench-runtime/src/bytecode.rs"],
        marks: &[Mark::Function("decoded_instruction"), Mark::Function("instruction_at"), Mark::Function("widened"), Mark::Spelled(".wide_index()]")],
    },
    // The projection design (Linear "Projection design: interpreter → b-jit (rustc/LLVM) → u-jit
    // (Cranelift)"): the interpreter's staged rows define WHAT, the JITs define HOW, and both JITs
    // share every layer but the last. Each concept below is a layer that must have one home; every
    // other definition is a second copy, held shrink-only so none can be added.
    Concept {
        // One call row: `staged::invoke` and the protocol it composes. Every other function that
        // makes an activation is a second call path.
        name: "call-row",
        homes: &["quench-runtime/src/staged/invoke.rs", "quench-runtime/src/staged/protocol.rs", "quench-runtime/src/staged/call_body.rs"],
        marks: &[Mark::Named(&["activate", "push_frame", "push_plain", "push_activation", "push_callback", "push_body_header", "push_native", "construct_frame", "call_user"])],
    },
    Concept {
        // One inliner, over staged `invoke` and `ret`, whose spliced unit both JITs consume.
        name: "inliner",
        homes: &["quench-runtime/src/staged/inline"],
        marks: &[Mark::Named(&["inline_call", "inline_callee", "inline_body", "inlined_unit", "inline_unit", "splice_callee"])],
    },
    Concept {
        // One specializer driver over staged operations, shared by both JITs.
        name: "specializer",
        homes: &["quench-runtime/src/vm/specializer.rs", "quench-runtime-engine/src/compiled/admission.rs"],
        marks: &[Mark::Named(&["specialize"])],
    },
    Concept {
        // One reading of the JIT mode: the specializer decides what each tier may do
        // (`admission::tier_runs`, `stencil_only`); a tier that reads the mode itself decides again.
        name: "mode-read",
        homes: &["quench-runtime-engine/src/compiled/admission.rs", "quench-runtime/src/jit_mode.rs", "quench-runtime/src/vm/tier_policy.rs"],
        marks: &[Mark::Spelled("jit_mode::"), Mark::Spelled("MemoryFirst"), Mark::Spelled("tiers_allowed("), Mark::Spelled("base_only("), Mark::Spelled("tier_may_run(")],
    },
    Concept {
        // u-jit HOW is the Cranelift stage of the staged rows; CLIF built by hand elsewhere is
        // semantics the rows do not define.
        name: "hand-clif",
        homes: &["quench-runtime/src/staged/"],
        marks: &[Mark::Spelled(".ins().")],
    },
    Concept {
        // b-jit HOW is rustc/LLVM compiling the staged rows; a stencil entry written by hand is
        // semantics the rows do not define.
        name: "hand-stencil",
        homes: &["quench-bjit/stencils/staged.rs", "quench-bjit/stencils/staged_rows.rs", "quench-bjit/stencils/entry"],
        marks: &[Mark::Spelled("extern\"rust-preserve-none\"fn")],
    },
    Concept {
        // One stitcher places every b-jit family's rows (JS, Wasm, RegExp): a family that copies
        // and patches stencils itself places them a second way.
        name: "stitch",
        homes: &["quench-runtime-engine/src/compiled/stitcher.rs", "quench-bjit/src/"],
        marks: &[Mark::Spelled("copy_patch("), Mark::Spelled("patch_successor(")],
    },
];

/// Analyses whose every production call recomputes a per-function fact.
const ANALYSES: &[(&str, &[&str])] = &[
    ("liveness-solve", &["liveness::analyze("]),
    ("liveness-graph-solve", &["analyze_graph("]),
    ("successor-table", &["Successors::of_code("]),
    ("back-edge-scan", &["has_backward_branch("]),
    ("lane-leaders", &["=leaders(", "::leaders("]),
];

/// Records the concept measures of the production files among `files`.
pub fn scan(files: &[PathBuf], measures: &mut Measures) {
    let mut found = Found::default();
    for path in files {
        let file = path.display().to_string();
        if crate::in_test_path(&file) || file.contains(LINT_SOURCES) {
            continue;
        }
        let Some(syntax) = fs::read_to_string(path).ok().and_then(|source| syn::parse_file(&source).ok()) else { continue };
        found.read(&file, &syntax);
    }
    for (concept, count) in CONCEPTS.iter().zip(found.twins) {
        measures.insert(format!("concept_twin {}", concept.name), count);
    }
    for ((analysis, _), count) in ANALYSES.iter().zip(found.runs) {
        measures.insert(format!("concept_runs {analysis}"), count);
    }
}

/// The counts so far, in the order of `CONCEPTS` and `ANALYSES`.
struct Found {
    twins: Vec<usize>,
    runs: Vec<usize>,
}

impl Default for Found {
    fn default() -> Self {
        Self { twins: vec![0; CONCEPTS.len()], runs: vec![0; ANALYSES.len()] }
    }
}

impl Found {
    fn read(&mut self, file: &str, syntax: &syn::File) {
        let mut items = Items::default();
        items.visit_file(syntax);
        for (concept, twins) in CONCEPTS.iter().zip(self.twins.iter_mut()) {
            if !concept.homes.iter().any(|home| file.contains(home)) {
                *twins += concept.marks.iter().map(|mark| items.count(mark)).sum::<usize>();
            }
        }
        for ((_, calls), runs) in ANALYSES.iter().zip(self.runs.iter_mut()) {
            *runs += calls.iter().map(|call| items.text.matches(call).count()).sum::<usize>();
        }
    }
}

/// A file's production items: the names of its functions and its token text.
#[derive(Default)]
struct Items {
    functions: Vec<String>,
    text: String,
}

impl Items {
    fn count(&self, mark: &Mark) -> usize {
        match mark {
            Mark::Function(name) => self.functions.iter().filter(|function| function == name).count(),
            Mark::Spelled(text) => self.text.matches(text).count(),
            Mark::Named(parts) => self.functions.iter().filter(|function| parts.iter().any(|part| function.contains(part))).count(),
        }
    }

    fn take(&mut self, tokens: impl ToTokens) {
        self.text.extend(tokens.to_token_stream().to_string().chars().filter(|unit| !unit.is_whitespace()));
    }
}

impl<'ast> Visit<'ast> for Items {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs = match item {
            syn::Item::Fn(function) => &function.attrs,
            syn::Item::Mod(module) => &module.attrs,
            syn::Item::Impl(block) => &block.attrs,
            _ => return self.take(item),
        };
        if is_test(attrs) {
            return;
        }
        match item {
            syn::Item::Fn(function) => {
                self.functions.push(function.sig.ident.to_string());
                self.take(function);
            }
            _ => syn::visit::visit_item(self, item),
        }
    }

    fn visit_impl_item_fn(&mut self, function: &'ast syn::ImplItemFn) {
        if !is_test(&function.attrs) {
            self.functions.push(function.sig.ident.to_string());
            self.take(function);
        }
    }

    fn visit_trait_item_fn(&mut self, function: &'ast syn::TraitItemFn) {
        if function.default.is_some() {
            self.functions.push(function.sig.ident.to_string());
            self.take(function);
        }
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        self.take(item);
    }
}
