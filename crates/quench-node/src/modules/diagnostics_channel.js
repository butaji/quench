// Node compat: diagnostics_channel, matching Bun's documented surface.
// The registry is a Map so channel names may be strings or Symbols.
var channels = new Map();
function Channel(name) {
  this.name = name;
  this._subscribers = [];
  this._store = undefined;
  this._index = undefined;
}
function channelNameTypeError(name) {
  var err = new TypeError('The "channel" argument must be of type string or an instance of Symbol. Received ' + typeName(name));
  err.code = 'ERR_INVALID_ARG_TYPE';
  err.name = 'TypeError';
  return err;
}
function channelArgTypeError(name) {
  var err = new TypeError('The "' + name + '" argument must be of type function');
  err.code = 'ERR_INVALID_ARG_TYPE';
  err.name = 'TypeError';
  return err;
}
Channel.prototype.subscribe = function (fn) {
  if (typeof fn !== 'function') throw channelArgTypeError('subscriber');
  if (this._subscribers.indexOf(fn) < 0) this._subscribers.push(fn);
  return this;
};
Channel.prototype.unsubscribe = function (fn) {
  var i = this._subscribers.indexOf(fn);
  if (i < 0) return false;
  this._subscribers.splice(i, 1);
  return true;
};
// Node: `publish(message, ...rest)` invokes each subscriber as
// `subscriber(message, this.name)`; extra call args are not forwarded
// to the subscriber. The channel name is always the 2nd argument.
Channel.prototype.publish = function (message) {
  var copy = this._subscribers.slice();
  var name = this.name;
  for (var i = 0; i < copy.length; i++) copy[i](message, name);
};
Channel.prototype.bindStore = function (store) {
  this._store = store;
  return this;
};
Channel.prototype.unbindStore = function (store) {
  if (this._store === store) this._store = undefined;
  return this;
};
Object.defineProperty(Channel.prototype, 'hasSubscribers', { get: function () {
  return this._subscribers.length > 0;
}});
function channel(name) {
  if (typeof name !== 'string' && typeof name !== 'symbol') {
    throw channelNameTypeError(name);
  }
  var existing = channels.get(name);
  if (existing) return existing;
  var created = new Channel(name);
  channels.set(name, created);
  return created;
}

function TracingChannel(names) {
  this.start = names.start;
  this.end = names.end;
  this.asyncStart = names.asyncStart;
  this.asyncEnd = names.asyncEnd;
  this.error = names.error;
}
function tracingChannel(nameOrChannels) {
  var names;
  if (typeof nameOrChannels === 'string') {
    var base = 'tracing:' + nameOrChannels;
    names = {
      start: channel(base + ':start'),
      end: channel(base + ':end'),
      asyncStart: channel(base + ':asyncStart'),
      asyncEnd: channel(base + ':asyncEnd'),
      error: channel(base + ':error')
    };
  } else if (nameOrChannels && typeof nameOrChannels === 'object') {
    var start = channelFromMap(nameOrChannels, 'start');
    if (typeof start.hasSubscribers === 'undefined') {
      throw new TypeError('Cannot convert undefined or null to object');
    }
    names = {
      start: start,
      end: channelFromMap(nameOrChannels, 'end'),
      asyncStart: channelFromMap(nameOrChannels, 'asyncStart'),
      asyncEnd: channelFromMap(nameOrChannels, 'asyncEnd'),
      error: channelFromMap(nameOrChannels, 'error')
    };
  } else {
    var err = new TypeError('The "nameOrChannels" argument must be of type string or an instance of TracingChannel or Object. Received ' + typeName(nameOrChannels));
    err.code = 'ERR_INVALID_ARG_TYPE';
    err.name = 'TypeError';
    throw err;
  }
  return new TracingChannel(names);
}
function typeName(value) {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'an array';
  return typeof value;
}
function channelFromMap(map, name) {
  var value = map[name];
  if (value === undefined) return { hasSubscribers: undefined };
  if (!(value instanceof Channel)) {
    var err = new TypeError('The "nameOrChannels.' + name + '" property must be an instance of Channel. Received an instance of ' + (value && value.constructor ? value.constructor.name : typeof value));
    err.code = 'ERR_INVALID_ARG_TYPE';
    err.name = 'TypeError';
    throw err;
  }
  return value;
}
Object.defineProperty(TracingChannel.prototype, 'hasSubscribers', { get: function () {
  return Boolean(
    this.start && this.start.hasSubscribers ||
    this.end && this.end.hasSubscribers ||
    this.asyncStart && this.asyncStart.hasSubscribers ||
    this.asyncEnd && this.asyncEnd.hasSubscribers ||
    this.error && this.error.hasSubscribers
  );
}});
TracingChannel.prototype.subscribe = function (handlers) {
  for (var i = 0; i < START_EVENTS.length; i++) {
    var n = START_EVENTS[i];
    if (handlers && handlers[n]) this[n].subscribe(handlers[n]);
  }
};
TracingChannel.prototype.unsubscribe = function (handlers) {
  var ok = true;
  for (var i = 0; i < START_EVENTS.length; i++) {
    var n = START_EVENTS[i];
    if (handlers && handlers[n] && !this[n].unsubscribe(handlers[n])) ok = false;
  }
  return ok;
};
var START_EVENTS = ['start', 'end', 'asyncStart', 'asyncEnd', 'error'];
TracingChannel.prototype.traceSync = function (fn, context, thisArg) {
  var args = Array.prototype.slice.call(arguments, 3);
  if (!this.hasSubscribers) return fn.apply(thisArg, args);
  if (!context) context = {};
  if (this.start) this.start.publish(context);
  try {
    var result = fn.apply(thisArg, args);
    context.result = result;
    if (this.end) this.end.publish(context);
    return result;
  } catch (error) {
    context.error = error;
    if (this.error) this.error.publish(context);
    if (this.end) this.end.publish(context);
    throw error;
  }
};
TracingChannel.prototype.tracePromise = function (fn, context, thisArg) {
  var args = Array.prototype.slice.call(arguments, 3);
  if (!this.hasSubscribers) return fn.apply(thisArg, args);
  if (!context) context = {};
  if (this.start) this.start.publish(context);
  var self = this;
  var result;
  try {
    result = fn.apply(thisArg, args);
    context.result = result;
    if (self.end) self.end.publish(context);
  } catch (error) {
    context.error = error;
    if (self.error) self.error.publish(context);
    if (self.end) self.end.publish(context);
    throw error;
  }
  if (!result || typeof result.then !== 'function') {
    if (typeof process.emitWarning === 'function') {
      process.emitWarning(
        "tracePromise was called with the function '<anonymous>', which returned a non-thenable."
      );
    }
    return result;
  }
  return result.then(function (value) {
    context.result = value;
    if (self.asyncStart) self.asyncStart.publish(context);
    if (self.asyncEnd) self.asyncEnd.publish(context);
    return value;
  }, function (error) {
    context.error = error;
    if (self.error) self.error.publish(context);
    if (self.asyncStart) self.asyncStart.publish(context);
    if (self.asyncEnd) self.asyncEnd.publish(context);
    throw error;
  });
};
TracingChannel.prototype.traceCallback = function (fn, type, context, thisArg, callback) {
  var args = Array.prototype.slice.call(arguments, 5);
  if (typeof fn !== 'function') throw new TypeError('The "fn" argument must be of type function');
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  if (!this.hasSubscribers) return fn.apply(thisArg, [callback].concat(args));
  if (this.start) this.start.publish(context);
  var self = this;
  var done = function (error, result) {
    var completion = Object.assign({}, context);
    Object.defineProperty(completion, 'error', { value: error || undefined, enumerable: false });
    Object.defineProperty(completion, 'result', { value: result, enumerable: false });
    if (error && self.error) self.error.publish(completion);
    if (self.asyncStart) self.asyncStart.publish(completion);
    if (self.asyncEnd) self.asyncEnd.publish(completion);
    return callback(error, result);
  };
  try {
    fn.apply(thisArg, [done].concat(args));
    if (this.end) this.end.publish(context);
  } catch (error) {
    if (this.error) this.error.publish(Object.assign({}, context, { error: error }));
    if (this.end) this.end.publish(context);
    throw error;
  }
};

