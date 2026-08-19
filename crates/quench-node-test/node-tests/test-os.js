// Node compat: os module basics.
const os = require('node:os');
const totalmem = os.totalmem();
if (!(totalmem > 0)) throw new Error('totalmem=' + totalmem);
const freemem = os.freemem();
if (!(freemem > 0)) throw new Error('freemem=' + freemem);
const cpus = os.cpus();
if (!(cpus.length > 0)) throw new Error('cpus=' + cpus.length);
if (typeof cpus[0].model !== 'string') throw new Error('model=' + typeof cpus[0].model);
if (typeof cpus[0].speed !== 'number') throw new Error('speed=' + typeof cpus[0].speed);
const times = cpus[0].times;
if (typeof times.user !== 'number') throw new Error('times.user=' + typeof times.user);
if (typeof times.sys !== 'number') throw new Error('times.sys=' + typeof times.sys);
console.log('os: ' + totalmem + ' ' + cpus.length + ' ' + freemem);
