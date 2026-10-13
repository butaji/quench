# Shared array operations

`Array.prototype.shift` uses the shared VM builtin in every execution mode. Its dense path applies only to an ordinary extensible array with writable length, no mapped arguments or indexed descriptors, no sparse length, and a present own data element at every index below the observed length. These facts prove that all reads and writes address ordinary own data properties, deletion succeeds, and prototype getters or setters cannot run.

The operation mutates the existing copy-on-write element storage through its canonical mutation boundary. Moving existing values within the same array introduces no new collector edges. Removing the first element updates the intrinsic dense length; no guest callback or allocation occurs between removal and returning the value. The result remains the original value, including object identity and explicit `undefined` elements.

Receivers outside the guard use the original ordered property algorithm. Holes and prototype properties, proxies, indexed accessors and attributes, mapped arguments, readonly length, sealed/frozen arrays, and ordinary array-like objects retain their original effects, exceptions, and partial mutations. No stream, fixture, benchmark, or source identity participates in dispatch.

The specialized and unspecialized regression `regression_array_shift_guards_preserve_order_and_copy_on_write` checks identity, literal storage isolation, inherited getter/setter order, own accessors, readonly-length partial mutation, integrity constraints, ordinary objects, and mapped arguments. Node stream fixture deadlines and iteration benchmark diagnostics do not establish a production performance claim.
