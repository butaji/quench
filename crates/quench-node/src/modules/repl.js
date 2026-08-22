function REPLServer(input, output, options) {
  this.input = input;
  this.output = output;
  this.options = options || {};
  this.context = this.options.context || {};
  this._prompt = this.options.prompt || '> ';
  this.historySize = this.options.historySize === undefined ? 30 : Number(this.options.historySize);
  this.history = [];
  this._commands = Object.create(null);
  this.commands = this._commands;
  this._eval = typeof this.options.eval === 'function' ? this.options.eval : null;
  this.closed = false;
}
REPLServer.prototype.addHistory = function (line) {
  if (this.historySize <= 0) return this;
  line = String(line);
  if (!line) return this;
  if (this.history.length && this.history[0] === line) return this;
  this.history.unshift(line);
  if (this.history.length > this.historySize) this.history.length = this.historySize;
  return this;
};
REPLServer.prototype.removeHistory = function (index) {
  index = Number(index);
  if (index >= 0 && index < this.history.length) this.history.splice(index, 1);
  return this;
};
REPLServer.prototype.setPrompt = function (prompt) { this._prompt = String(prompt); };
REPLServer.prototype.getPrompt = function () { return this._prompt; };
REPLServer.prototype.eval = function (cmd, context, filename, callback) {
  callback = typeof callback === 'function' ? callback : function () {};
  try {
    if (!this._eval) return callback(null, undefined);
    this._eval(String(cmd), context || this.context, filename || '', callback);
  } catch (error) {
    callback(error);
  }
  return undefined;
};
REPLServer.prototype.clearBufferedCommand = function () { return this; };
REPLServer.prototype.defineCommand = function (command, callback) {
  command = String(command);
  if (typeof callback === 'function') {
    this._commands[command] = { action: callback };
  } else if (callback && typeof callback.action === 'function') {
    this._commands[command] = callback;
  } else {
    throw new TypeError('command callback must be a function');
  }
  return this;
};
REPLServer.prototype.setupHistory = function (file, callback) {
  if (typeof callback === 'function') callback(null);
  return this;
};
REPLServer.prototype.prompt = function () {
  if (this.closed) return;
  if (this.output && typeof this.output.write === 'function') this.output.write(this._prompt);
};
REPLServer.prototype.close = function () {
  if (this.closed) return undefined;
  this.closed = true;
  if (typeof this._closeListener === 'function') this._closeListener();
  return undefined;
};
REPLServer.prototype.displayPrompt = REPLServer.prototype.prompt;
REPLServer.prototype.on = function (event, listener) {
  if (event === 'close' && typeof listener === 'function') this._closeListener = listener;
  return this;
};
module.exports = {
  start: function (options) {
    options = options || {};
    return new REPLServer(options.input, options.output, options);
  },
  REPLServer: REPLServer
};