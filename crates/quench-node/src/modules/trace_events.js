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
      var invalid = new TypeError('The "options.categories" argument must be an instance of Array');
      invalid.code = 'ERR_INVALID_ARG_TYPE';
      throw invalid;
    }
    var list = options.categories.slice();
    if (list.length === 0) {
      var empty = new TypeError('At least one category is required');
      empty.code = 'ERR_TRACE_EVENTS_CATEGORY_REQUIRED';
      throw empty;
    }
    for (var i = 0; i < list.length; i++) {
      if (typeof list[i] !== 'string') {
        var invalidCategory = new TypeError('Category must be a string');
        invalidCategory.code = 'ERR_INVALID_ARG_TYPE';
        throw invalidCategory;
      }
    }
    this._categories = list;
    this._enabled = false;
  }
  Object.defineProperties(Tracing.prototype, {
    categories: {
      enumerable: false,
      configurable: true,
      get: function () { return this._categories.join(','); }
    },
    enabled: {
      enumerable: false,
      configurable: true,
      get: function () { return this._enabled; }
    }
  });
  Tracing.prototype.enable = function () {
    if (!this._enabled) {
      this._enabled = true;
      for (var i = 0; i < this._categories.length; i++) add(this._categories[i]);
    }
  };
  Tracing.prototype.disable = function () {
    if (this._enabled) {
      this._enabled = false;
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
