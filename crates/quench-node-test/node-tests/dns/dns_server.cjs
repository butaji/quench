'use strict';
// The DNS contract's authoritative server: UDP and TCP on one loopback port, printed as
// `PORT <n>` once both listen. It runs under Node so the client under test owns no
// part of the transport.
const dgram = require('dgram');
const net = require('net');

const TYPES = { A: 1, NS: 2, CNAME: 5, SOA: 6, PTR: 12, MX: 15, TXT: 16, AAAA: 28, SRV: 33, NAPTR: 35, OPT: 41, TLSA: 52, ANY: 255, CAA: 257 };
const RCODE = { NOERROR: 0, FORMERR: 1, SERVFAIL: 2, NXDOMAIN: 3, NOTIMP: 4, REFUSED: 5 };
const UDP_LIMIT = 512;

const name = (text) => Buffer.concat([...text.split('.').filter(Boolean).map((label) =>
  Buffer.concat([Buffer.from([label.length]), Buffer.from(label)])), Buffer.from([0])]);
const u16 = (value) => { const b = Buffer.alloc(2); b.writeUInt16BE(value); return b; };
const u32 = (value) => { const b = Buffer.alloc(4); b.writeUInt32BE(value); return b; };
const text = (value) => Buffer.concat([Buffer.from([Buffer.byteLength(value)]), Buffer.from(value)]);
const ipv4 = (value) => Buffer.from(value.split('.').map(Number));
const ipv6 = (value) => {
  const [head, tail = ''] = value.split('::');
  const left = head ? head.split(':') : [];
  const right = tail ? tail.split(':') : [];
  const groups = [...left, ...Array(8 - left.length - right.length).fill('0'), ...right];
  return Buffer.concat(groups.map((group) => u16(parseInt(group, 16))));
};

const rr = (owner, type, ttl, data) => ({ owner, type, ttl, data });
const A = (owner, ttl, address) => rr(owner, TYPES.A, ttl, ipv4(address));
const AAAA = (owner, ttl, address) => rr(owner, TYPES.AAAA, ttl, ipv6(address));
const CNAME = (owner, ttl, target) => rr(owner, TYPES.CNAME, ttl, name(target));
const NS = (owner, target) => rr(owner, TYPES.NS, 3600, name(target));
const PTR = (owner, target) => rr(owner, TYPES.PTR, 3600, name(target));
const MX = (owner, priority, exchange) => rr(owner, TYPES.MX, 3600, Buffer.concat([u16(priority), name(exchange)]));
const TXT = (owner, ...parts) => rr(owner, TYPES.TXT, 3600, Buffer.concat(parts.map(text)));
const SRV = (owner, priority, weight, port, target) =>
  rr(owner, TYPES.SRV, 3600, Buffer.concat([u16(priority), u16(weight), u16(port), name(target)]));
const NAPTR = (owner, order, preference, flags, service, regexp, replacement) => rr(owner, TYPES.NAPTR, 3600,
  Buffer.concat([u16(order), u16(preference), text(flags), text(service), text(regexp), name(replacement)]));
const SOA = (owner, nsname, hostmaster, serial, refresh, retry, expire, minttl) => rr(owner, TYPES.SOA, 3600,
  Buffer.concat([name(nsname), name(hostmaster), u32(serial), u32(refresh), u32(retry), u32(expire), u32(minttl)]));
const CAA = (owner, flags, tag, value) =>
  rr(owner, TYPES.CAA, 3600, Buffer.concat([Buffer.from([flags]), text(tag), Buffer.from(value)]));
const TLSA = (owner, usage, selector, match, data) =>
  rr(owner, TYPES.TLSA, 3600, Buffer.concat([Buffer.from([usage, selector, match]), Buffer.from(data, 'hex')]));

