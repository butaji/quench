You need to work with Ink examples/ to cover most of Ink features. If you feel like to add more examples, do it.

Examples can contain only ts/tsx, can not have any rust code.

runts -- to work with tx/tsx

runts plugins/crates -- glue to frameworks

You have to reach 100% look&feel parity on each Ink example in 3 enviroments:
1) deno
2) runts dev (quickjs or HIR runtime with hot-reload, not compilation at all)
3) runts compile (ts/tsx transpile to rust in-memory, and then compile)

task=commit + push
