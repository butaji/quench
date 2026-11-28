//! The AOT facts solver's closed-world tests (`compile::aot_facts`): call targets, class layouts,
//! index bounds, reads and identities, and the soundness regressions. An integration test, so the
//! layer crates are linked in (`extern crate quench_layers`) and the solver's items are reached
//! through the `test-support` feature.

extern crate quench_layers;

#[path = "aot_facts/bounds.rs"]
mod bounds;
#[path = "aot_facts/calls.rs"]
mod calls;
#[path = "aot_facts/identities.rs"]
mod identities;
#[path = "aot_facts/layouts.rs"]
mod layouts;
#[path = "aot_facts/reads.rs"]
mod reads;
#[path = "aot_facts/soundness.rs"]
mod soundness;
#[path = "aot_facts/support.rs"]
mod support;
