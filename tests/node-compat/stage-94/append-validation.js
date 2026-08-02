const fs = require('fs');

try { fs.appendFileSync('/tmp/quench-node-stage-94', {}); throw new Error('invalid append data accepted'); }
catch (error) { if (error.code !== 'ERR_INVALID_ARG_TYPE') throw error; }
