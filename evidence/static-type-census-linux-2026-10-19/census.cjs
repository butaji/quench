// Static census of type-predictable JS patterns. Usage: quench-node census.cjs file...
const acorn = require('/home/user/quench/work/web-upstream/node_modules/acorn');
const fs = require('fs');
function walk(node, visit, parent) {
  if (!node || typeof node.type !== 'string') return;
  visit(node, parent);
  for (const key of Object.keys(node)) {
    if (key === 'parent') continue;
    const child = node[key];
    if (Array.isArray(child)) { for (const c of child) if (c && typeof c.type === 'string') { c.parent = node; walk(c, visit, node); } }
    else if (child && typeof child.type === 'string') { child.parent = node; walk(child, visit, node); }
  }
}
const isLit = (n) => n && (n.type === 'Literal' || (n.type === 'TemplateLiteral' && n.expressions.length === 0) || (n.type === 'UnaryExpression' && n.operator === '-' && n.argument.type === 'Literal'));
function census(file) {
  const src = fs.readFileSync(file, 'utf8');
  const ast = acorn.parse(src, { ecmaVersion: 'latest', sourceType: 'module', allowHashBang: true, allowReturnOutsideFunction: true });
  const c = {};
  const inc = (k, n = 1) => { c[k] = (c[k] || 0) + n; };
  const assigned = new Map(); // name -> assignment count (approximate, by identifier name)
  const declKind = new Map(); // name -> 'class' | 'function' | 'const-fn' | 'const-lit'
  walk(ast, (n) => {
    switch (n.type) {
      case 'AssignmentExpression': if (n.left.type === 'Identifier') assigned.set(n.left.name, (assigned.get(n.left.name) || 0) + 1); break;
      case 'UpdateExpression': if (n.argument.type === 'Identifier') assigned.set(n.argument.name, (assigned.get(n.argument.name) || 0) + 1); break;
      case 'ClassDeclaration': if (n.id) declKind.set(n.id.name, 'class'); break;
      case 'FunctionDeclaration': if (n.id) declKind.set(n.id.name, 'function'); break;
      case 'VariableDeclarator':
        if (n.id.type === 'Identifier' && n.init) {
          const t = n.init.type;
          const k = (t === 'ArrowFunctionExpression' || t === 'FunctionExpression') ? 'fn' : t === 'ClassExpression' ? 'class' : isLit(n.init) ? 'lit' : t === 'ObjectExpression' ? 'object' : t === 'ArrayExpression' ? 'array' : t === 'NewExpression' ? 'new' : 'other';
          declKind.set(n.id.name, (n.parent.kind === 'const' ? 'const-' : n.parent.kind + '-') + k);
          inc(`binding.${n.parent.kind}.${k}`);
        }
        break;
    }
  });
  walk(ast, (n) => {
    switch (n.type) {
      case 'ClassDeclaration': case 'ClassExpression': {
        inc('class');
        if (n.superClass) inc('class.extends');
        const body = n.body.body;
        const fields = body.filter((m) => m.type === 'PropertyDefinition' && !m.static);
        if (fields.length) { inc('class.withFields'); inc('class.fields', fields.length); }
        inc('class.privateFields', fields.filter((f) => f.key.type === 'PrivateIdentifier').length);
        inc('class.methods', body.filter((m) => m.type === 'MethodDefinition' && m.kind === 'method').length);
        inc('class.accessors', body.filter((m) => m.type === 'MethodDefinition' && (m.kind === 'get' || m.kind === 'set')).length);
        inc('class.staticMembers', body.filter((m) => m.static).length);
        const ctor = body.find((m) => m.kind === 'constructor');
        if (ctor) {
          let straight = 0, total = 0;
          walk(ctor.value.body, (x) => { if (x.type === 'AssignmentExpression' && x.left.type === 'MemberExpression' && x.left.object.type === 'ThisExpression' && !x.left.computed) total++; });
          for (const s of ctor.value.body.body) if (s.type === 'ExpressionStatement' && s.expression.type === 'AssignmentExpression' && s.expression.left.type === 'MemberExpression' && s.expression.left.object.type === 'ThisExpression') straight++;
          inc('class.ctor.thisStores', total); inc('class.ctor.thisStoresStraightLine', straight);
        }
        break;
      }
      case 'FunctionDeclaration': case 'FunctionExpression': {
        // ES5 constructor: body stores this.x at top level and the name is used with `new`.
        const top = (n.body.body || []).filter((s) => s.type === 'ExpressionStatement' && s.expression.type === 'AssignmentExpression' && s.expression.left.type === 'MemberExpression' && s.expression.left.object.type === 'ThisExpression');
        if (top.length >= 2) { inc('es5ctorLike'); inc('es5ctorLike.thisStores', top.length); }
        if (n.params.some((p) => p.type === 'RestElement')) inc('fn.restParam');
        break;
      }
      case 'NewExpression': {
        inc('new');
        if (n.callee.type === 'Identifier') {
          const k = declKind.get(n.callee.name);
          if (k === 'class' || k === 'function' || k === 'const-class' || k === 'const-fn') inc(`new.knownLocal.${k}`);
          else if (['Map', 'Set', 'Error', 'TypeError', 'Promise', 'Array', 'Date', 'URL', 'WeakMap', 'RegExp', 'Uint8Array', 'TextEncoder', 'TextDecoder', 'Headers', 'Request', 'Response'].includes(n.callee.name)) inc(`new.builtin`);
          else inc('new.otherIdentifier');
        } else inc('new.member');
        break;
      }
      case 'CallExpression': {
        inc('call');
        const f = n.callee;
        if (f.type === 'Identifier') {
          const k = declKind.get(f.name);
          const reassigned = assigned.get(f.name) || 0;
          if ((k === 'function' || k === 'const-fn') && reassigned === 0) inc('call.knownLocalCallee');
          else if (k === 'function' || (k || '').endsWith('-fn')) inc('call.localCalleeReassignable');
          else inc('call.otherIdentifier');
        } else if (f.type === 'MemberExpression') {
          if (f.object.type === 'ThisExpression') inc('call.thisMethod');
          const name = !f.computed && f.property.name;
          if (name === 'call' || name === 'apply') inc('call.callApply');
          if (name === 'bind') inc('call.bind');
          if (f.object.type === 'Identifier' && ['JSON', 'Math', 'Object', 'Array', 'Number', 'String', 'Reflect', 'Buffer', 'Promise', 'Symbol'].includes(f.object.name)) inc(`call.builtinStatic`);
          if (['push', 'slice', 'indexOf', 'join', 'map', 'forEach', 'filter', 'split', 'replace', 'toLowerCase', 'charCodeAt', 'startsWith', 'endsWith', 'includes', 'trim', 'substring', 'test', 'exec', 'hasOwnProperty', 'concat', 'toString'].includes(name)) inc('call.builtinProtoMethodName');
        } else if (f.type === 'FunctionExpression' || f.type === 'ArrowFunctionExpression') inc('call.iife');
        if (n.arguments.some((a) => a.type === 'SpreadElement')) inc('call.spread');
        if (f.type === 'Identifier' && f.name === 'require' && n.arguments[0] && n.arguments[0].type === 'Literal') inc('require.literal');
        break;
      }
      case 'ObjectExpression': {
        inc('objectLiteral');
        const props = n.properties;
        const staticKeys = props.every((p) => p.type === 'Property' && !p.computed && p.kind === 'init');
        if (props.length === 0) inc('objectLiteral.empty');
        else if (staticKeys) { inc('objectLiteral.staticKeys'); inc(`objectLiteral.staticKeys.n${Math.min(props.length, 8)}`); }
        else inc('objectLiteral.dynamic');
        if (props.some((p) => p.type === 'Property' && (p.kind === 'get' || p.kind === 'set'))) inc('objectLiteral.accessors');
        if (props.some((p) => p.type === 'SpreadElement')) inc('objectLiteral.spread');
        break;
      }
      case 'ArrayExpression': {
        inc('arrayLiteral');
        const els = n.elements;
        if (els.length && els.every((e) => e && e.type === 'Literal' && typeof e.value === 'number')) inc('arrayLiteral.allNumbers');
        else if (els.length && els.every((e) => e && e.type === 'Literal' && typeof e.value === 'string')) inc('arrayLiteral.allStrings');
        else if (els.length && els.every(isLit)) inc('arrayLiteral.allLiterals');
        break;
      }
      case 'BinaryExpression': {
        const op = n.operator;
        if (op === '|' || op === '>>>' || op === '>>' || op === '&' || op === '<<' || op === '^') inc('binary.int32Producing');
        if ((op === '===' || op === '!==' || op === '==' || op === '!=') && ((n.left.type === 'UnaryExpression' && n.left.operator === 'typeof') || (n.right.type === 'UnaryExpression' && n.right.operator === 'typeof'))) inc('guard.typeof');
        if (op === 'instanceof') inc('guard.instanceof');
        if ((op === '===' || op === '!==') && ((n.left.type === 'Identifier' && n.left.name === 'undefined') || (n.right.type === 'Identifier' && n.right.name === 'undefined') || isLit(n.left) && n.left.value === null || isLit(n.right) && n.right.value === null)) inc('guard.nullish');
        if (op === '+' && (n.left.type === 'Literal' && typeof n.left.value === 'string' || n.right.type === 'Literal' && typeof n.right.value === 'string')) inc('binary.stringConcatWithLiteral');
        if (op === 'in') inc('binary.in');
        break;
      }
      case 'TemplateLiteral': if (n.expressions.length) inc('templateLiteral'); break;
      case 'ForOfStatement': inc('forOf'); break;
      case 'ForStatement': {
        const init = n.init;
        if (init && init.type === 'VariableDeclaration' && init.declarations.length >= 1 && isLit(init.declarations[0].init) && typeof init.declarations[0].init.value === 'number') inc('for.numericInduction');
        break;
      }
      case 'ForInStatement': inc('forIn'); break;
      case 'ObjectPattern': inc('destructure.object'); break;
      case 'ArrayPattern': inc('destructure.array'); break;
      case 'MemberExpression': {
        inc('member');
        if (n.object.type === 'ThisExpression') inc('member.this');
        if (n.computed && n.property.type === 'Literal' && typeof n.property.value === 'string') inc('member.computedStringLiteral');
        if (n.property.type === 'PrivateIdentifier') inc('member.private');
        if (!n.computed && n.property.name === 'length') inc('member.length');
        break;
      }
      case 'SwitchStatement': if (n.cases.length >= 4 && n.cases.every((k) => !k.test || (k.test.type === 'Literal' && typeof k.test.value === 'string'))) inc('switch.stringCases4plus'); break;
      case 'ArrowFunctionExpression': inc('arrow'); break;
      case 'ReturnStatement': if (n.argument && n.argument.type === 'ObjectExpression') inc('return.objectLiteral'); if (n.argument && n.argument.type === 'ArrayExpression') inc('return.arrayLiteral'); break;
      case 'Identifier': if (n.name === 'arguments') inc('arguments'); break;
    }
  });
  return { bytes: src.length, ...Object.fromEntries(Object.entries(c).sort()) };
}
const out = {};
for (const file of process.argv.slice(2)) {
  try { out[file.replace(/.*\/(dist|lib)\//, '')] = census(file); } catch (e) { out[file] = { error: String(e.message) }; }
}
console.log(JSON.stringify(out, null, 1));
