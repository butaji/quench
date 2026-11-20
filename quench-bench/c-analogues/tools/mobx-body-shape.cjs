// Counts the work the MobX bundle's module body does when it is evaluated (not when its functions are
// called). usage: node mobx-body-shape.cjs [bundle.js]   (default: the JetStream bundle.es6.min.js)
// A node "runs at body time" when no non-IIFE function encloses it: function objects (closures, class
// methods, getters/setters), classes, object and array literals, and `new` expressions.
'use strict';
const fs = require('fs');
const path = require('path');
const acorn = require('/opt/node-tools/node_modules/acorn');
const file = process.argv[2] || path.join(__dirname, '..', '..', 'jetstream', 'mobx', 'dist', 'bundle.es6.min.js');
const src = fs.readFileSync(file, 'utf8').replace(/\n\/\*ThouShaltNotCache\*\/\n/g, '\n/*0*/\n');
const ast = acorn.parse(src, { ecmaVersion: 'latest', sourceType: 'script', allowReturnOutsideFunction: true });
const counts = { functions: 0, arrows: 0, classes: 0, methods: 0, objects: 0, arrays: 0, news: 0, templates: 0 };
const iifeCallees = new WeakSet();
function mark(node) { // functions that are immediately invoked run once, so their bodies run at body time too
  if (node.type === 'CallExpression' && (node.callee.type === 'ArrowFunctionExpression' || node.callee.type === 'FunctionExpression')) iifeCallees.add(node.callee);
}
const stack = [];
function walk(node, bodyTime) {
  if (!node || typeof node.type !== 'string') return;
  mark(node);
  let here = bodyTime;
  if (node.type === 'FunctionDeclaration' || node.type === 'FunctionExpression' || node.type === 'ArrowFunctionExpression') {
    if (bodyTime) { counts.functions++; if (node.type === 'ArrowFunctionExpression') counts.arrows++; }
    here = bodyTime && iifeCallees.has(node);
  }
  if (bodyTime) {
    if (node.type === 'ClassDeclaration' || node.type === 'ClassExpression') counts.classes++;
    if (node.type === 'MethodDefinition' || node.type === 'Property' && node.method) counts.methods++;
    if (node.type === 'ObjectExpression') counts.objects++;
    if (node.type === 'ArrayExpression') counts.arrays++;
    if (node.type === 'NewExpression') counts.news++;
    if (node.type === 'TemplateLiteral') counts.templates++;
  }
  for (const key of Object.keys(node)) {
    const v = node[key];
    if (key === 'type') continue;
    if (Array.isArray(v)) for (const c of v) walk(c, here);
    else if (v && typeof v.type === 'string') walk(v, here);
  }
}
walk(ast, true);
console.log(JSON.stringify(counts));
