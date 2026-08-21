// Node compat: diagnostics_channel, matching Bun's documented surface.
var channels = {};
function Channel(name) {
  this.name = String(name);
  this._subscribers = [];
  this._store = undefined;
}
Channel.prototype.subscribe = function (fn) {
  if (typeof fn !== 'function') throw new TypeError('subscriber must be a function');
  if (this._subscribers.indexOf(fn) < 0) this._subscribers.push(fn);
  return this;
};
Channel.prototype.unsubscribe = function (fn) {
  var i = this._subscribers.indexOf(fn);
  if (i >= 0) this._subscribers.splice(i, 1);
  return this;
};
Channel.prototype.publish = function (message, context) {
  var copy = this._subscribers.slice();
  for (var i = 0; i < copy.length; i++) copy[i](message, context);
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
  var key = String(name);
  if (!channels[key]) channels[key] = new Channel(key);
  return channels[key];
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
  } else {
    names = nameOrChannels;
  }
  return new TracingChannel(names);
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
  } catch (error) {
    context.error = error;
    if (this.error) this.error.publish(context);
    throw error;
  }
  if (!result || typeof result.then !== 'function') return result;
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
function boundedChannel(nameOrChannels) {
  return new BoundedChannel(nameOrChannels);
}

module.exports = {
  Channel: Channel,
  channel: channel,
  subscribe: function (name, fn) { return channel(name).subscribe(fn); },
  unsubscribe: function (name, fn) { return channel(name).unsubscribe(fn); },
  hasSubscribers: function (name) { return channel(name).hasSubscribers; },
  channelNames: function () { return Object.keys(channels); },
  tracingChannel: tracingChannel,
  boundedChannel: boundedChannel
};