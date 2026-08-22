(function (deps) {
  'use strict';

  var active = Object.create(null);

  function add(category) {
    active[category] = (active[category] || 0) + 1;
  }
  function remove(category) {
    if (!active[category]) return;
    if (--active[category] === 0) delete active[category];
  }
  function categoriesString() {
    var out = [];
    for (var category in active) out.push(category);
    return out.join(',');
  }
  function Tracing(options) {
    if (!options || !Array.isArray(options.categories)) {
      throw new TypeError('The "options.categories" argument must be an instance of Array');
    }
    var list = options.categories.slice();
    if (list.length === 0) {
      var empty = new TypeError('At least one category is required');
      empty.code = 'ERR_TRACE_EVENTS_CATEGORY_REQUIRED';
      throw empty;
    }
    for (var i = 0; i < list.length; i++) {
      if (typeof list[i] !== 'string') throw new TypeError('Category must be a string');
    }
    this.categories = list.join(',');
    this.enabled = false;
    this._categories = list;
  }
  Tracing.prototype.enable = function () {
    if (!this.enabled) {
      this.enabled = true;
      for (var i = 0; i < this._categories.length; i++) add(this._categories[i]);
    }
  };
  Tracing.prototype.disable = function () {
    if (this.enabled) {
      this.enabled = false;
      for (var i = 0; i < this._categories.length; i++) remove(this._categories[i]);
    }
  };
  function createTracing(options) {
    return new Tracing(options);
  }

  function getEnabledCategories() { return categoriesString(); }

  return {
    createTracing: createTracing,
    getEnabledCategories: getEnabledCategories,
    Tracing: Tracing,
    WRITE_METADATA: 1,
    WRITE_EVENTS: 2
  };

});
