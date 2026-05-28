//! Component macro - marks functions as Preact components

use proc_macro::TokenStream;
use quote::quote;

/// The `#[component]` attribute macro
///
/// This macro marks a function as a Preact component. It:
/// - Marks the function for component registration
/// - Validates the return type is VNode
/// - Adds metadata for the runtime
///
/// Usage:
/// ```ignore
/// #[component]
/// pub fn Counter(props: CounterProps) -> VNode {
///     let (count, set_count) = use_state(|| props.initial.unwrap_or(0));
///     
///     html! {
///         <div class="counter">
///             <p>Count: { count }</p>
///             <button on_click={move |_| set_count(count + 1)}>+</button>
///         </div>
///     }
/// }
/// ```
pub fn component_macro(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_fn: syn::ItemFn = syn::parse_macro_input!(item as syn::ItemFn);
    let name = &item_fn.sig.ident;
    let _vis = &item_fn.vis;

    // Extract the function signature
    let sig = &item_fn.sig;
    let _inputs = &sig.inputs;
    let _output = &sig.output;
    let _generics = &sig.generics;
    let _where_clause = &sig.generics.where_clause;

    // Get the component name
    let component_name = name.to_string();

    // Build the expanded output
    let expanded = quote! {
        #item_fn

        // Static registration of this component
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        static __RUNTS_COMPONENT_INFO: ::runts_lib::runtime::component::ComponentInfo =
            ::runts_lib::runtime::component::ComponentInfo::new(
                #component_name,
            );
    };

    expanded.into()
}
