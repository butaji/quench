// Layout training workload (tools/bolt-layout.sh): the npm package under QUENCH_LAYOUT_NODE_MODULES.
const mm = require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/minimatch'); const minimatch = mm.minimatch || mm;
const dirs = ['src', 'lib', 'test', 'docs', 'node_modules/pkg', 'a/b/c']; const exts = ['js', 'ts', 'json', 'md', 'cjs', 'mjs', 'd.ts'];
const files = []; for (const d of dirs) for (const e of exts) for (let i = 0; i < 12; i++) files.push(d + '/file' + i + (i % 4 ? '' : '.spec') + '.' + e);
const pats = ['**/*.js', 'src/**/*.{ts,tsx}', '!(node_modules)/**', '**/*.spec.*', 'lib/file[0-5].*', '{docs,test}/**/*.md', '**/?(*.)d.ts', 'a/**/c/*'];
let n = 0; for (const p of pats) for (const f of files) if (minimatch(f, p, { dot: true })) n++; console.log(n, files.length);
