const fs = require('fs');
try { fs.accessSync(100); throw new Error('accepted numeric path'); }
catch (error) { if (error.code !== 'ERR_INVALID_ARG_TYPE') throw error; }
try { fs.access(100, fs.constants.F_OK, () => {}); throw new Error('accepted numeric path'); }
catch (error) { if (error.code !== 'ERR_INVALID_ARG_TYPE') throw error; }
