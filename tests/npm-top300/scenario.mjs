// Real published package APIs. One fresh process and working directory per package.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { Readable } from 'node:stream';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import http from 'node:http';
import { fileURLToPath } from 'node:url';

const name = process.argv[2];
const inventory = JSON.parse(fs.readFileSync(fileURLToPath(new URL('inventory.json', import.meta.url)), 'utf8'));
const installation = inventory.packages.find(p => p.name === name)?.installation ?? name;
const compiler = inventory.packages.find(p => p.name === 'typescript')?.installation ?? 'typescript';
const declarations = inventory.packages.find(p => p.name === name)?.types_only;
const namespace = name === '@esbuild/linux-x64' || declarations
  ? {}
  : name === 'node-releases'
  ? { default: createRequire(import.meta.url)('node-releases/data/processed/envs.json') }
  : name === '@babel/compat-data'
  ? { default: createRequire(import.meta.url)('@babel/compat-data/plugins') }
  : name === '@babel/runtime'
  ? await import('@babel/runtime/helpers/typeof')
  : name === '@babel/helper-globals'
  ? { default: createRequire(import.meta.url)('@babel/helper-globals/data/builtin-lower.json') }
  : name === '@aws-sdk/nested-clients'
  ? await import('@aws-sdk/nested-clients/sts')
  : name === '@img/sharp-linux-x64'
  ? { default: createRequire(import.meta.url)('@img/sharp-linux-x64/sharp.node') }
  : name === '@img/sharp-libvips-linux-x64'
  ? { default: createRequire(import.meta.url)('@img/sharp-libvips-linux-x64/versions') }
  : name === 'vitest'
  ? {}
  : name.startsWith('@rollup/rollup-linux-') || name === '@tailwindcss/oxide-linux-x64-gnu'
  ? { default: createRequire(import.meta.url)(name) }
  : await import(name === 'punycode' ? 'punycode/punycode.js'
    : name === 'string_decoder' ? 'string_decoder/lib/string_decoder.js'
    : name === 'buffer' ? 'buffer/index.js' : installation);
const api = namespace.default ?? namespace;
let assertions = 0;
const equal = (actual, expected) => { assert.deepEqual(actual, expected); assertions++; };
// These scenarios actually invoke and assert the public default function. The
// tags cover that function only, including its alias paths; returned functions,
// attached methods and other exports remain independent obligations.
const defaultFunctionChecks = new Set([
  'ms', 'strip-ansi', 'escape-string-regexp',
  'emoji-regex', 'ansi-regex', 'has-flag', 'is-fullwidth-code-point', 'isarray',
  'is-number', 'is-glob', 'onetime', 'once', 'camelcase', 'fill-range',
  'to-regex-range', 'fast-json-stable-stringify', 'fast-deep-equal', 'deep-is',
  'get-intrinsic', 'call-bound', 'call-bind-apply-helpers', 'strip-bom',
  'escape-html', 'normalize-path', 'range-parser', 'bytes', 'lodash.merge',
  'buffer-from', 'anymatch', 'which-typed-array', 'is-regex', 'safe-regex-test',
  'is-number-object', 'is-string', 'which-boxed-primitive', 'typed-array-buffer',
  'is-shared-array-buffer', 'is-array-buffer', 'array-buffer-byte-length',
  'which-collection', 'is-binary-path', 'is-async-function', 'data-view-buffer',
  'is-unicode-supported', 'parent-module', 'shebang-command', 'p-map',
]);
const fixture = 'fixture.txt';
fs.writeFileSync(fixture, 'hello\n');

async function requestMiddleware(handler, { method = 'GET', body, headers = {}, url = '/' } = {}) {
  const server = http.createServer((request, response) => handler(request, response, error => {
    response.statusCode = error ? 500 : 404; response.end(error ? error.message : 'missing');
  }));
  await new Promise((resolve, reject) => { server.on('error', reject); server.listen(0, '127.0.0.1', resolve); });
  try {
    return await new Promise((resolve, reject) => {
      const request = http.request({ hostname: '127.0.0.1', port: server.address().port, path: url, method, headers }, response => {
        let text = ''; response.setEncoding('utf8'); response.on('data', chunk => text += chunk);
        response.on('end', () => resolve({ status: response.statusCode, headers: response.headers, text }));
      });
      request.on('error', reject); request.end(body);
    });
  } finally { await new Promise(resolve => server.close(resolve)); }
}

