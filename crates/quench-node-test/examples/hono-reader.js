// Smallest possible Hono-style reader + assertion. Pairs with
// `hono-server.js`. The reader checks the server's output and
// exits non-zero if any expected line is missing.

const fs = require('node:fs');
const readline = require('node:readline');

(async () => {
  const stream = fs.createReadStream(process.argv[2] || '/dev/null');
  const rl = readline.createInterface({ input: stream });
  const lines = [];
  for await (const line of rl) {
    lines.push(line);
  }
  const expected = [
    'hono-server: started',
    'hono-server: ok',
    'Buffer.from("hi") hi',
    'util.format hi there',
    'path.join /tmp/a/b.js',
    'os.platform',
    'net.isIP 4',
    'net.isIPv4 false',
  ];
  let ok = 0;
  for (const needle of expected) {
    if (lines.some((l) => l.includes(needle))) {
      ok += 1;
      console.log('reader: OK', needle);
    } else {
      console.log('reader: MISS', needle);
    }
  }
  console.log('reader: ok =', ok, '/', expected.length);
  if (ok !== expected.length) process.exit(1);
})();
