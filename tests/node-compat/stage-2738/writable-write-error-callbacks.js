const common = require('../../node/test/common');
const { Writable } = require('stream');

for (const autoDestroy of [false, true]) {
  const stream = new Writable({ autoDestroy, write() {} });
  stream.end();
  let errorCalled = false;
  let ticked = false;
  stream.write('late', common.mustCall((error) => {
    if (!ticked || errorCalled) throw new Error('callback order');
    if (error.code !== 'ERR_STREAM_WRITE_AFTER_END') throw error;
  }));
  stream.on('error', common.mustCall((error) => {
    errorCalled = true;
    if (error.code !== 'ERR_STREAM_WRITE_AFTER_END') throw error;
  }));
  ticked = true;
}
