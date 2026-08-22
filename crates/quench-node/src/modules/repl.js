function REPLServer(input, output, options) {
  this.input = input;
  this.output = output;
  this.options = options || {};
  this.context = this.options.context || {};
  this._prompt = this.options.prompt || '> ';
  this.historySize = this.options.historySize === undefined ? 30 : Number(this.options.historySize);
  this.history = [];
  this._commands = Object.create(null);
  this._eval = typeof this.options.eval === 'function' ? this.options.eval : null;
}
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
  this._commands[String(command)] = callback;
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