//! Data-oriented layout rules, counted per file outside tests:
//! - `dod-bool-array`: an array, slice or vector of `bool` (`vec![false; n]` included), which
//!   belongs in a bitset: one bit per flag, scanned a word at a time;
//! - `dod-dense-hash`: a hashed or tree map/set keyed by an integer or id type, which belongs
//!   in a flat array indexed by that id, with frequent flags packed into the index's high bits;
//! - `dod-cache`: a field or static named as a cache or memo, which hides a slow core path:
//!   make the core path cheap instead.
//!
//! The counts join the shrink-only measures in `oversized.txt`: a new use fails, an old one
//! may only go away.

use quote::ToTokens;
use syn::visit::Visit;

use crate::shape::Measures;

const HASHED: &[&str] = &["HashMap", "HashSet", "BTreeMap", "BTreeSet", "FxHashMap", "FxHashSet", "IndexMap", "IndexSet"];
/// Key types that are dense indices: integers and the engine's interned ids.
const DENSE_KEYS: &[&str] = &["u8", "u16", "u32", "u64", "usize", "Atom", "ProgramId", "ShapeId", "FunctionId"];
const CACHE_WORDS: &[&str] = &["cache", "memo"];
const BOOL_CONTAINERS: &[&str] = &["Vec", "SmallVec", "Box", "Rc", "Arc"];

pub fn scan(file: &str, syntax: &syn::File, measures: &mut Measures) {
    let mut layout = Layout::default();
    layout.visit_file(syntax);
    for (rule, count) in [("dod-bool-array", layout.bools), ("dod-dense-hash", layout.dense), ("dod-cache", layout.caches)] {
        if count > 0 {
            measures.insert(format!("{rule} {file}"), count);
        }
    }
}

#[derive(Default)]
struct Layout {
    bools: usize,
    dense: usize,
    caches: usize,
}

fn is_bool(ty: &syn::Type) -> bool {
    matches!(ty, syn::Type::Path(path) if path.path.is_ident("bool"))
}

fn first_type_argument(segment: &syn::PathSegment) -> Option<&syn::Type> {
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

/// Whether a container's element is `bool`, directly or as `[bool; N]` / `[bool]`.
fn holds_bools(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Array(array) => is_bool(&array.elem),
        syn::Type::Slice(slice) => is_bool(&slice.elem),
        _ => is_bool(ty),
    }
}

fn is_dense_key(ty: &syn::Type) -> bool {
    matches!(ty, syn::Type::Path(path)
        if path.path.segments.last().is_some_and(|last| DENSE_KEYS.iter().any(|key| last.ident == key)))
}

fn names_cache(ident: &syn::Ident) -> bool {
    let name = ident.to_string().to_lowercase();
    CACHE_WORDS.iter().any(|word| name.contains(word))
}

impl<'ast> Visit<'ast> for Layout {
    fn visit_type(&mut self, ty: &'ast syn::Type) {
        match ty {
            syn::Type::Array(array) if is_bool(&array.elem) => self.bools += 1,
            syn::Type::Path(path) => {
                if let Some(last) = path.path.segments.last()
                    && let Some(argument) = first_type_argument(last)
                {
                    let containers_bools = BOOL_CONTAINERS.iter().any(|name| last.ident == name) && holds_bools(argument);
                    self.bools += usize::from(containers_bools);
                    self.dense += usize::from(HASHED.iter().any(|name| last.ident == name) && is_dense_key(argument));
                }
            }
            _ => {}
        }
        syn::visit::visit_type(self, ty);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let tokens = mac.tokens.to_string();
        let bool_fill = tokens.starts_with("false ;") || tokens.starts_with("true ;");
        self.bools += usize::from(mac.path.is_ident("vec") && bool_fill);
        syn::visit::visit_macro(self, mac);
    }

    fn visit_field(&mut self, field: &'ast syn::Field) {
        self.caches += usize::from(field.ident.as_ref().is_some_and(names_cache));
        syn::visit::visit_field(self, field);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.caches += usize::from(names_cache(&item.ident));
        syn::visit::visit_item_static(self, item);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !crate::shape::is_test(&item.attrs) {
            syn::visit::visit_item_mod(self, item);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !crate::shape::is_test(&item.attrs) {
            syn::visit::visit_item_fn(self, item);
        }
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        // `thread_local!` and other item macros declare statics in their tokens.
        let tokens = format!(" {}", item.mac.tokens.to_token_stream()).to_lowercase();
        let declares = tokens.split(" static ").skip(1).filter(|rest| {
            rest.split([':', ' ']).next().is_some_and(|name| CACHE_WORDS.iter().any(|word| name.contains(word)))
        });
        self.caches += declares.count();
        syn::visit::visit_item_macro(self, item);
    }
}
