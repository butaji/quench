// Real published package APIs. One fresh process and working directory per package.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { Readable } from 'node:stream';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const name = process.argv[2];
const declarations = ['type-fest', 'undici-types', '@types/node'].includes(name);
const namespace = name === '@esbuild/linux-x64' || declarations
  ? {}
  : name === 'node-releases'
  ? { default: createRequire(import.meta.url)('node-releases/data/processed/envs.json') }
  : await import(name === 'punycode' ? 'punycode/punycode.js'
    : name === 'string_decoder' ? 'string_decoder/lib/string_decoder.js' : name);
const api = namespace.default ?? namespace;
const equal = (actual, expected) => assert.deepEqual(actual, expected);
const fixture = 'fixture.txt';
fs.writeFileSync(fixture, 'hello\n');

async function exercise() {
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
      const binary = path.join(path.dirname(require.resolve('typescript/package.json')), 'bin', 'tsc');
      const compile = value => {
        fs.writeFileSync('types.ts', prefix + `const typedValue: ${type} = ${value};\n`);
        return spawnSync(process.execPath, [binary, 'types.ts', '--noEmit', '--target', 'esnext', '--module', 'nodenext', '--moduleResolution', 'nodenext', '--allowImportingTsExtensions', '--skipLibCheck'], { encoding: 'utf8' });
      };
      const positive = compile(valid); equal(positive.status, 0);
      const negative = compile(invalid); equal(negative.status === 0, false);
      equal(negative.stdout.includes('error TS2322:'), true); break;
    }
    case 'semver':
      equal(api.satisfies('1.5.0', '^1.2.3'), true);
      equal(api.satisfies('2.0.0', '^1.2.3'), false);
      equal(api.inc('1.2.3', 'minor'), '1.3.0');
      equal(api.valid('bad'), null);
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
      const binary = path.join(path.dirname(createRequire(import.meta.url).resolve('typescript/package.json')), 'bin', 'tsc');
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
    default: throw new Error('No behavioral scenario for ' + name);
  }
  console.log('npm-behavior-ok:' + name);
}

await exercise();
