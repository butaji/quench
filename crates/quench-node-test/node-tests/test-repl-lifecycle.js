// Node compat: REPL prompt and close lifecycle.
const repl = require('node:repl');
const writes = [];
let closed = 0;
const server = repl.start({
  prompt: 'first> ',
  output: { write(value) { writes.push(String(value)); } }
});
if (!(server instanceof repl.REPLServer)) throw new Error('REPLServer instance');
server.on('close', () => { closed += 1; });
server.prompt();
server.setPrompt('second> ');
server.prompt();
if (writes.join('|') !== 'first> |second> ') throw new Error('prompt output: ' + writes.join('|'));
if (server.close() !== undefined || closed !== 1) throw new Error('close lifecycle');
server.prompt();
if (writes.length !== 2) throw new Error('prompt after close');
console.log('repl lifecycle: ok');