async function exercise() {
  if (declarations && !['type-fest', 'undici-types', '@types/node'].includes(name)) {
    const require = createRequire(import.meta.url);
    const directory = path.join(path.dirname(fileURLToPath(import.meta.url)), 'node_modules', name);
    const metadata = JSON.parse(fs.readFileSync(path.join(directory, 'package.json'), 'utf8'));
    const declaration = JSON.stringify(path.join(directory, metadata.types ?? metadata.typings ?? 'index.d.ts'));
    const binary = path.join(path.dirname(require.resolve(compiler + '/package.json')), 'bin', 'tsc');
    const compile = value => {
      fs.writeFileSync('types.ts', `import type * as Public from ${declaration};\ntype PublicAPI = typeof Public;\nconst typedValue: number = ${value};\n`);
      return spawnSync(process.execPath, [binary, 'types.ts', '--noEmit', '--target', 'esnext', '--module', 'nodenext', '--moduleResolution', 'nodenext', '--allowImportingTsExtensions', '--skipLibCheck'], { encoding: 'utf8' });
    };
    equal(compile('3').status, 0);
    const negative = compile('"invalid"'); equal(negative.status === 0, false);
    equal(negative.stdout.includes('error TS2322:'), true);
    console.log('npm-behavior-ok:' + name); return;
  }
  switch (name) {
    case 'type-fest':
    case 'undici-types':
    case '@types/node': {
      const require = createRequire(import.meta.url);
      const packageFile = path.join(path.dirname(fileURLToPath(import.meta.url)), 'node_modules', name, 'package.json');
      const metadata = JSON.parse(fs.readFileSync(packageFile, 'utf8'));
      const declaration = path.join(path.dirname(packageFile), metadata.types ?? metadata.typings ?? 'index.d.ts');
      const imported = JSON.stringify(declaration.replaceAll('\\', '/'));
      const prefix = name === '@types/node' ? '/// <reference path=' + JSON.stringify(declaration) + ' />\n' : '';
      const type = name === 'type-fest' ? `import(${imported}).JsonValue`
        : name === 'undici-types' ? `import(${imported}).Headers['get']` : 'import("node:buffer").Buffer';
      const valid = name === 'type-fest' ? '{a:1}' : name === 'undici-types' ? '(name: string) => name' : 'Buffer.from("x")';
      const invalid = name === 'type-fest' ? '() => 1' : name === 'undici-types' ? '42' : '"wrong"';
      const binary = path.join(path.dirname(require.resolve(compiler + '/package.json')), 'bin', 'tsc');
      const compile = value => {
        fs.writeFileSync('types.ts', prefix + `const typedValue: ${type} = ${value};\n`);
        return spawnSync(process.execPath, [binary, 'types.ts', '--noEmit', '--target', 'esnext', '--module', 'nodenext', '--moduleResolution', 'nodenext', '--allowImportingTsExtensions', '--skipLibCheck'], { encoding: 'utf8' });
      };
      const positive = compile(valid); equal(positive.status, 0);
      const negative = compile(invalid); equal(negative.status === 0, false);
      equal(negative.stdout.includes('error TS2322:'), true); break;
    }
    case 'semver':
      fs.writeFileSync('function-checks.json', JSON.stringify((await import('./contracts/semver.mjs')).coverage));
      break;
    case 'minimatch':
      equal(api.minimatch('src/a.js', '**/*.js'), true);
      equal(api.minimatch('src/a.ts', '**/*.js'), false);
      equal(api.braceExpand('a{b,c}'), ['ab', 'ac']);
      break;
    case 'debug': {
      api.enable('npm:*');
      const log = api('npm:test');
      equal(log.enabled, true);
      equal(api('other:test').enabled, false);
      equal(log.extend('child').namespace, 'npm:test:child');
      api.disable();
      break;
    }
    case 'brace-expansion':
      equal((namespace.expand ?? api)('a{1..3}'), ['a1', 'a2', 'a3']);
      break;
    case 'ansi-regex':
      equal('\u001b[31mred\u001b[39m'.replace(api(), ''), 'red');
      break;
    case 'supports-color':
      equal(namespace.createSupportsColor({ isTTY: false }, { sniffFlags: false }), false);
      break;
    case 'ms':
      equal(api('2 days'), 172800000);
      equal(api(60000), '1m');
      equal(api('invalid'), undefined);
      break;
    case 'ansi-styles':
      equal(api.red.open, '\u001b[31m');
      equal(api.red.close, '\u001b[39m');
      break;
    case 'chalk': {
      const chalk = new namespace.Chalk({ level: 1 });
      equal(chalk.red('x'), '\u001b[31mx\u001b[39m');
      equal(new namespace.Chalk({ level: 0 }).red('x'), 'x');
      break;
    }
    case 'emoji-regex':
      equal('a😀b'.match(api()), ['😀']);
      break;
    case 'wrap-ansi':
      equal(api('hello world', 5), 'hello\nworld');
      break;
    case 'lru-cache': {
      const cache = new api.LRUCache({ max: 2 });
      cache.set('a', 1); cache.set('b', 2); cache.get('a'); cache.set('c', 3);
      equal(cache.has('b'), false);
      equal(cache.get('a'), 1);
      break;
    }
    case 'tslib':
      equal(api.__assign({}, { a: 1 }), { a: 1 });
      equal(api.__spreadArray([1], [2, 3], false), [1, 2, 3]);
      break;
    case 'picomatch':
      equal(api('*.js')('a.js'), true);
      equal(api('*.js')('a.ts'), false);
      break;
    case 'glob':
      equal(api.globSync('*.txt'), [fixture]);
      equal(await api.glob('*.txt'), [fixture]);
      break;
    case 'minipass': {
      const stream = new api.Minipass({ encoding: 'utf8' });
      const result = stream.collect();
      stream.write('a'); stream.end('b');
      const chunks = await result;
      equal([...chunks], ['a', 'b']); equal(chunks.dataLength, 2);
      break;
    }
    case 'color-name': equal(api.red, [255, 0, 0]); break;
    case 'strip-ansi': equal(api('\u001b[31mred\u001b[39m'), 'red'); break;
    case 'balanced-match':
      equal(namespace.balanced('{', '}', 'a{b{c}d}e'), { start: 1, end: 7, pre: 'a', body: 'b{c}d', post: 'e' });
      break;
    case 'p-limit': {
      const limit = api(1);
      const order = [];
      const results = await Promise.all([1, 2, 3].map(x => limit(async () => { order.push(x); return x * 2; })));
      equal(results, [2, 4, 6]); equal(order, [1, 2, 3]);
      break;
    }
    case 'glob-parent': equal(api('src/**/*.js'), 'src'); break;
    case 'p-locate': equal(await api([1, 2, 3], async x => x === 2), 2); break;
    case 'has-flag': equal(api('verbose', ['node', 'app', '--verbose']), true); break;
    case 'iconv-lite': equal(api.decode(api.encode('café', 'latin1'), 'latin1'), 'café'); break;
    case 'entities':
      equal(api.decodeHTML('&amp;&#x1f600;'), '&😀');
      equal(api.escapeText('<a>&'), '&lt;a&gt;&amp;');
      break;
    case 'uuid': {
      const value = api.v5('hello', api.v5.DNS);
      equal(api.validate(value), true); equal(api.version(value), 5);
      equal(api.stringify(api.parse(value)), value);
      break;
    }
    case 'json-schema-traverse': {
      const pointers = [];
      api({ properties: { a: { type: 'string' } } }, { cb: (_s, pointer) => pointers.push(pointer) });
      equal(pointers, ['', '/properties/a']);
      break;
    }
    case 'string-width': equal(api('a😀中'), 5); break;
    case 'escape-string-regexp': equal(api('a+b?'), 'a\\+b\\?'); break;
    case 'globals': equal(api.node.process, false); equal(api.browser.window, false); break;
    case 'is-fullwidth-code-point': equal(api('中'.codePointAt(0)), true); equal(api(65), false); break;
    case 'argparse': {
      const parser = new api.ArgumentParser({ add_help: false });
      parser.add_argument('--count', { type: 'int' });
      equal(parser.parse_args(['--count', '3']).count, 3);
      break;
    }
    case 'ignore': {
      const ig = api().add(['*.log', '!keep.log']);
      equal(ig.ignores('a.log'), true); equal(ig.ignores('keep.log'), false);
      break;
    }
    case 'which':
      equal(typeof await (api.which ?? api)('node'), 'string');
      break;
    case 'esbuild': {
      const result = api.transformSync('const x: number = 1 + 2;', { loader: 'ts', minify: true });
      equal(result.code.includes('number'), false);
      equal(result.code.includes('const x=3'), true);
      break;
    }
    case 'isexe':
      equal(await (api.isexe ?? api)(process.execPath), true);
      break;
    case 'js-yaml': equal(api.load('a: 1\nb: [two, three]\n'), { a: 1, b: ['two', 'three'] }); break;
    case 'resolve': equal(api.sync('./fixture.txt', { basedir: process.cwd() }), path.resolve(fixture)); break;
    case 'mime-types': equal(api.lookup('file.json'), 'application/json'); equal(api.lookup('file.unknownxyz'), false); break;
    case 'nanoid': equal(namespace.nanoid(12).length, 12); equal(namespace.customAlphabet('a', 4)(), 'aaaa'); break;
    case 'yargs-parser': equal(api(['--count', '3', 'file']).count, 3); break;
    case 'source-map': {
      const map = new api.SourceMapGenerator({ file: 'out.js' });
      map.addMapping({ generated: { line: 1, column: 0 }, original: { line: 2, column: 1 }, source: 'in.js' });
      const consumer = await new api.SourceMapConsumer(map.toJSON());
      equal(consumer.originalPositionFor({ line: 1, column: 0 }).line, 2);
      consumer.destroy(); break;
    }
    case 'string_decoder': {
      const decoder = new api.StringDecoder('utf8');
      const bytes = Buffer.from('😀');
      equal(decoder.write(bytes.subarray(0, 2)), '');
      equal(decoder.end(bytes.subarray(2)), '😀'); break;
    }
    case 'color-convert': equal(api.rgb.hex(255, 0, 0), 'FF0000'); break;
    case 'estraverse': {
      const types = [];
      api.traverse({ type: 'Program', body: [{ type: 'ExpressionStatement', expression: { type: 'Literal', value: 1 } }] }, { enter: node => types.push(node.type) });
      equal(types, ['Program', 'ExpressionStatement', 'Literal']); break;
    }
    case 'https-proxy-agent': {
      const agent = new api.HttpsProxyAgent('http://localhost:8080');
      equal(agent.proxy.hostname, 'localhost'); agent.destroy(); break;
    }
    case '@babel/helper-validator-identifier': equal(api.isIdentifierName('hello'), true); equal(api.isIdentifierName('1bad'), false); break;
    case 'json5': equal(api.parse('{a:1, /*ok*/ b:"two",}'), { a: 1, b: 'two' }); break;
    case 'react-is': equal(api.isValidElementType('div'), true); equal(api.isValidElementType(42), false); break;
    case 'readdirp': {
      const entries = await namespace.readdirpPromise(process.cwd(), { depth: 0 });
      equal(entries.map(e => e.path).sort(), [fixture]); break;
    }
    case 'commander': {
      const cmd = new api.Command();
      cmd.exitOverride().option('--count <n>'); cmd.parse(['--count', '3'], { from: 'user' });
      equal(cmd.opts(), { count: '3' }); break;
    }
    case 'js-tokens': equal([...api('const x = 1')].filter(t => t.type !== 'WhiteSpace').map(t => t.value), ['const', 'x', '=', '1']); break;
    case 'shebang-regex': equal('#!/usr/bin/env node\nx'.match(api)[0], '#!/usr/bin/env node'); break;
    case 'fs-extra':
      await api.writeJson('data.json', { a: 1 }); equal(await api.readJson('data.json'), { a: 1 }); await api.remove('data.json'); break;
    case 'readable-stream': {
      const chunks = [];
      for await (const chunk of api.Readable.from(['a', 'b'])) chunks.push(chunk);
      equal(chunks, ['a', 'b']); break;
    }
    case 'punycode': equal(api.toASCII('mañana.com'), 'xn--maana-pta.com'); break;
    case 'tr46': equal(api.toASCII('mañana.com'), 'xn--maana-pta.com'); break;
    case 'find-up': equal(await namespace.findUp(fixture), path.resolve(fixture)); break;
    case 'webidl-conversions': equal(api['unsigned long'](-1), 4294967295); break;
    case 'path-exists': equal(await namespace.pathExists(fixture), true); equal(await namespace.pathExists('absent.txt'), false); break;
    case 'graceful-fs': equal(api.readFileSync(fixture, 'utf8'), 'hello\n'); break;
    case 'eslint-scope': {
      const tree = { type: 'Program', sourceType: 'script', body: [] };
      equal(api.analyze(tree).scopes[0].type, 'global'); break;
    }
    case 'yargs': equal(api(['--count', '3']).option('count', { type: 'number' }).parse().count, 3); break;
    case 'cross-spawn': {
      const result = api.sync('/bin/echo', ['hello'], { encoding: 'utf8' });
      equal(result.status, 0); equal(result.stdout, 'hello\n'); break;
    }
    case 'statuses': equal(api(404), 'Not Found'); equal(api('not found'), 404); break;
    case 'whatwg-url': equal(new api.URL('../b?q=1', 'https://example.com/a/').href, 'https://example.com/b?q=1'); break;
    case 'fast-deep-equal': equal(api({ a: [1, 2] }, { a: [1, 2] }), true); equal(api([1], [2]), false); break;
    case 'locate-path': equal(await namespace.locatePath(['absent.txt', fixture]), fixture); break;
    case 'is-number': equal(api('42'), true); equal(api(''), false); break;
    case 'get-stream': equal(await api(Readable.from(['a', 'b'])), 'ab'); break;
    case 'yaml': equal(api.parse('a: 1\nb: [two, three]\n'), { a: 1, b: ['two', 'three'] }); break;
    case 'path-scurry': {
      const scurry = new api.PathScurry(process.cwd());
      equal(scurry.readdirSync().map(p => p.name).sort(), [fixture]); break;
    }
    case '@babel/parser': equal(api.parse('const x = 1').program.body[0].type, 'VariableDeclaration'); break;
    case 'browserslist': equal(api('chrome 100'), ['chrome 100']); break;
    case '@babel/helper-string-parser': {
      const errors = { invalidDigit: () => { throw Error('invalid digit'); } };
      equal(api.readInt('123', 0, 0, 0, 10, undefined, false, errors).n, 123); break;
    }
    case 'camelcase': equal(api('hello-world'), 'helloWorld'); break;
    case 'yallist': {
      const list = new api.Yallist([1, 2]); list.push(3); equal(list.toArray(), [1, 2, 3]); break;
    }
    case '@babel/template': equal(api.statement('const x = VALUE;')({ VALUE: { type: 'NumericLiteral', value: 3 } }).declarations[0].init.value, 3); break;
    case 'cookie': equal(api.parseCookie('a=1; b=hello%20world').b, 'hello world'); break;
    case 'agent-base': {
      const agent = new api.Agent(); equal(agent.getName({ host: 'example.com', port: 80 }), 'example.com:80:'); agent.destroy(); break;
    }
    case 'safe-buffer': equal(api.Buffer.from('hello').toString('hex'), '68656c6c6f'); break;
    case 'qs': equal(api.parse('a[b]=1&a[c]=2'), { a: { b: '1', c: '2' } }); break;
    case 'fill-range': equal(api('a', 'c'), ['a', 'b', 'c']); break;
    case 'path-to-regexp': equal(api.match('/user/:id')('/user/42').params.id, '42'); break;
    case 'lodash': equal(api.chunk([1, 2, 3], 2), [[1, 2], [3]]); equal(api.get({ a: { b: 2 } }, 'a.b'), 2); break;
    case 'universalify': {
      const fn = api.fromCallback((x, cb) => cb(null, x * 2)); equal(await fn(3), 6); break;
    }
    case 'form-data': {
      const form = new api(); form.setBoundary('npm-test-boundary'); form.append('a', 'hello');
      equal(form.getBuffer().toString().includes('hello'), true);
      equal(form.getLengthSync(), form.getBuffer().length); break;
    }
    case 'jiti': {
      fs.writeFileSync('input.ts', 'const x: number = 3; export default x;');
      const jiti = namespace.createJiti(import.meta.url, { moduleCache: false, fsCache: false });
      equal(await jiti.import(path.resolve('input.ts'), { default: true }), 3); break;
    }
    case '@radix-ui/react-primitive': {
      const React = await import('react');
      const element = React.createElement(api.Primitive.div, { id: 'npm' }, 'hello');
      equal(element.type, api.Primitive.div); equal(element.props, { id: 'npm', children: 'hello' }); break;
    }
    case 'onetime': { let calls = 0; const fn = api(() => ++calls); equal(fn(), 1); equal(fn(), 1); equal(calls, 1); break; }
    case 'node-releases': equal(Array.isArray(api), true); equal(api.some(r => r.name === 'nodejs'), true); break;
    case 'ajv': {
      const validate = new api().compile({ type: 'object', properties: { a: { type: 'number' } }, required: ['a'] });
      equal(validate({ a: 1 }), true); equal(validate({ a: 'bad' }), false); break;
    }
    case 'is-glob': equal(api('src/*.js'), true); equal(api('plain.js'), false); break;
    case 'escalade': equal(await api(path.resolve(fixture), (_dir, files) => files.includes(fixture) && fixture), path.resolve(fixture)); break;
    case 'update-browserslist-db':
      assert.throws(() => api(() => {}), error => error.name === 'BrowserslistUpdateError' && error.message.startsWith('Cannot find package.json.'));
      break;
    case 'yocto-queue': { const q = new api(); q.enqueue(1); q.enqueue(2); equal(q.dequeue(), 1); equal(q.size, 1); break; }
    case 'to-regex-range': { const re = new RegExp('^(?:' + api(10, 20) + ')$'); equal(re.test('15'), true); equal(re.test('21'), false); break; }
    case 'fast-json-stable-stringify': equal(api({ b: 2, a: 1 }), '{"a":1,"b":2}'); break;
    case 'get-intrinsic': equal(api('%Array.prototype.slice%'), Array.prototype.slice); break;
    case 'eslint-visitor-keys':
      equal(api.KEYS.MemberExpression, ['object', 'property']);
      equal(api.getKeys({ type: 'Identifier', name: 'x', parent: null }), ['type', 'name']); break;
    case 'path-key': equal(api({ platform: 'win32', env: { Path: 'bin' } }), 'Path'); equal(api({ platform: 'linux', env: {} }), 'PATH'); break;
    case 'postcss': {
      const root = api.parse('a{color:red}'); root.walkDecls(d => { d.value = 'blue'; });
      equal(root.toString(), 'a{color:blue}'); break;
    }
    case 'zod': {
      const schema = api.object({ count: api.number().int().positive() });
      equal(schema.parse({ count: 3 }), { count: 3 }); equal(schema.safeParse({ count: '3' }).success, false); break;
    }
    case 'typescript': {
      fs.writeFileSync('input.ts', 'const x: number = 3;');
      const binary = path.join(path.dirname(createRequire(import.meta.url).resolve(compiler + '/package.json')), 'bin', 'tsc');
      const result = spawnSync(process.execPath, [binary, 'input.ts', '--outDir', 'out', '--target', 'es2020', '--module', 'esnext', '--skipLibCheck'], { encoding: 'utf8' });
      equal(result.status, 0); equal(fs.readFileSync('out/input.js', 'utf8').trim(), '"use strict";\nconst x = 3;'); break;
    }
    case 'signal-exit': {
      let calls = 0; const stop = api.onExit(code => { equal(code, 0); calls++; });
      process.emit('exit', 0); equal(calls, 1); stop(); break;
    }
    case 'ws': {
      const frames = namespace.Sender.frame(Buffer.from('hello'), { fin: true, opcode: 1, mask: false, rsv1: false, readOnly: false });
      equal(Buffer.concat(frames).toString('hex'), '810568656c6c6f');
      const receiver = new namespace.Receiver();
      const message = new Promise((resolve, reject) => { receiver.on('message', data => resolve(data.toString())); receiver.on('error', reject); });
      receiver.write(Buffer.concat(frames)); equal(await message, 'hello'); receiver.end(); break;
    }
    case '@esbuild/linux-x64': {
      const binary = createRequire(import.meta.url).resolve('@esbuild/linux-x64/bin/esbuild');
      const result = spawnSync(binary, ['--loader=ts'], { input: 'const x: number = 3;', encoding: 'utf8' });
      equal(result.status, 0); equal(result.stdout.trim(), 'const x = 3;'); break;
    }
    case '@jridgewell/trace-mapping': {
      const map = new api.TraceMap({ version: 3, names: [], sources: ['in.js'], mappings: 'AAAA' });
      equal(api.originalPositionFor(map, { line: 1, column: 0 }), { source: 'in.js', line: 1, column: 0, name: null }); break;
    }
    case 'shebang-command': equal(api('#!/usr/bin/env node'), 'node'); equal(api('#!/bin/bash'), 'bash'); break;
    case '@babel/types': equal(api.identifier('x'), { type: 'Identifier', name: 'x' }); equal(api.isIdentifier(api.numericLiteral(1)), false); break;
    case 'isarray': equal(api([]), true); equal(api({}), false); break;
    case 'cliui': { const ui = api({ width: 20 }); ui.div('hello'); equal(ui.toString(), 'hello'); break; }
    case 'resolve-from': equal(api(process.cwd(), './fixture.txt'), path.resolve(fixture)); break;
    case 'convert-source-map': {
      const map = { version: 3, names: [], sources: ['in.js'], mappings: 'AAAA' };
      equal(api.fromComment(api.fromObject(map).toComment()).toObject(), map); break;
    }
    case '@babel/generator': {
      const ast = { type: 'Program', sourceType: 'script', body: [{ type: 'ExpressionStatement', expression: { type: 'NumericLiteral', value: 3 } }] };
      equal((namespace.generate ?? api)(ast).code, '3;'); break;
    }
    case 'undici': {
      const headers = new api.Headers({ a: 'b' }); headers.append('a', 'c'); equal(headers.get('a'), 'b, c');
      equal(new api.Request('https://example.com/').method, 'GET'); break;
    }
    case 'tinyglobby': equal(await api.glob('*.txt'), [fixture]); break;
    case 'chokidar': {
      const watcher = api.watch(fixture, { persistent: false });
      await new Promise((resolve, reject) => { watcher.on('ready', resolve); watcher.on('error', reject); });
      equal(Object.values(watcher.getWatched()).flat().includes(fixture), true);
      await watcher.close(); break;
    }
    case '@babel/traverse': {
      const parser = await import('@babel/parser'); const ast = parser.parse('const x = 1;'); const names = [];
      (namespace.default ?? api)(ast, { Identifier(p) { names.push(p.node.name); } }); equal(names, ['x']); break;
    }
    case 'inherits': {
      function Parent() {} function Child() {} api(Child, Parent);
      equal(Object.getPrototypeOf(Child.prototype), Parent.prototype); equal(Child.super_, Parent); break;
    }
    case '@jridgewell/resolve-uri': equal(api('../b', 'https://example.com/a/'), 'https://example.com/b'); break;
    case 'call-bind-apply-helpers': equal(api([String.prototype.slice])('abc', 1), 'bc'); break;
    case 'call-bound': equal(api('String.prototype.slice')('abc', 1), 'bc'); break;
    case 'is-stream': equal(api.isReadableStream(Readable.from(['a'])), true); equal(api.isStream({}), false); break;
    case 'estree-walker': {
      const names = []; api.walk({ type: 'Identifier', name: 'x' }, { enter(node) { names.push(node.name); } }); equal(names, ['x']); break;
    }
    case 'minipass': {
      const stream = new api.Minipass({ encoding: 'utf8' }); stream.end('abc'); equal(await stream.concat(), 'abc'); break;
    }
    case 'fdir': equal(new api.fdir().withBasePath().crawl('.').sync().map(x => path.basename(x)).sort(), [fixture]); break;
    case 'electron-to-chromium': equal(api.electronToChromium('1.0'), '49'); break;
    case 'y18n': { const i18n = api({ updateFiles: false }); equal(i18n.__('hello %s', 'world'), 'hello world'); break; }
    case '@typescript-eslint/types': equal(api.AST_NODE_TYPES.Identifier, 'Identifier'); break;
    case '@typescript-eslint/visitor-keys': equal(api.visitorKeys.Identifier, ['decorators', 'typeAnnotation']); break;
    case '@babel/compat-data': {
      const plugins = createRequire(import.meta.url)('@babel/compat-data/plugins'); equal(typeof plugins['transform-arrow-functions'].chrome, 'string'); break;
    }
    case '@babel/runtime': equal(api(Symbol('x')), 'symbol'); equal(api(null), 'object'); break;
    case 'react': { const element = api.createElement('div', { id: 'npm' }, 'hello'); equal(element.props.children, 'hello'); equal(api.isValidElement(element), true); break; }
    case 'react-dom': equal(api.flushSync(() => 42), 42); assert.throws(() => api.createPortal('hello', {})); break;
    case 'buffer': equal(api.Buffer.from('hello').toString('hex'), '68656c6c6f'); break;
    case 'safer-buffer': equal(api.Buffer.alloc(2).toString('hex'), '0000'); break;
    case 'keyv': { const store = new api(); await store.set('a', 3); equal(await store.get('a'), 3); await store.delete('a'); equal(await store.has('a'), false); break; }
    case 'braces': equal(api.expand('a{b,c}'), ['ab', 'ac']); break;
    case 'once': { let count = 0; const fn = api(() => ++count); equal(fn(), 1); equal(fn(), 1); equal(count, 1); break; }
    case 'micromatch': equal(api(['a.js', 'a.ts'], '*.js'), ['a.js']); break;
    case 'esutils': equal(api.keyword.isIdentifierNameES6('hello'), true); equal(api.keyword.isIdentifierNameES6('a-b'), false); break;
    case 'strip-bom': equal(api('\ufeffhello'), 'hello'); break;
    case 'fast-glob': equal(await api('*.txt'), [fixture]); break;
    case 'flatted': { const object = { a: 1 }; object.self = object; const restored = api.parse(api.stringify(object)); equal(restored.self, restored); equal(restored.a, 1); break; }
    case 'raw-body': equal(await api(Readable.from([Buffer.from('hello')]), { encoding: 'utf8' }), 'hello'); break;
    case 'require-from-string': {
      const { exercise } = await import('./contracts/require-from-string.mjs');
      const result = exercise(api); assertions += result.assertions;
      fs.writeFileSync('function-checks.json', JSON.stringify(result)); break;
    }
    case 'whatwg-mimetype': { const mime = new api.MIMEType('Text/HTML; Charset=utf-8'); equal(mime.essence, 'text/html'); equal(mime.parameters.get('charset'), 'utf-8'); break; }
    case 'deep-is': equal(api({ a: [1] }, { a: [1] }), true); equal(api({ a: 1 }, { a: 2 }), false); break;
    case 'path-parse': equal(api('/a/file.txt'), { root: '/', dir: '/a', base: 'file.txt', ext: '.txt', name: 'file' }); break;
    case 'chai': api.expect({ a: 1 }).to.deep.equal({ a: 1 }); api.assert.throws(() => { throw new TypeError('x'); }, TypeError); break;
    case 'bytes': equal(api('1kb'), 1024); equal(api(1024), '1KB'); break;
    case 'tough-cookie': { const cookie = api.Cookie.parse('a=b; Path=/; HttpOnly'); equal(cookie.key, 'a'); equal(cookie.httpOnly, true); const jar = new api.CookieJar(); await jar.setCookie(cookie, 'https://example.com/'); equal(await jar.getCookieString('https://example.com/'), 'a=b'); break; }
    case 'normalize-path': equal(api('a\\b\\c'), 'a/b/c'); break;
    case 'make-dir': equal(await api.makeDirectory('nested/dir'), path.resolve('nested/dir')); equal(fs.statSync('nested/dir').isDirectory(), true); break;
    case '@eslint/js': equal(api.configs.recommended.rules['no-undef'], 'error'); break;
    case 'prettier': equal(await api.format('const x=1', { parser: 'babel' }), 'const x = 1;\n'); break;
    case 'range-parser': equal(api(100, 'bytes=1-3')[0], { start: 1, end: 3 }); equal(api(100, 'bytes=200-300'), -1); break;
    case 'supports-preserve-symlinks-flag': equal(api, true); break;
    case 'json-stable-stringify-without-jsonify': equal(api({ b: 2, a: 1 }), '{"a":1,"b":2}'); break;
    case 'mkdirp': equal(await api.mkdirp('nested/dir'), path.resolve('nested')); equal(fs.statSync('nested/dir').isDirectory(), true); break;
    case 'ansi-escapes': equal(api.cursorTo(2, 3), '\x1b[4;3H'); equal(api.cursorUp(2), '\x1b[2A'); break;
    case 'json-buffer': { const restored = api.parse(api.stringify({ data: Buffer.from('hi') })); equal(restored.data.toString(), 'hi'); break; }
    case 'escape-html': equal(api('<a x="hi">&'), '&lt;a x=&quot;hi&quot;&gt;&amp;'); break;
    case 'proxy-from-env': { process.env.http_proxy = 'http://localhost:1234'; equal(api.getProxyForUrl('http://example.com/'), 'http://localhost:1234'); delete process.env.http_proxy; break; }
    case 'queue-microtask': { const order = []; const promise = new Promise(resolve => api(() => { order.push(2); resolve(); })); order.push(1); await promise; equal(order, [1, 2]); break; }
    case 'is-unicode-supported': equal(api(), process.platform !== 'win32' && process.env.TERM !== 'linux'); break;
    case 'lodash.merge': equal(api({}, { a: { b: 1 } }, { a: { c: 2 } }), { a: { b: 1, c: 2 } }); break;
    case 'ieee754': { const data = new Uint8Array(8); api.write(data, 1.5, 0, true, 52, 8); equal(api.read(data, 0, true, 52, 8), 1.5); break; }
    case 'axios': equal(api.getUri({ url: '/a', baseURL: 'https://example.com', params: { x: 1 } }), 'https://example.com/a?x=1'); break;
    case 'proxy-addr': { const trust = api.compile('loopback'); equal(trust('127.0.0.1'), true); equal(trust('8.8.8.8'), false); break; }
    case 'schema-utils': api.validate({ type: 'number' }, 3); assert.throws(() => api.validate({ type: 'number' }, 'bad')); break;
    case 'slice-ansi': equal(api('\x1b[31mhello\x1b[39m', 1, 3), '\x1b[31mel\x1b[39m'); break;
    case 'buffer-from': equal(api('hello').toString(), 'hello'); break;
    case '@opentelemetry/semantic-conventions': equal(api.ATTR_SERVICE_NAME, 'service.name'); break;
    case '@babel/helper-globals': equal(api.includes('undefined'), true); break;
    case 'anymatch': equal(api('*.js', 'a.js'), true); equal(api('*.js', 'a.ts'), false); break;
    case 'which-typed-array': equal(api(new Uint16Array(2)), 'Uint16Array'); equal(api({}), false); break;
    case 'is-regex': equal(api(/x/), true); equal(api('x'), false); break;
    case 'define-properties': { const object = {}; api(object, { a: 3 }); equal(object.a, 3); equal(Object.keys(object), []); break; }
    case 'async': equal(await api.map([1, 2], async x => x * 2), [2, 4]); break;
    case 'rxjs': { const values = []; await new Promise(resolve => api.of(1, 2).subscribe({ next: x => values.push(x), complete: resolve })); equal(values, [1, 2]); break; }
    case 'domelementtype': equal(api.isTag({ type: api.ElementType.Tag }), true); equal(api.isTag({ type: api.ElementType.Text }), false); break;
    case 'safe-regex-test': { const test = api(/^a/); equal(test('abc'), true); equal(test('bc'), false); break; }
    case 'path-type': equal(await api.isFile(fixture), true); equal(await api.isDirectory(fixture), false); break;
    case 'fflate': { const input = new TextEncoder().encode('hello'); equal(new TextDecoder().decode(api.gunzipSync(api.gzipSync(input))), 'hello'); break; }
    case 'date-fns': equal(api.format(new Date('2020-01-02T00:00:00Z'), 'yyyy-MM-dd'), '2020-01-02'); equal(api.addDays(new Date('2020-01-02T00:00:00Z'), 2).toISOString(), '2020-01-04T00:00:00.000Z'); break;
    case 'd3-path': { const p = api.path(); p.moveTo(1, 2); p.lineTo(3, 4); equal(p.toString(), 'M1,2L3,4'); break; }
    case 'is-number-object': equal(api(new Number(1)), true); equal(api('1'), false); break;
    case 'is-string': equal(api(new String('a')), true); equal(api(1), false); break;
    case 'which-boxed-primitive': equal(api(new Number(1)), 'Number'); equal(api({}), undefined); break;
    case 'typed-array-buffer': { const view = new Uint8Array(2); assert.strictEqual(api(view), view.buffer); assertions++; assert.throws(() => api({})); break; }
    case 'is-shared-array-buffer': equal(api(new ArrayBuffer(2)), false); equal(api(new SharedArrayBuffer(2)), true); break;
    case 'is-array-buffer': equal(api(new ArrayBuffer(2)), true); equal(api(new Uint8Array(2)), false); break;
    case 'array-buffer-byte-length': equal(api(new ArrayBuffer(3)), 3); equal(Number.isNaN(api({})), true); break;
    case 'globby': equal(await api.globby('*.txt'), [fixture]); break;
    case 'is-binary-path': equal(api('image.png'), true); equal(api('script.js'), false); break;
    case 'eastasianwidth': equal(api.eastAsianWidth('Ａ'), 'F'); equal(api.characterLength('Ａ'), 2); break;
    case 'which-collection': equal(api(new Map()), 'Map'); equal(api([]), false); break;
    case 'is-async-function': equal(api(async () => {}), true); equal(api(() => {}), false); break;
    case 'data-view-buffer': { const buffer = new ArrayBuffer(2); assert.strictEqual(api(new DataView(buffer)), buffer); assertions++; assert.throws(() => api({})); break; }
    case '@pkgjs/parseargs': equal(api.parseArgs({ args: ['--foo', 'bar'], options: { foo: { type: 'string' } } }).values.foo, 'bar'); break;
    case 'd3-color': equal(api.rgb('red').formatHex(), '#ff0000'); equal(api.hsl('blue').h, 240); break;
    case 'ecdsa-sig-formatter': { const jose = Buffer.alloc(64, 1).toString('base64url'); equal(api.derToJose(api.joseToDer(jose, 'ES256'), 'ES256'), jose); break; }
    case 'json-schema': equal(api.validate({ a: 1 }, { type: 'object' }).valid, true); equal(api.validate('bad', { type: 'number' }).valid, false); break;
    case 'p-map': equal(await api([1, 2, 3], async x => x * 2, { concurrency: 2 }), [2, 4, 6]); break;
    case 'sisteransi': equal(api.cursor.up(2), '\x1b[2A'); equal(api.erase.line, '\x1b[2K'); break;
    case 'minimist': equal(api(['--count', '3', 'file'], { string: ['count'] }), { _: ['file'], count: '3' }); break;
    case 'dotenv': equal(api.parse('A=one\nB="two words"\n'), { A: 'one', B: 'two words' }); break;
    case 'rimraf': fs.mkdirSync('remove-me'); fs.writeFileSync('remove-me/file', 'x'); equal(await api.rimraf('remove-me'), true); equal(fs.existsSync('remove-me'), false); break;
    case 'readdirp': { const files = await api.readdirpPromise('.'); equal(files.map(f => f.basename).sort(), [fixture]); break; }
    case 'node-fetch': { const response = await api('data:application/json,%7B%22a%22%3A1%7D'); equal(response.status, 200); equal(await response.json(), { a: 1 }); break; }
    case '@babel/core': equal(api.transformSync('const x = 1', { configFile: false, babelrc: false }).code, 'const x = 1;'); break;
    case '@babel/helper-compilation-targets': equal(api({ chrome: '100' }), { chrome: '100.0.0' }); break;
    case '@babel/helpers': equal(namespace.get('typeof').nodes.length > 0, true); assert.throws(() => namespace.get('missing-helper')); break;
    case '@babel/plugin-syntax-typescript': { const babel = await import('@babel/core'); equal(babel.transformSync('const x: number = 1', { configFile: false, babelrc: false, plugins: [api] }).code, 'const x: number = 1;'); break; }
    case 'execa': { const result = await api.execa(process.execPath, ['-e', 'console.log(42)']); equal(result.stdout, '42'); equal(result.exitCode, 0); break; }
    case 'tinyexec': { const result = await api.exec(process.execPath, ['-e', 'console.log(42)']); equal(result.stdout.trim(), '42'); equal(result.exitCode, 0); break; }
    case 'eslint': { const coverage = (await import('./contracts/eslint.mjs')).exercise(api); fs.writeFileSync('function-checks.json', JSON.stringify(coverage)); break; }
    case '@typescript-eslint/parser': equal(api.parse('const x: number = 1').body[0].type, 'VariableDeclaration'); break;
    case '@typescript-eslint/typescript-estree': equal(api.parse('const x: number = 1').body[0].type, 'VariableDeclaration'); break;
    case '@typescript-eslint/scope-manager': { const parser = await import('@typescript-eslint/typescript-estree'); const scope = api.analyze(parser.parse('const x = 1')); equal(scope.scopes.some(s => s.variables.some(v => v.name === 'x')), true); break; }
    case '@typescript-eslint/utils': equal(api.AST_NODE_TYPES.Identifier, 'Identifier'); equal(api.ESLintUtils.getParserServices instanceof Function, true); break;
    case '@typescript-eslint/tsconfig-utils': { fs.writeFileSync('tsconfig.json', '{"compilerOptions":{"strict":true},"files":["input.ts"]}'); fs.writeFileSync('input.ts', 'const x=1;'); equal(api.getParsedConfigFile((await import('typescript')).default, path.resolve('tsconfig.json')).options.strict, true); break; }
    case 'jose': { const decoded = api.decodeJwt('eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.invalid'); equal(decoded.sub, '123'); equal(api.base64url.decode(api.base64url.encode(new Uint8Array([1, 2]))), new Uint8Array([1, 2])); break; }
    case 'http-proxy-agent': { const agent = new api.HttpProxyAgent('http://localhost:8080'); equal(agent.proxy.hostname, 'localhost'); equal(agent.getName({ host: 'example.com', port: 80 }), 'example.com:80:'); agent.destroy(); break; }
    case 'ajv-formats': { const Ajv = (await import('ajv')).default; const ajv = new Ajv(); api(ajv); const validate = ajv.compile({ type: 'string', format: 'email' }); equal(validate('a@example.com'), true); equal(validate('bad'), false); break; }
    case 'ajv-keywords': { const Ajv = (await import('ajv')).default; const ajv = new Ajv(); api(ajv, ['typeof']); const validate = ajv.compile({ typeof: 'undefined' }); equal(validate(undefined), true); equal(validate(null), false); break; }
    case 'jsdom': { const dom = new api.JSDOM('<p id="x">hello</p>'); equal(dom.window.document.getElementById('x').textContent, 'hello'); dom.window.close(); break; }
    case 'playwright': equal(api.chromium.name(), 'chromium'); equal(api.devices['Desktop Chrome'].defaultBrowserType, 'chromium'); break;
    case 'rollup': { const bundle = await api.rollup({ input: 'virtual', plugins: [{ name: 'fixture', resolveId: id => id, load: () => 'export default 42;' }] }); const result = await bundle.generate({ format: 'es' }); equal(result.output[0].code.includes('42'), true); await bundle.close(); break; }
    case '@rollup/rollup-linux-x64-gnu': equal(api.parse('const x=1;', false, false).length > 0, true); break;
    case '@tailwindcss/oxide-linux-x64-gnu': { const scanner = new api.Scanner({ sources: [] }); equal(scanner.scanFiles([{ content: '<div class="flex p-4">', extension: 'html' }]).includes('flex'), true); break; }
    case 'sharp': { const result = await api({ create: { width: 2, height: 3, channels: 3, background: '#ff0000' } }).png().toBuffer(); const metadata = await api(result).metadata(); equal([metadata.width, metadata.height, metadata.format], [2, 3, 'png']); break; }
    case 'cosmiconfig': { fs.writeFileSync('package.json', '{"fixtureConfig":{"answer":42}}'); const explorer = api.cosmiconfig('fixtureConfig', { searchPlaces: ['package.json'] }); equal((await explorer.search(process.cwd())).config, { answer: 42 }); break; }
    case '@eslint/config-helpers': equal(api.defineConfig({ rules: { semi: 'error' } }), [{ rules: { semi: 'error' } }]); equal(api.globalIgnores(['dist']).ignores, ['dist']); break;
    case '@eslint/plugin-kit': equal(new api.ConfigCommentParser().parseJSONLikeConfig('semi: 2').config, { semi: 2 }); break;
    case '@eslint-community/eslint-utils': equal(api.getStringIfConstant({ type: 'Literal', value: 'abc' }), 'abc'); break;
    case '@eslint/eslintrc': equal(api.Legacy.ConfigOps.getRuleSeverity(['error']), 2); break;
    case '@aws-sdk/types': equal(api.HostAddressType.A, 'A'); equal(api.HttpAuthLocation.HEADER, 'header'); break;
    case '@aws-sdk/xml-builder': equal(api.XmlNode.of('item', 'hello').toString(), '<item>hello</item>'); break;
    case '@vitest/expect': equal(new api.Any(Number).asymmetricMatch(3), true); equal(new api.Any(Number).asymmetricMatch('3'), false); break;
    case '@vitest/snapshot': equal(api.stripSnapshotIndentation('\n  a\n  b\n'), '\na\nb\n'); break;
    case '@vitest/mocker': { const registry = new api.MockerRegistry(); registry.register('redirect', 'raw', 'url', 'resolved', 'target'); equal(registry.has('resolved'), true); registry.clear(); equal(registry.has('resolved'), false); break; }
    case '@vitest/runner': { const utility = await import('@vitest/runner/utils'); equal(utility.createTaskName(['suite', 'test']), 'suite > test'); break; }
    case '@vitest/utils': { const helpers = await import('@vitest/utils/helpers'); equal(helpers.toArray(3), [3]); equal(helpers.toArray([1, 2]), [1, 2]); break; }
    case 'resolve-pkg-maps': equal(api.resolveExports({ '.': './index.js' }, '', ['node']), ['./index.js']); break;
    case 'baseline-browser-mapping': equal(api.getCompatibleVersions({ baselineYear: 2020 }).some(v => v.browser === 'chrome'), true); break;
    case '@asamuzakjp/css-color': equal(api.resolve('red'), 'rgb(255, 0, 0)'); break;
    case 'obug': api.enable('fixture:*'); equal(api.enabled('fixture:x'), true); api.disable(); equal(api.enabled('fixture:x'), false); break;
    case 'parent-module': equal(fileURLToPath(api()), fileURLToPath(import.meta.url)); break;
    case 'loose-envify': { const transform = api('input.js'); let result = ''; transform.on('data', x => result += x); const done = new Promise(resolve => transform.on('end', resolve)); transform.end('console.log(process.env.NODE_ENV)'); await done; equal(result.includes('console.log('), true); break; }
    case 'cli-cursor': api.hide({ isTTY: false }); api.show({ isTTY: false }); break;
    case 'ora': { const spinner = api({ text: 'hello', isEnabled: false, stream: { isTTY: false, write() {} } }); spinner.start(); equal(spinner.text, 'hello'); spinner.stop(); equal(spinner.isSpinning, false); break; }
    case 'react-refresh': { const runtime = createRequire(import.meta.url)('react-refresh/runtime'); function Component() {} runtime.register(Component, 'fixture'); equal(runtime.getFamilyByID('fixture').current, Component); break; }
    case 'eslint-plugin-react-hooks': equal(typeof api.rules['rules-of-hooks'].create, 'function'); equal(api.configs.flat.recommended.plugins['react-hooks'], api); break;
    case '@vitejs/plugin-react': { const plugins = api(); equal(plugins.some(p => p.name.includes('react')), true); break; }
    case 'vite': equal(api.normalizePath('a/b'), 'a/b'); equal(api.mergeConfig({ a: { b: 1 } }, { a: { c: 2 } }), { a: { b: 1, c: 2 } }); break;
    case 'tailwindcss': { const compiler = await namespace.compile('@tailwind utilities;'); equal(compiler.build(['flex']).includes('display: flex'), true); break; }
    case '@rolldown/pluginutils': equal(api.prefixRegex('abc').test('abcdef'), true); equal(api.prefixRegex('abc').test('defabc'), false); break;
    case '@radix-ui/react-slot': { const React = await import('react'); const render = await import('react-dom/server'); const child = React.createElement('div', { id: 'child' }, 'hello'); equal(render.renderToStaticMarkup(React.createElement(api.Slot, { className: 'slot' }, child)), '<div id="child" class="slot">hello</div>'); break; }
    case 'lucide-react': { const React = await import('react'); const element = React.createElement(api.Camera, { size: 16 }); equal(element.props.size, 16); equal(element.type, api.Camera); break; }
    case '@floating-ui/react-dom': { const result = await api.computePosition({ getBoundingClientRect: () => ({ x: 0, y: 0, width: 10, height: 10 }) }, {}, { platform: { getElementRects: async () => ({ reference: { x: 0, y: 0, width: 10, height: 10 }, floating: { x: 0, y: 0, width: 2, height: 2 } }), getClippingRect: async () => ({ x: 0, y: 0, width: 100, height: 100 }), getDimensions: async () => ({ width: 2, height: 2 }), isRTL: async () => false } }); equal([result.x, result.y], [4, 10]); break; }
    case 'express': { const app = api(); app.get('/hello', (_req, res) => res.json({ answer: 42 })); const response = await requestMiddleware(app, { url: '/hello' }); equal(response.status, 200); equal(JSON.parse(response.text), { answer: 42 }); break; }
    case 'serve-static': { const response = await requestMiddleware(api(process.cwd()), { url: '/fixture.txt' }); equal(response.status, 200); equal(response.text, 'hello\n'); break; }
    case 'body-parser': { const parse = api.json(); const response = await requestMiddleware((req, res, next) => parse(req, res, error => { if (error) return next(error); res.end(JSON.stringify(req.body)); }), { method: 'POST', body: '{"a":1}', headers: { 'content-type': 'application/json', 'content-length': '7' } }); equal(response.status, 200); equal(JSON.parse(response.text), { a: 1 }); break; }
    case 'cors': { const middleware = api(); const response = await requestMiddleware((req, res, next) => middleware(req, res, () => res.end('ok'))); equal(response.headers['access-control-allow-origin'], '*'); equal(response.text, 'ok'); break; }
    case 'jackspeak': equal(api.jack().opt({ name: {} }).parse(['--name', 'hello']).values.name, 'hello'); break;
    case 'foreground-child': { const result = spawnSync(process.execPath, ['-e', `require(${JSON.stringify(createRequire(import.meta.url).resolve('foreground-child'))}).foregroundChild(process.execPath,['-e','console.log(42)']);`], { encoding: 'utf8' }); equal(result.status, 0); equal(result.stdout, '42\n'); break; }
    case 'tar-stream': { const pack = api.pack(), extract = api.extract(); const contents = []; const done = new Promise((resolve, reject) => { extract.on('finish', resolve); extract.on('error', reject); }); extract.on('entry', (header, stream, next) => { let data = ''; stream.on('data', chunk => data += chunk); stream.on('end', () => { contents.push([header.name, data]); next(); }); }); pack.pipe(extract); pack.entry({ name: 'file', mtime: new Date(0) }, 'hello'); pack.finalize(); await done; equal(contents, [['file', 'hello']]); break; }
    case 'minizlib': { const gzip = new api.Gzip(); const chunks = []; const done = new Promise((resolve, reject) => { gzip.on('end', resolve); gzip.on('error', reject); }); gzip.on('data', chunk => chunks.push(chunk)); gzip.end('hello'); await done; const zlib = await import('node:zlib'); equal(zlib.gunzipSync(Buffer.concat(chunks)).toString(), 'hello'); break; }
    case '@img/colour': equal(api('red').hex(), '#FF0000'); equal(api('red').rgb().array(), [255, 0, 0]); break;
    case '@img/sharp-linux-x64': { const sharp = (await import('sharp')).default; const data = await sharp({ create: { width: 1, height: 1, channels: 3, background: 'red' } }).png().toBuffer(); equal((await sharp(data).metadata()).width, 1); break; }
    case '@img/sharp-libvips-linux-x64': { const sharp = (await import('sharp')).default; equal(sharp.versions.vips, api.vips); const data = await sharp({ create: { width: 1, height: 1, channels: 3, background: 'red' } }).png().toBuffer(); equal((await sharp(data).metadata()).width, 1); break; }
    case '@rollup/rollup-linux-x64-musl': equal(api.parse('const x=1;', false, false).length > 0, true); break;
    case '@typescript-eslint/eslint-plugin': { const ESLint = (await import('eslint')).Linter; const parser = await import('@typescript-eslint/parser'); const messages = new ESLint().verify('const x: any = 1;', [{ languageOptions: { parser }, plugins: { ts: api }, rules: { 'ts/no-explicit-any': 'error' } }]); equal(messages.map(m => m.ruleId), ['ts/no-explicit-any']); break; }
    case 'typescript-eslint': { const messages = new (await import('eslint')).Linter().verify('const x: any = 1;', [{ languageOptions: { parser: api.parser }, plugins: { ts: api.plugin }, rules: { 'ts/no-explicit-any': 'error' } }]); equal(messages.map(m => m.ruleId), ['ts/no-explicit-any']); break; }
    case '@typescript-eslint/type-utils': equal(api.isTypeAnyType({ flags: (await import('typescript')).default.TypeFlags.Any }), true); break;
    case '@typescript-eslint/project-service': { const service = api.createProjectService({ options: { allowDefaultProject: ['input.ts'] } }).service; equal(service.constructor.name, '_ProjectService'); service.closeLog(); break; }
    case '@babel/helper-module-transforms': { const babel = await import('@babel/core'); const plugin = () => ({ visitor: { Program(p) { const { headers } = api.rewriteModuleStatementsAndPrepareHeader(p, { exportName: 'exports' }); p.unshiftContainer('body', headers); } } }); const result = babel.transformSync('export const x = 1;', { plugins: [plugin], configFile: false, babelrc: false }); equal(result.code.includes('exports.x'), true); break; }
    case '@opentelemetry/instrumentation': { class Fixture extends api.InstrumentationBase { init() { return []; } enable() {} disable() {} _updateMetricInstruments() {} } const instrumentation = new Fixture('fixture', '1.0.0', { enabled: false }); instrumentation.setConfig({ enabled: false, custom: 42 }); equal(instrumentation.getConfig().custom, 42); break; }
    case '@aws-sdk/core': equal(api._toNum('42'), 42); equal(api._toBool('true'), true); break;
    case '@aws-sdk/nested-clients': { const input = { RoleArn: 'arn:aws:iam::123:role/test', RoleSessionName: 'fixture' }; const command = new api.AssumeRoleCommand(input); equal(command.input, input); const client = new api.STSClient({ region: 'us-east-1', credentials: { accessKeyId: 'test', secretAccessKey: 'test' } }); equal(await client.config.region(), 'us-east-1'); client.destroy(); break; }
    case '@aws-sdk/signature-v4-multi-region': { const crypto = await import('node:crypto'); class SHA256 { constructor(secret) { this.hash = secret ? crypto.createHmac('sha256', secret) : crypto.createHash('sha256'); } update(data) { this.hash.update(data); } async digest() { return this.hash.digest(); } } const signer = new api.SignatureV4MultiRegion({ service: 's3', region: 'us-east-1', credentials: { accessKeyId: 'AKID', secretAccessKey: 'SECRET' }, sha256: SHA256 }); const signed = await signer.sign({ method: 'GET', protocol: 'https:', hostname: 'example.com', path: '/', headers: { host: 'example.com' }, query: {} }, { signingDate: new Date('2020-01-01T00:00:00Z') }); equal(signed.headers.authorization.includes('Credential=AKID/20200101/us-east-1/s3/aws4_request'), true); break; }
    case 'google-logging-utils': { const entries = []; const logger = new api.AdhocDebugLogger('fixture', (fields, ...args) => entries.push([fields, args])); logger.invoke({ answer: 42 }, 'hello'); equal(entries.length, 1); break; }
    case '@radix-ui/react-id': { const React = await import('react'), render = await import('react-dom/server'); function Component() { return React.createElement('div', { id: api.useId('explicit') }, 'hello'); } equal(render.renderToStaticMarkup(React.createElement(Component)), '<div id="explicit">hello</div>'); break; }
    case '@radix-ui/react-focus-scope': { const React = await import('react'), render = await import('react-dom/server'); equal(render.renderToStaticMarkup(React.createElement(api.FocusScope, null, 'hello')).includes('hello'), true); break; }
    case '@testing-library/dom': { const { JSDOM } = await import('jsdom'); const dom = new JSDOM('<button>hello</button>'); equal(api.getByRole(dom.window.document.body, 'button').textContent, 'hello'); dom.window.close(); break; }
    case 'vitest': { const require = createRequire(import.meta.url); fs.writeFileSync('package.json', '{"type":"module"}'); fs.writeFileSync('spec.test.mjs', `import {test,expect} from ${JSON.stringify(path.join(path.dirname(require.resolve('vitest/package.json')), 'dist', 'index.js'))};test('add',()=>expect(1+1).toBe(2));`); const binary = path.join(path.dirname(require.resolve('vitest/package.json')), 'vitest.mjs'); const result = spawnSync(process.execPath, [binary, 'run', 'spec.test.mjs', '--maxWorkers=1', '--no-file-parallelism'], { encoding: 'utf8' }); assert.equal(result.status, 0, result.stderr + result.stdout); equal(result.stdout.includes('1 passed'), true); break; }
    default: throw new Error('No behavioral scenario for ' + name);
  }
  if (defaultFunctionChecks.has(name)) {
    assert.equal(typeof api, 'function');
    assert.ok(assertions > 0);
    fs.writeFileSync('function-checks.json', JSON.stringify({ package: name, assertions,
      checks: { '': 1 }, scope: 'asserted default-function scenario; aliases only; other/returned APIs remain uncovered' }));
  }
  if (name === 'brace-expansion' || name === 'balanced-match') {
    const checked = name === 'brace-expansion' ? 'expand' : 'balanced';
    fs.writeFileSync('function-checks.json', JSON.stringify({ package: name, assertions,
      checks: { [checked]: 1 }, scope: 'asserted named-function scenario; aliases only; other/returned APIs remain uncovered' }));
  }
  console.log('npm-behavior-ok:' + name);
}

await exercise();
