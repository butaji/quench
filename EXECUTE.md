You need to work with Ink examples/ to cover most of Ink features. If you feel like to add more examples, do it.

Examples can contain only ts/tsx, can not have any rust code.

runts -- to work with tx/tsx

runts plugins/crates -- glue to frameworks

You have to reach 100% look&feel parity on each Ink example in 3 enviroments:
1) deno
2) runts dev (quickjs or HIR runtime with hot-reload, not compilation at all)
3) runts compile (ts/tsx transpile to rust in-memory, and then compile)

Parity test must be run by a single script, harnessing all the executing, tracing TUI/CLI apps output to files, and providing per symbol diff results.

If HIR or HIR runtime doesnt support something to be compatible with Ink, you have to implement it.

All the changes and complicated sections must be covered with unit-tests. High test coverage is a requirement.

task=commit + push

before starting make a code and architecture review, track tasks in tasks/index.json and tasks/xxx.md per each task with descriptions

Ultimate goal: 100% matching of comprehensive set of Ink/ts/tsx examples to cover all of its features among all 3 platforms
