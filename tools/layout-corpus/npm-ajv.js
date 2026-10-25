// Layout training workload (tools/bolt-layout.sh): the npm package under QUENCH_LAYOUT_NODE_MODULES.
const Ajv = require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/ajv');
const ajv = new Ajv();
const validate = ajv.compile({ type: 'object', properties: { a: { type: 'integer' }, b: { type: 'string', maxLength: 5 } }, required: ['a'] });
console.log(validate({ a: 1, b: 'x' }), validate({ b: 'too long' }));
