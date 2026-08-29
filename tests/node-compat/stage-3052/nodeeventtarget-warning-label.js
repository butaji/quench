const { NodeEventTarget } = require('internal/event_target');

const warning = new Promise((resolve) => process.on('warning', resolve));
const target = new NodeEventTarget();
target.setMaxListeners(1);
target.on('foo', () => {});
target.on('foo', () => {});

warning.then((value) => {
  if (value.name !== 'MaxListenersExceededWarning') throw new Error('wrong warning name');
  if (value.target !== target) throw new Error('wrong warning target');
  if (value.count !== 2 || value.type !== 'foo') throw new Error('wrong warning facts');
  if (!value.message.includes('added to NodeEventTarget')) {
    throw new Error('wrong warning label');
  }
});
