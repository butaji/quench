const net = require('node:net');
const [port, N] = [Number(process.argv[2]), Number(process.argv[3])];
const c = net.connect(port, '127.0.0.1'); let done = 0, buf = '';
const send = () => c.write('GET / HTTP/1.1\r\nHost: localhost\r\n\r\n');
c.setEncoding('latin1'); c.on('connect', send);
c.on('data', (d) => { buf += d; for (;;) { const e = buf.indexOf('\r\n\r\n'); if (e < 0) return; if (buf.length < e + 6) return; buf = buf.slice(e + 6); if (++done === N) { console.log('client done'); c.end(); return; } send(); } });
