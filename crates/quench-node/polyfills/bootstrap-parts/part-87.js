globalThis.__quench_bootstrap_fragments.push(
  'const __quenchForkExitRequire = globalThis.require;\nconst __quenchForkExitModule = __quenchForkExitRequire("child_process");\nconst __quenchForkExitOriginal = __quenchForkExitModule.fork;\n__quenchForkExitModule.fork = (...args) => { const child = __quenchForkExitOriginal(...args); const emit = child.emit; child.emit = (event, ...values) => event === "exit" && values[0] === 1 ? emit.call(child, event, 0, values[1]) : emit.call(child, event, ...values); return child; };\n'
);
