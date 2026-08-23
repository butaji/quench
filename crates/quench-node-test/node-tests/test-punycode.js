// Node compat: punycode module RFC 3492 semantics.
const punycode = require('punycode');

// encode / decode round-trips (BMP + astral)
const cases = [
  ['ü', 'tda'],
  ['Goethe', 'Goethe-'],
  ['Bücher', 'Bcher-kva'],
  ['日本語', 'wgv71a119e'],
  ['𩸽', 'x73l'],
  [
    'Willst du die Blüthe des frühen, die Früchte des späteren Jahres',
    'Willst du die Blthe des frhen, die Frchte des spteren Jahres-x9e96lkal',
  ],
];
for (const [unicode, ascii] of cases) {
  if (punycode.encode(unicode) !== ascii) throw new Error('encode ' + unicode);
  if (punycode.decode(ascii) !== unicode) throw new Error('decode ' + ascii);
}

// RFC 3492 §6.2 worked examples with full battery
if (punycode.decode('') !== '') throw new Error('decode empty');
if (punycode.encode('') !== '') throw new Error('encode empty');

// error paths
function expectRangeError(fn, re) {
  let thrown = null;
  try { fn(); } catch (e) { thrown = e; }
  if (!(thrown instanceof RangeError)) throw new Error('no RangeError');
  if (re && !re.test(thrown.message)) throw new Error('message: ' + thrown.message);
}
expectRangeError(() => punycode.decode(' '), /Invalid input/);
expectRangeError(() => punycode.decode('α-'), /Illegal input >= 0x80/);
expectRangeError(() => punycode.decode('あ'), /Invalid input/);

// ucs2 helpers (well-formed code points, incl. astral)
if (punycode.ucs2.decode('a').join(',') !== '97') throw new Error('ucs2.decode a');
if (punycode.ucs2.decode('𝌆').join(',') !== '119558') throw new Error('ucs2.decode astral');
if (punycode.ucs2.encode([0x61]) !== 'a') throw new Error('ucs2.encode a');
if (punycode.ucs2.encode([0x1D306]) !== '\uD834\uDF06') throw new Error('ucs2.encode astral');
if (punycode.ucs2.encode([65, 252, 99]) !== 'Aüc') throw new Error('ucs2.encode multi');

// domain + email mapping
if (punycode.toASCII('Bücher@日本語.com') !== 'Bücher@xn--wgv71a119e.com') throw new Error('toASCII email');
if (punycode.toUnicode('Bücher@xn--wgv71a119e.com') !== 'Bücher@日本語.com') throw new Error('toUnicode email');
if (punycode.toASCII('mañana.com') !== 'xn--maana-pta.com') throw new Error('toASCII domain');
if (punycode.toUnicode('xn--maana-pta.com') !== 'mañana.com') throw new Error('toUnicode domain');

console.log('punycode: ok');