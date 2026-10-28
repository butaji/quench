'use strict';
const out = {};
function selectedMatch(match) {
  return match === null ? null : {
    values: Array.from(match),
    index: match.index,
    input: match.input,
    groups: match.groups === undefined ? null : match.groups,
  };
}

const short = 'aaab ab A12 zzzb';
const shortRe = /(a+)(?:\s+(b))?/g;
out.shortExec = [];
for (let i = 0; i < 3; i++) out.shortExec.push(selectedMatch(shortRe.exec(short)));
out.shortLastIndex = shortRe.lastIndex;
out.shortReplace = short.replace(/([a-z]+)(?:\s+(b))?/g, '[$1:$2]');

const longAscii = 'x'.repeat(140) + 'ABC1234' + 'y'.repeat(180) + 'QWE5678';
const longRe = /([A-Z]{3})(\d{4})/g;
longRe.lastIndex = 128;
out.longExec = [];
for (let i = 0; i < 3; i++) out.longExec.push(selectedMatch(longRe.exec(longAscii)));
out.longLastIndex = longRe.lastIndex;
out.longReplace = longAscii.replace(/([A-Z]{3})(\d{4})/g, '$2:$1');
out.longSearches = [
  longAscii.indexOf('ABC1234'),
  longAscii.indexOf('QWE5678'),
];

const utf16 = String.fromCodePoint(0x1f642) + 'é' + '\ud800' + 'z';
const utf16Re = /(.)/gu;
out.utf16Exec = [];
for (let i = 0; i < 5; i++) out.utf16Exec.push(selectedMatch(utf16Re.exec(utf16)));
out.utf16LastIndex = utf16Re.lastIndex;
out.utf16Replace = utf16.replace(/(.)/gu, (whole, capture, index) => `${index}:${capture.codePointAt(0).toString(16)}`);
out.utf16Search = [utf16.indexOf('é'), utf16.indexOf('\ud800')];

out.emptyMatchIndices = Array.from('ab'.matchAll(/(?:)/g), match => match.index);
out.emptyReplace = 'ab'.replace(/(?:)/g, '|');
out.emptyCaptureReplace = 'a1 b2'.replace(/([a-z])(\d)?/g, '<$1:$2>');

const result = JSON.stringify(out);
console.log(result);
