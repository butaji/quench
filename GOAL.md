# Goal: stages 48–50

Bring test262 stages 48–50 to 100% passing through the canonical runner:
Function, GeneratorFunction, and GeneratorPrototype. Preserve callable and
constructable behavior, strictness, own-property descriptors, prototype and
realm identity, generator creation, suspension, resumption, and completion
semantics. Fix shared runtime mechanisms, with no harness or test262 edits.
Re-run stages 48–50 and earlier regressions after every fix; finish clean and
commit verified changes.
