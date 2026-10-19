// quench-node serve.cjs SCENARIO : start a framework scenario server and report its port.
const app = require(require('node:path').resolve(process.argv[2]));
app.start().then(({ port }) => console.log('PORT ' + port));
