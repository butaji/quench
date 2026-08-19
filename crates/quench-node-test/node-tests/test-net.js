// Node compat: net module basics.
const net = require('node:net');
if (net.isIP('127.0.0.1') !== 4) throw new Error('isIP v4=' + net.isIP('127.0.0.1'));
if (net.isIP('::1') !== 6) throw new Error('isIP v6=' + net.isIP('::1'));
if (net.isIP('not-an-ip') !== 0) throw new Error('isIP bad=' + net.isIP('not-an-ip'));
if (net.isIPv4('127.0.0.1') !== true) throw new Error('isIPv4=' + net.isIPv4('127.0.0.1'));
if (net.isIPv4('::1') !== false) throw new Error('isIPv4 v6=' + net.isIPv4('::1'));
if (net.isIPv6('::1') !== true) throw new Error('isIPv6=' + net.isIPv6('::1'));
if (net.isIPv6('127.0.0.1') !== false) throw new Error('isIPv6 v4=' + net.isIPv6('127.0.0.1'));
console.log('net: %s', net.isIP('127.0.0.1'));
