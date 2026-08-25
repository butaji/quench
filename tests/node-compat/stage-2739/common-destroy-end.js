const common = require('../../node/test/common');
const { Writable } = require('stream');
const write = new Writable({ write(_chunk, _encoding, callback) { callback(); } });
write.destroy();
write.end(common.mustCall((error) => {
  if (error.code !== 'ERR_STREAM_DESTROYED') throw error;
}));
