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
Object.defineProperty(Channel.prototype, 'hasSubscribers', { get: function () {
  return this._subscribers.length > 0;
}});
function channel(name) {
  var key = String(name);
  if (!channels[key]) channels[key] = new Channel(key);
  return channels[key];
}
module.exports = {
  channel: channel,
  subscribe: function (name, fn) { return channel(name).subscribe(fn); },
  unsubscribe: function (name, fn) { return channel(name).unsubscribe(fn); },
  hasSubscribers: function (name) { return channel(name).hasSubscribers; },
  channelNames: function () { return Object.keys(channels); }
};