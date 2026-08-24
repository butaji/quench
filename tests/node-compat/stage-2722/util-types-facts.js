'use strict';

const assert = require('assert');
const { types } = require('util');
const vm = require('vm');

const argumentsObject = (function() { return arguments; })(1);
assert.strictEqual(types.isArgumentsObject(argumentsObject), true);
assert.strictEqual(types.isArgumentsObject({}), false);

const float16 = new Float16Array();
assert.strictEqual(types.isFloat16Array(float16), true);
assert.strictEqual(types.isUint16Array(float16), false);
assert.strictEqual(types.isUint16Array(new Uint16Array()), true);

const sourceModule = new vm.SourceTextModule('');
assert.strictEqual(types.isModuleNamespaceObject(sourceModule.namespace), true);
