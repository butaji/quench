Confirmed remaining probe work/private-eval-environments-{node,quench}.stdout: instance-method eval of a static private name, ordinary nested-function eval of an instance private name, and static-method eval of an instance private name fail. Two same-home cases pass. Do not call the current 13-row storage matrix complete private-context parity.

Root limitation: private declarations are split over public prototype/constructor homes and eval context is conflated with super_home_atom. Eval should inherit the lexical private environment, independently of [[HomeObject]]/super permission, and each private name must retain its actual declaring environment.

Candidate next model to verify against source/spec:
- Allocate one opaque private environment per class evaluation, before any heritage/body code where private names are in lexical scope. Use its identity + logical Atom for private brands/data. Keep implementation values/declarations on that inaccessible environment rather than public prototypes. This also avoids falsely branding Class.prototype and treating static fields as initialized during declaration.
- Give BcFunction explicit ordered lexical private-environment binding metadata, propagated to methods, arrows and ordinary nested functions. Register actual capture references in NameBindings. Keep super_home_atom separate. Runtime private lookup resolves these declared bindings, not arbitrary sibling-home scanning or receiver prototypes.
- Eval projects nearest visible labels to (logical identity, actual private environment), preserving nested-class shadowing; install each name's real environment instead of one super-home value for all names.
- OXC contextual eval wrapping must support private syntax with super disallowed in ordinary functions and new.target disallowed in script contexts. Use AST/semantic context checks, never string grammar policy.
- Explicit private get/set/definition operations are the eventual boundary separating language-private access from public strings; runtime/internal marker migration remains its own namespace work. Bytecode must be bumped again after publishing format 92.

Avoid implementing this sketch without reading the current compiler/eval structure and checking early class-heritage scope against Node/OXC. The current private data-storage fix is independently verified on its stated cases; broader lexical contexts stay open.