const soa = SOA('zone.test', 'ns1.zone.test', 'admin.zone.test', 2024010101, 7200, 3600, 1209600, 300);
const zone = {
  'a.test': { [TYPES.A]: [A('a.test', 300, '192.0.2.1'), A('a.test', 60, '192.0.2.2')] },
  'six.test': { [TYPES.AAAA]: [AAAA('six.test', 120, '2001:db8::1'), AAAA('six.test', 30, '2001:db8::2')] },
  'alias.test': {
    [TYPES.A]: [CNAME('alias.test', 40, 'a.test'), A('a.test', 300, '192.0.2.1')],
    [TYPES.CNAME]: [CNAME('alias.test', 40, 'a.test')],
    [TYPES.AAAA]: [CNAME('alias.test', 40, 'six.test')],
  },
  'chain.test': { [TYPES.CNAME]: [CNAME('chain.test', 50, 'mid.test'), CNAME('mid.test', 50, 'end.test')] },
  'mx.test': { [TYPES.MX]: [MX('mx.test', 10, 'mail.test'), MX('mx.test', 20, 'backup.test')] },
  'txt.test': { [TYPES.TXT]: [TXT('txt.test', 'v=spf1', ' include:test'), TXT('txt.test', 'single'), TXT('txt.test', '')] },
  'srv.test': { [TYPES.SRV]: [SRV('srv.test', 5, 10, 5060, 'sip.test'), SRV('srv.test', 0, 0, 1, '.')] },
  'naptr.test': { [TYPES.NAPTR]: [NAPTR('naptr.test', 100, 10, 'S', 'SIP+D2U', '', '_sip._udp.test')] },
  'zone.test': { [TYPES.SOA]: [soa], [TYPES.NS]: [NS('zone.test', 'ns1.zone.test'), NS('zone.test', 'ns2.zone.test')] },
  'caa.test': { [TYPES.CAA]: [CAA('caa.test', 0, 'issue', 'ca.test'), CAA('caa.test', 128, 'iodef', 'mailto:sec@test')] },
  'tlsa.test': { [TYPES.TLSA]: [TLSA('tlsa.test', 3, 1, 1, '0123456789abcdef')] },
  '1.2.0.192.in-addr.arpa': { [TYPES.PTR]: [PTR('1.2.0.192.in-addr.arpa', 'host.test'), PTR('1.2.0.192.in-addr.arpa', 'alias.test')] },
  'ptr.test': { [TYPES.PTR]: [CNAME('ptr.test', 10, 'real.ptr.test'), PTR('real.ptr.test', 'target.test')] },
  'any.test': {
    [TYPES.ANY]: [
      A('any.test', 10, '192.0.2.9'), AAAA('any.test', 20, '2001:db8::9'), MX('any.test', 1, 'mx.any.test'),
      NS('any.test', 'ns.any.test'), TXT('any.test', 'hello', 'world'), SRV('any.test', 1, 2, 3, 'srv.any.test'),
      PTR('any.test', 'ptr.any.test'), NAPTR('any.test', 1, 2, 'U', 'E2U+sip', '!^.*$!sip:x@test!', '.'),
      SOA('any.test', 'ns.any.test', 'root.any.test', 1, 2, 3, 4, 5), CAA('any.test', 0, 'issuewild', ';'),
      TLSA('any.test', 0, 0, 2, 'ff'),
    ],
  },
  'cnameany.test': { [TYPES.ANY]: [CNAME('cnameany.test', 10, 'a.test')] },
  'mixed.test': { [TYPES.A]: [MX('mixed.test', 1, 'm.test')] },
  'big.test': { [TYPES.TXT]: Array.from({ length: 12 }, (_, i) => TXT('big.test', `${i}`.padStart(3, '0') + 'x'.repeat(60))) },
};
const rcodes = {
  'servfail.test': RCODE.SERVFAIL, 'refused.test': RCODE.REFUSED, 'nxdomain.test': RCODE.NXDOMAIN,
  'notimp.test': RCODE.NOTIMP, 'formerr.test': RCODE.FORMERR,
};

function parseQuestion(packet) {
  let offset = 12;
  const labels = [];
  while (packet[offset]) {
    labels.push(packet.toString('latin1', offset + 1, offset + 1 + packet[offset]));
    offset += packet[offset] + 1;
  }
  const type = packet.readUInt16BE(offset + 1);
  return { id: packet.readUInt16BE(0), owner: labels.join('.').toLowerCase(), type, question: packet.subarray(12, offset + 5) };
}

function respond(packet, overTcp) {
  const { id, owner, type, question } = parseQuestion(packet);
  if (owner === 'garbage.test') return Buffer.from([packet[0], packet[1], 0x81, 0x80, 0, 1, 0, 1]);
  const answers = (zone[owner] || {})[type] || [];
  const rcode = rcodes[owner] ?? (zone[owner] || owner.endsWith('.arpa') && answers.length ? RCODE.NOERROR
    : owner in zone ? RCODE.NOERROR : RCODE.NXDOMAIN);
  const body = Buffer.concat(answers.map((r) =>
    Buffer.concat([name(r.owner), u16(r.type), u16(1), u32(r.ttl), u16(r.data.length), r.data])));
  const header = Buffer.concat([u16(id), u16(0x8180 | rcode), u16(1), u16(answers.length), u16(0), u16(0)]);
  const message = Buffer.concat([header, question, body]);
  if (!overTcp && message.length > UDP_LIMIT) {
    const truncated = Buffer.concat([u16(id), u16(0x8380 | rcode), u16(1), u16(0), u16(0), u16(0), question]);
    return truncated;
  }
  return message;
}

const silent = new Set(['silent.test']);
async function main() {
  const tcp = net.createServer((socket) => {
    let pending = Buffer.alloc(0);
    socket.on('data', (chunk) => {
      pending = Buffer.concat([pending, chunk]);
      while (pending.length >= 2 && pending.length >= 2 + pending.readUInt16BE(0)) {
        const packet = pending.subarray(2, 2 + pending.readUInt16BE(0));
        pending = pending.subarray(2 + packet.length);
        const reply = respond(packet, true);
        socket.write(Buffer.concat([u16(reply.length), reply]));
      }
    });
    socket.on('error', () => {});
  });
  await new Promise((resolve) => tcp.listen(0, '127.0.0.1', resolve));
  const port = tcp.address().port;
  const udp = dgram.createSocket('udp4');
  udp.on('message', (packet, remote) => {
    if (silent.has(parseQuestion(packet).owner)) return;
    udp.send(respond(packet, false), remote.port, remote.address);
  });
  await new Promise((resolve) => udp.bind(port, '127.0.0.1', resolve));
  console.log(`PORT ${port}`);
  process.stdin.resume();
  process.stdin.on('end', () => { tcp.close(); udp.close(); });
}

main();


