# Interpreter-only adaptive specialization trial

Each bytecode instruction has a bounded one-byte sidecar state owned by the
`Vm`, not the immutable `ResidualProgram`. A generic `Binary` site counts
consecutive integer operand pairs. At eight observations it transitions to
`IntStable`; that state calls the integer operation directly after guards.

If either guard fails, the site resets to generic and executes the complete
coercing operation. Thus feedback can change performance, never semantics.
State is indexed only by `(function, pc)`, has no guest names or values, and
cannot describe more than one instruction. There is no whole-function plan,
native code, or domain-specific state.

This sidecar is a measured trial. It should remain only if avoiding generic
dispatch outweighs the counter lookup and guard cost on the parity workload;
otherwise the code is removed and the negative result recorded in task 14.
