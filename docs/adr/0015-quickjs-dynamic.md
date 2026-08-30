# Shared ownership for JavaScript and Wasm

The JavaScript layer and Wasm store share the runtime but keep their ownership
rules explicit. Dynamic JavaScript objects, atoms, shapes, and cycle handling
belong to the Dynamic runtime; Wasm instance data belongs to the shared store.

Crossings remain Guard and Box. Do not reintroduce a second JavaScript heap,
duplicate collector, or parallel value representation.
