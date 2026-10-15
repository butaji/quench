import {exec} from '/workspace/quench/tests/npm-top300/node_modules/tinyexec/dist/main.mjs';
const result = await exec(process.execPath, ['-e', 'console.log(42)']);
console.log(JSON.stringify({ stdout: result.stdout, stderr: result.stderr, exitCode: result.exitCode }));