function BoundedChannel(nameOrChannels) {
  var names;
  if (typeof nameOrChannels === 'string') {
    var base = 'tracing:' + nameOrChannels;
    names = { start: channel(base + ':start'), end: channel(base + ':end') };
  } else {
    names = nameOrChannels;
  }
  this.start = names && names.start;
  this.end = names && names.end;
}
Object.defineProperty(BoundedChannel.prototype, 'hasSubscribers', { get: function () {
  return Boolean(this.start && this.start.hasSubscribers || this.end && this.end.hasSubscribers);
}});
BoundedChannel.prototype.subscribe = function (handlers) {
  if (handlers && handlers.start) this.start.subscribe(handlers.start);
  if (handlers && handlers.end) this.end.subscribe(handlers.end);
};
BoundedChannel.prototype.unsubscribe = function (handlers) {
  var ok = true;
  if (handlers && handlers.start && !this.start.unsubscribe(handlers.start)) ok = false;
  if (handlers && handlers.end && !this.end.unsubscribe(handlers.end)) ok = false;
  return ok;
};
// Run `fn(thisArg, ...args)` inside a start/end publish window. The
// context object is published as the message on start and end; a
// thrown error still publishes end before propagating. Store binding
// via bindStore is not materialized (async_hooks AsyncLocalStorage is
// a stub), so the no-subscriber fast path and window semantics hold
// without store enrichment.
BoundedChannel.prototype.run = function (context, fn, thisArg) {
  var args = Array.prototype.slice.call(arguments, 3);
  if (!this.hasSubscribers) return fn.apply(thisArg, args);
  if (this.start) this.start.publish(context);
  try {
    var result = fn.apply(thisArg, args);
    if (this.end) this.end.publish(context);
    return result;
  } catch (error) {
    if (this.end) this.end.publish(context);
    throw error;
  }
};
BoundedChannel.prototype.withScope = function (context) {
  var self = this;
  return {
    [Symbol.for('nodejs.dispose')]: function () {
      if (self.end) self.end.publish(context);
    },
    enter: function () {
      if (self.start) self.start.publish(context);
    }
  };
};
function boundedChannel(nameOrChannels) {
  return new BoundedChannel(nameOrChannels);
}

module.exports = {
  Channel: Channel,
  BoundedChannel: BoundedChannel,
  channel: channel,
  subscribe: function (name, fn) { return channel(name).subscribe(fn); },
  unsubscribe: function (name, fn) { return channel(name).unsubscribe(fn); },
  hasSubscribers: function (name) { return channel(name).hasSubscribers; },
  channelNames: function () {
    var out = [];
    channels.forEach(function (ch, key) {
      if (typeof key === 'string') out.push(key);
    });
    return out;
  },
  tracingChannel: tracingChannel,
  boundedChannel: boundedChannel
};
