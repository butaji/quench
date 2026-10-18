'use strict';
// DNS contract: a local authoritative server (dns_server.cjs, run by Node) answers every
// record type, response code and transport case; each observation prints one line in an
// order fixed by awaiting each scenario. Lookups use only names in the host's own files.
const dns = require('dns');
const net = require('net');
const path = require('path');
const { spawn } = require('child_process');
const util = require('util');

function startServer() {
  const child = spawn(process.env.DNS_CONTRACT_SERVER || 'node', [path.join(__dirname, 'dns_server.cjs')], { stdio: ['pipe', 'pipe', 'inherit'] });
  return new Promise((resolve) => {
    let output = '';
    child.stdout.on('data', (chunk) => {
      output += chunk;
      const match = /PORT (\d+)/.exec(output);
      if (match) resolve({ port: Number(match[1]), close: () => child.stdin.end() });
    });
  });
}

const show = (value) => util.inspect(value, { depth: 6, breakLength: Infinity, sorted: false });
const failure = (error) => `${error.constructor.name} ${show({ ...error, message: error.message })}`;
async function observe(label, run) {
  try {
    console.log(label, show(await run()));
  } catch (error) {
    console.log(label, failure(error));
  }
}
const callback = (method, ...args) => new Promise((resolve, reject) =>
  method(...args, (error, ...results) => (error ? reject(error) : resolve(results))));

