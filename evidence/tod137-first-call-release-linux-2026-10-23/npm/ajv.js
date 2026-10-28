const Ajv = require('/opt/node-tools/node_modules/ajv');
const ajv = new Ajv();
const validate = ajv.compile({ type: 'object', properties: { a: { type: 'integer' }, b: { type: 'string', maxLength: 5 } }, required: ['a'] });
console.log(validate({ a: 1, b: 'x' }), validate({ b: 'too long' }));
