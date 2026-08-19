const os = require('node:os');
const ifaces = os.networkInterfaces();
console.log('count:', Object.keys(ifaces).length);
console.log('keys:', Object.keys(ifaces).join(','));
for (const name of Object.keys(ifaces)) {
  for (const a of ifaces[name]) {
    console.log(name, a.family, a.address, a.internal);
  }
}