async function main() {
  const server = await startServer();
  const address = `127.0.0.1:${server.port}`;
  const resolver = new dns.Resolver({ timeout: 200, tries: 1 });
  resolver.setServers([address]);
  console.log('servers', show(resolver.getServers().map((server) => server.replace(String(server.split(':')[1]), 'PORT'))));
  const promises = new dns.promises.Resolver({ timeout: 200, tries: 1 });
  promises.setServers([address]);

  const methods = ['resolve4', 'resolve6', 'resolveCname', 'resolveMx', 'resolveNs', 'resolveTxt', 'resolveSrv',
    'resolveNaptr', 'resolveSoa', 'resolveCaa', 'resolveTlsa', 'resolvePtr', 'resolveAny'];
  const names = ['a.test', 'six.test', 'alias.test', 'chain.test', 'mx.test', 'txt.test', 'srv.test', 'naptr.test',
    'zone.test', 'caa.test', 'tlsa.test', '1.2.0.192.in-addr.arpa', 'ptr.test', 'any.test', 'cnameany.test',
    'mixed.test', 'empty.test'];
  for (const method of methods) {
    for (const host of names) {
      await observe(`${method} ${host}`, () => promises[method](host));
    }
  }
  await observe('resolve4 ttl', () => promises.resolve4('a.test', { ttl: true }));
  await observe('resolve6 ttl', () => promises.resolve6('six.test', { ttl: true }));
  await observe('resolve4 cname ttl', () => promises.resolve4('alias.test', { ttl: true }));
  await observe('resolve rrtype MX', () => promises.resolve('mx.test', 'MX'));
  await observe('resolve default', () => promises.resolve('a.test'));
  await observe('resolve bad rrtype', () => promises.resolve('a.test', 'BOGUS'));
  await observe('callback resolve4', () => callback(resolver.resolve4.bind(resolver), 'a.test'));
  await observe('callback resolveTxt', () => callback(resolver.resolveTxt.bind(resolver), 'txt.test'));
  await observe('callback resolve4 ttl', () => callback(resolver.resolve4.bind(resolver), 'a.test', { ttl: true }));
  await observe('reverse', () => promises.reverse('192.0.2.1'));
  await observe('reverse invalid', () => promises.reverse('not-an-ip'));
  await observe('reverse hosts file', () => dns.promises.reverse('127.0.0.1'));
  await observe('callback reverse', () => callback(resolver.reverse.bind(resolver), '192.0.2.1'));
  for (const host of ['servfail.test', 'refused.test', 'nxdomain.test', 'notimp.test', 'formerr.test']) {
    await observe(`rcode ${host}`, () => promises.resolve4(host));
  }
  await observe('truncated over tcp', async () => (await promises.resolveTxt('big.test')).length);
  await observe('garbage', () => promises.resolve4('garbage.test'));
  await observe('timeout', () => promises.resolve4('silent.test'));
  await observe('long label', () => promises.resolve4(`${'x'.repeat(64)}.test`));

  const cancelled = new dns.Resolver({ timeout: 5000, tries: 1 });
  cancelled.setServers([address]);
  const pending = callback(cancelled.resolve4.bind(cancelled), 'silent.test');
  await observe('setServers pending', () => cancelled.setServers(['192.0.2.53']));
  cancelled.cancel();
  await observe('cancel', () => pending);

  const refused = new dns.promises.Resolver({ timeout: 200, tries: 2 });
  const closed = net.createServer();
  await new Promise((resolve) => closed.listen(0, '127.0.0.1', resolve));
  const closedPort = closed.address().port;
  await new Promise((resolve) => closed.close(resolve));
  refused.setServers([`127.0.0.1:${closedPort}`]);
  await observe('refused', () => refused.resolve4('a.test'));

  const configured = new dns.Resolver();
  configured.setServers(['192.0.2.1', '[2001:db8::1]:5353', '192.0.2.2:53', '2001:db8::2']);
  console.log('setServers mixed', show(configured.getServers()));
  await observe('setServers invalid', () => configured.setServers(['not an ip']));
  await observe('setServers type', () => configured.setServers('192.0.2.1'));
  await observe('setLocalAddress', () => configured.setLocalAddress('127.0.0.1'));
  await observe('setLocalAddress two v4', () => configured.setLocalAddress('127.0.0.1', '127.0.0.2'));
  await observe('setLocalAddress invalid', () => configured.setLocalAddress('nope'));
  await observe('resolver options', () => new dns.Resolver({ timeout: -2 }));

  await observe('lookup localhost v4', () => callback(dns.lookup, 'localhost', { family: 4 }));
  await observe('lookup localhost all', () => callback(dns.lookup, 'localhost', { all: true, family: 4 }));
  await observe('lookup ip', () => callback(dns.lookup, '192.0.2.7'));
  await observe('lookup ip all', () => callback(dns.lookup, '2001:db8::7', { all: true }));
  await observe('lookup missing', () => callback(dns.lookup, 'missing.invalid', { family: 4 }));
  await observe('lookup bad family', () => callback(dns.lookup, 'localhost', { family: 5 }));
  await observe('lookup bad hints', () => callback(dns.lookup, 'localhost', { hints: 0xffff }));
  await observe('lookup empty', () => callback(dns.lookup, ''));
  await observe('lookup promises', () => dns.promises.lookup('localhost', { family: 4 }));
  await observe('lookup promises all', () => dns.promises.lookup('localhost', { all: true, family: 4 }));
  await observe('lookupService', () => callback(dns.lookupService, '127.0.0.1', 0));
  await observe('lookupService promises', () => dns.promises.lookupService('127.0.0.1', 0));
  await observe('lookupService invalid', () => callback(dns.lookupService, 'nope', 22));
  await observe('lookupService port', () => callback(dns.lookupService, '127.0.0.1', 'x'));
  console.log('order', dns.getDefaultResultOrder(), dns.promises.getDefaultResultOrder());
  dns.setDefaultResultOrder('ipv4first');
  console.log('order set', dns.getDefaultResultOrder(), dns.promises.getDefaultResultOrder());
  await observe('order invalid', () => dns.setDefaultResultOrder('random'));
  console.log('constants', show(Object.keys(dns).filter((key) => /^[A-Z]+$/.test(key)).map((key) => [key, dns[key]])));
  console.log('promises is', dns.promises === require('dns/promises'), typeof dns.promises.Resolver);
  console.log('exports', Object.keys(dns).filter((key) => !/^[A-Z]+$/.test(key)).join());
  console.log('promise exports', Object.keys(dns.promises).filter((key) => !/^[A-Z]+$/.test(key)).join());
  console.log('promisify lookup', show(await util.promisify(dns.lookup)('localhost', { family: 4 })));
  server.close();
}

main();
