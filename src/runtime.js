// TuiBridge Runtime — React-like reconciler + Ink bridge wrappers
// ~450 lines. All logic that must live in JS (hooks, reconciliation).
// Bridge wrappers at the top call into Rust via __ink_call.

// ===================================================================
// 0. Bridge Wrappers (Rust FFI via __ink_call)
// ===================================================================

globalThis.__ink_create_root = function() {
  return parseFloat(__ink_call('create_root', '[]')) || 0;
};

globalThis.__ink_destroy_root = function(id) {
  __ink_call('destroy_root', JSON.stringify([id]));
};

globalThis.__ink_create_node = function(tag, props) {
  return parseFloat(__ink_call('create_node', JSON.stringify([tag, props]))) || 0;
};

globalThis.__ink_create_text_node = function(text) {
  return parseFloat(__ink_call('create_text_node', JSON.stringify([text]))) || 0;
};

globalThis.__ink_append_child = function(parent, child) {
  return __ink_call('append_child', JSON.stringify([parent, child])) === 'true';
};

globalThis.__ink_remove_child = function(parent, child) {
  return __ink_call('remove_child', JSON.stringify([parent, child])) === 'true';
};

globalThis.__ink_insert_before = function(parent, child, before) {
  return __ink_call('insert_before', JSON.stringify([parent, child, before])) === 'true';
};

globalThis.__ink_commit_update = function(id, props) {
  return __ink_call('commit_update', JSON.stringify([id, props])) === 'true';
};

globalThis.__ink_set_text = function(id, text) {
  return __ink_call('set_text', JSON.stringify([id, text])) === 'true';
};

globalThis.__ink_commit = function() {
  __ink_call('commit', '[]');
};

globalThis.__ink_is_dirty = function() {
  return __ink_call('is_dirty', '[]') === 'true';
};

globalThis.__ink_clear_dirty = function() {
  __ink_call('clear_dirty', '[]');
};

globalThis.__ink_measure_text = function(text, width) {
  var result = __ink_call('measure_text', JSON.stringify([text, width]));
  var parts = result.split(',');
  return { width: parseInt(parts[0]) || 0, height: parseInt(parts[1]) || 0 };
};

globalThis.__ink_measure_element = function(id) {
  var result = __ink_call('measure_element', JSON.stringify([id]));
  if (result === 'null') return null;
  var parts = result.split(',');
  return { width: parseFloat(parts[0]) || 0, height: parseFloat(parts[1]) || 0 };
};

globalThis.__ink_exit = function(code) {
  __ink_call('exit', JSON.stringify([code || 0]));
};

globalThis.__ink_should_exit = function() {
  return __ink_call('should_exit', '[]') === 'true';
};

globalThis.__ink_get_exit_code = function() {
  return parseFloat(__ink_call('get_exit_code', '[]')) || 0;
};

globalThis.__ink_reset_exit = function() {
  __ink_call('reset_exit', '[]');
};

globalThis.__ink_set_terminal_size = function(width, height) {
  __ink_call('set_terminal_size', JSON.stringify([width, height]));
};

globalThis.__ink_get_terminal_size = function() {
  var result = __ink_call('get_terminal_size', '[]');
  var parts = result.split(',');
  return { width: parseInt(parts[0]) || 0, height: parseInt(parts[1]) || 0 };
};

globalThis.__ink_get_node_tag = function(id) {
  var result = __ink_call('get_node_tag', JSON.stringify([id]));
  return result === 'null' ? null : result;
};

globalThis.__ink_get_node_text = function(id) {
  var result = __ink_call('get_node_text', JSON.stringify([id]));
  return result === 'null' ? null : result;
};

globalThis.__ink_get_node_children = function(id) {
  var result = __ink_call('get_node_children', JSON.stringify([id]));
  if (result === 'null') return null;
  try { return JSON.parse(result); } catch(e) { return null; }
};

globalThis.__ink_get_node_prop = function(id, prop) {
  var result = __ink_call('get_node_prop', JSON.stringify([id, prop]));
  return result === 'null' ? null : result;
};

globalThis.__ink_get_root_id = function() {
  var result = __ink_call('get_root_id', '[]');
  return result === 'null' ? null : parseFloat(result) || null;
};

globalThis.__ink_calculate_layout = function() {
  return __ink_call('calculate_layout', '[]') === 'true';
};

globalThis.__ink_get_layout = function(id) {
  var result = __ink_call('get_layout', JSON.stringify([id]));
  if (result === 'null') return null;
  var parts = result.split(',');
  return {
    left: parseFloat(parts[0]) || 0,
    top: parseFloat(parts[1]) || 0,
    width: parseFloat(parts[2]) || 0,
    height: parseFloat(parts[3]) || 0
  };
};

globalThis.__ink_get_node_parent = function(id) {
  var result = __ink_call('get_node_parent', JSON.stringify([id]));
  return result === 'null' ? null : parseFloat(result) || null;
};

globalThis.__ink_register_input = function(callback) {
  return parseFloat(__ink_call('register_input', JSON.stringify([callback]))) || 0;
};

globalThis.__ink_unregister_input = function(id) {
  __ink_call('unregister_input', JSON.stringify([id]));
};

globalThis.__ink_stdout_write = function(data) {
  __ink_call('stdout_write', JSON.stringify([data]));
};

globalThis.__ink_stderr_write = function(data) {
  __ink_call('stderr_write', JSON.stringify([data]));
};

globalThis.__ink_stdin_is_raw = function() {
  return __ink_call('stdin_is_raw', '[]') === 'true';
};

globalThis.__ink_set_raw_mode = function(enabled) {
  __ink_call('set_raw_mode', JSON.stringify([enabled]));
};

// Timer bridge wrappers
globalThis.__ink_set_timeout = function(callback, delay) {
  return parseFloat(__ink_call('set_timeout', JSON.stringify([callback, delay]))) || 0;
};

globalThis.__ink_set_interval = function(callback, interval) {
  return parseFloat(__ink_call('set_interval', JSON.stringify([callback, interval]))) || 0;
};

globalThis.__ink_clear_timer = function(id) {
  __ink_call('clear_timer', JSON.stringify([id]));
};

globalThis.__ink_process_timers = function() {
  var result = __ink_call('process_timers', '[]');
  if (result === 'null' || result === '[]') return [];
  try { return JSON.parse(result); } catch(e) { return []; }
};

globalThis.__ink_has_pending_timers = function() {
  return __ink_call('has_pending_timers', '[]') === 'true';
};

globalThis.__ink_next_timer_delay = function() {
  var result = __ink_call('next_timer_delay', '[]');
  var ms = parseInt(result);
  return ms >= 0 ? ms : null;
};

// Microtask bridge wrappers
globalThis.__ink_enqueue_microtask = function(callback) {
  __ink_call('enqueue_microtask', JSON.stringify([callback]));
};

globalThis.__ink_drain_microtasks = function() {
  var result = __ink_call('drain_microtasks', '[]');
  if (result === 'null' || result === '[]') return [];
  try { return JSON.parse(result); } catch(e) { return []; }
};

// ===================================================================
// 1. Element Factory (JSX target)
// ===================================================================

function createElement(type, props, ...children) {
  props = props || {};
  const flatChildren = children
    .flat(Infinity)
    .filter(c => c !== null && c !== undefined && c !== false);
  if (flatChildren.length === 1) {
    props.children = flatChildren[0];
  } else if (flatChildren.length > 1) {
    props.children = flatChildren;
  }
  return { type, props };
}

// ===================================================================
// 2. Component Instances & Hook Context
// ===================================================================

let currentInstance = null;
let currentHookIndex = 0;
let renderScheduled = false;
let pendingRerenders = new Set();

function getHookState() {
  if (!currentInstance) throw new Error('Hook called outside component');
  const hooks = currentInstance.hooks;
  const idx = currentHookIndex++;
  if (hooks[idx] === undefined) hooks[idx] = { type: 'empty' };
  return hooks[idx];
}

function scheduleRerender(instance) {
  pendingRerenders.add(instance);
  if (renderScheduled) return;
  renderScheduled = true;
  inkSetTimeout(function() {
    flushRerenders();
  }, 0);
}

globalThis.__tb_flush_rerenders = function() {
  renderScheduled = false;
  const batch = Array.from(pendingRerenders);
  pendingRerenders.clear();
  for (const inst of batch) {
    try {
      inst.rerender();
    } catch (e) {
      console.error('Rerender error:', e);
    }
  }
};

// ===================================================================
// 3. Hooks
// ===================================================================

function useState(initial) {
  const state = getHookState();
  if (state.type === 'empty') {
    state.type = 'state';
    state.value = typeof initial === 'function' ? initial() : initial;
    state.setters = [];
  }
  const instance = currentInstance;
  const hookIdx = currentHookIndex - 1;
  function setValue(updater) {
    const newVal = typeof updater === 'function' ? updater(state.value) : updater;
    if (newVal !== state.value) {
      state.value = newVal;
      scheduleRerender(instance);
    }
  }
  return [state.value, setValue];
}

function useEffect(effect, deps) {
  const state = getHookState();
  if (state.type === 'empty') {
    state.type = 'effect';
    state.effect = effect;
    state.deps = deps;
    state.cleanup = undefined;
    state.hasRun = false;
    state._shouldRun = true;
  } else {
    const oldDeps = state.deps;
    const hasChanged = !deps || !oldDeps || deps.length !== oldDeps.length ||
      deps.some((d, i) => d !== oldDeps[i]);
    if (hasChanged) {
      state.deps = deps;
      state.effect = effect;
      state._shouldRun = true;
    }
  }
}

let hookEffectId = 1;
const effectRegistry = new Map();

function useRef(initial) {
  const state = getHookState();
  if (state.type === 'empty') {
    state.type = 'ref';
    state.value = { current: initial };
  }
  return state.value;
}

function useMemo(fn, deps) {
  const state = getHookState();
  if (state.type === 'empty') {
    state.type = 'memo';
    state.value = fn();
    state.deps = deps;
  } else {
    const oldDeps = state.deps;
    const hasChanged = !deps || !oldDeps || deps.length !== oldDeps.length ||
      deps.some((d, i) => d !== oldDeps[i]);
    if (hasChanged) {
      state.value = fn();
      state.deps = deps;
    }
  }
  return state.value;
}

function useCallback(fn, deps) {
  return useMemo(() => fn, deps);
}

function useContext(ctx) {
  return ctx ? ctx._currentValue : undefined;
}

function createContext(defaultValue) {
  return {
    _currentValue: defaultValue,
    Provider: function Provider({ value, children }) {
      this._currentValue = value !== undefined ? value : defaultValue;
      return children;
    }
  };
}

// ===================================================================
// 4. Ink-Specific Hooks
// ===================================================================

const inputHandlers = new Map();
let nextHandlerId = 1;

// Track the current rendering node ID for mouse hit-testing
let currentRenderingNodeId = null;

function useInput(handler, options) {
  options = options || {};
  const state = getHookState();
  if (state.type === 'empty') {
    state.type = 'input';
    state.handler = handler;
    state.options = options;
    state.id = nextHandlerId++;
    // Associate this handler with the current rendering node
    state.nodeId = currentRenderingNodeId;
    inputHandlers.set(state.id, state);
  } else {
    state.handler = handler;
    state.options = options;
  }
}

function useApp() {
  return useMemo(() => ({
    exit: (err) => globalThis.__ink_exit(err ? 1 : 0),
    stdout: {
      write: (d) => globalThis.__ink_stdout_write ? globalThis.__ink_stdout_write(d) : null
    },
    stdin: { isRawModeSupported: true },
    stderr: {
      write: (d) => globalThis.__ink_stderr_write ? globalThis.__ink_stderr_write(d) : null
    }
  }), []);
}

function useStdin() {
  return useMemo(() => ({
    isRawMode: () => globalThis.__ink_stdin_is_raw ? globalThis.__ink_stdin_is_raw() : false
  }), []);
}

function useStdout() {
  return useMemo(() => {
    const ts = globalThis.__ink_get_terminal_size ? globalThis.__ink_get_terminal_size() : { width: 80, height: 24 };
    return {
      columns: ts.width || 80,
      write: (d) => globalThis.__ink_stdout_write ? globalThis.__ink_stdout_write(d) : null
    };
  }, []);
}

function useStderr() {
  return useMemo(() => ({
    write: (d) => globalThis.__ink_stderr_write ? globalThis.__ink_stderr_write(d) : null
  }), []);
}

function useFocus() {
  return useMemo(() => ({ isFocused: () => true }), []);
}

function useFocusManager() {
  return useMemo(() => ({
    focus: () => {}, blur: () => {}, next: () => {}, previous: () => {}
  }), []);
}

function measureElement(ref) {
  if (!ref || !ref.current || !ref.current.id) return undefined;
  const result = globalThis.__ink_measure_element(String(ref.current.id));
  if (!result || result === 'null') return undefined;
  const parts = result.split(',');
  return { width: parseFloat(parts[0]) || 0, height: parseFloat(parts[1]) || 0 };
}

// ===================================================================
// 5. Timer Polyfills (using Rust bridge) - OPTIMIZED
// Store Functions in JS Maps, pass only IDs to Rust
// ===================================================================

const timerCallbacks = new Map(); // JS timer ID -> { rustId, callback, type }
const rustTimerToJsId = new Map(); // Rust timer ID -> JS timer ID (for cleanup)
let timerIdCounter = 1;

// Synchronous wrappers - pass function reference to Rust via eval-safe ID
function inkSetTimeout(callback, delay) {
  if (typeof callback !== 'function') return 0;
  
  const jsId = timerIdCounter++;
  // Register callback in JS registry before passing to Rust
  timerCallbacks.set(jsId, { callback, type: 'timeout' });
  
  const rustId = globalThis.__ink_set_timeout ?
    globalThis.__ink_set_timeout(jsId, Math.floor(delay || 0)) : 0;
  
  if (rustId) {
    rustTimerToJsId.set(rustId, jsId);
  }
  
  return jsId;
}

function inkClearTimeout(id) {
  const entry = timerCallbacks.get(id);
  if (entry) {
    // Find and clear the Rust timer ID
    for (const [rustId, jsId] of rustTimerToJsId) {
      if (jsId === id) {
        if (globalThis.__ink_clear_timer) globalThis.__ink_clear_timer(rustId);
        rustTimerToJsId.delete(rustId);
        break;
      }
    }
    timerCallbacks.delete(id);
  }
}

function inkSetInterval(callback, interval) {
  if (typeof callback !== 'function') return 0;
  
  const jsId = timerIdCounter++;
  timerCallbacks.set(jsId, { callback, type: 'interval' });
  
  const rustId = globalThis.__ink_set_interval ?
    globalThis.__ink_set_interval(jsId, Math.floor(interval || 0)) : 0;
  
  if (rustId) {
    rustTimerToJsId.set(rustId, jsId);
  }
  
  return jsId;
}

function inkClearInterval(id) {
  inkClearTimeout(id);
}

// Called from Rust to invoke timer callbacks - avoids eval!
globalThis.__tb_invoke_timers = function(timerJsIds) {
  try {
    const ids = typeof timerJsIds === 'string' ? JSON.parse(timerJsIds) : timerJsIds;
    for (const rustId of ids) {
      const jsId = rustTimerToJsId.get(rustId);
      if (!jsId) continue;
      const entry = timerCallbacks.get(jsId);
      if (entry && typeof entry.callback === 'function') {
        try {
          entry.callback();
        } catch (e) {
          console.error('Timer callback error:', e);
        }
      }
      // Clean up one-shot timers
      if (entry && entry.type === 'timeout') {
        timerCallbacks.delete(jsId);
        rustTimerToJsId.delete(rustId);
      }
    }
  } catch (e) {
    console.error('Timer dispatch error:', e);
  }
};

// ===================================================================
// 6. Reconciler
// ===================================================================

function isFunctionComponent(type) {
  return typeof type === 'function';
}

function normalizeChildren(children) {
  if (children === null || children === undefined || children === false) return [];
  if (Array.isArray(children)) return children.flat(Infinity).filter(c => c !== null && c !== undefined && c !== false);
  return [children];
}

function mountTree(vNode, parentId) {
  if (vNode === null || vNode === undefined || vNode === false) return null;

  if (typeof vNode === 'string' || typeof vNode === 'number') {
    const id = globalThis.__ink_create_text_node(String(vNode));
    globalThis.__ink_append_child(String(parentId), String(id));
    return { nodeId: id, vNode: String(vNode), children: [], isText: true };
  }

  if (Array.isArray(vNode)) {
    const children = [];
    for (const child of vNode) {
      const mounted = mountTree(child, parentId);
      if (mounted) children.push(mounted);
    }
    return { nodeId: parentId, vNode: vNode, children, isFragment: true };
  }

  if (isFunctionComponent(vNode.type)) {
    const inst = new ComponentInstance(vNode.type, vNode.props);
    const savedNodeId = currentRenderingNodeId;
    currentRenderingNodeId = parentId;
    const output = inst.render();
    const mounted = mountTree(output, parentId);
    currentRenderingNodeId = savedNodeId;
    if (mounted) {
      mounted.instance = inst;
      inst.mountedTree = mounted;
      inst.parentId = parentId;
    }
    return mounted;
  }

  const type = vNode.type || 'ink-box';
  const props = vNode.props || {};
  const children = normalizeChildren(props.children);
  const propsCopy = {};
  for (const k in props) {
    if (k !== 'children') propsCopy[k] = props[k];
  }

  const nodeId = globalThis.__ink_create_node(type, JSON.stringify(propsCopy));
  globalThis.__ink_append_child(String(parentId), String(nodeId));

  const childTrees = [];
  for (const child of children) {
    const mounted = mountTree(child, nodeId);
    if (mounted) childTrees.push(mounted);
  }

  return { nodeId, vNode, children: childTrees, isHost: true };
}

function reconcileTree(tree, newVNode, parentId) {
  if (!tree) {
    return mountTree(newVNode, parentId);
  }

  if (newVNode === null || newVNode === undefined || newVNode === false) {
    if (tree.nodeId && !tree.isFragment) {
      globalThis.__ink_remove_child(String(parentId), String(tree.nodeId));
    }
    return null;
  }

  const newIsPrimitive = typeof newVNode === 'string' || typeof newVNode === 'number';
  if (tree.isText && newIsPrimitive) {
    const newText = String(newVNode);
    if (tree.vNode !== newText) {
      globalThis.__ink_set_text(String(tree.nodeId), newText);
      tree.vNode = newText;
    }
    return tree;
  }
  if (tree.isText !== newIsPrimitive) {
    globalThis.__ink_remove_child(String(parentId), String(tree.nodeId));
    return mountTree(newVNode, parentId);
  }

  if (Array.isArray(newVNode)) {
    if (!tree.isFragment) {
      if (tree.nodeId && !tree.isFragment) {
        globalThis.__ink_remove_child(String(parentId), String(tree.nodeId));
      }
      return mountTree(newVNode, parentId);
    }
    for (const child of tree.children) {
      if (child.nodeId) {
        globalThis.__ink_remove_child(String(parentId), String(child.nodeId));
      }
    }
    const newChildren = [];
    for (const child of newVNode) {
      const mounted = mountTree(child, parentId);
      if (mounted) newChildren.push(mounted);
    }
    tree.vNode = newVNode;
    tree.children = newChildren;
    return tree;
  }

  if (isFunctionComponent(newVNode.type)) {
    if (tree.instance && tree.instance.fn === newVNode.type) {
      tree.instance.props = newVNode.props;
      const savedNodeId = currentRenderingNodeId;
      currentRenderingNodeId = parentId;
      const newOutput = tree.instance.render();
      currentRenderingNodeId = savedNodeId;
      const newChild = reconcileTree(tree.children[0], newOutput, parentId);
      tree.children = newChild ? [newChild] : [];
      tree.instance.mountedTree = newChild;
      if (newChild) newChild.instance = tree.instance;
      return tree;
    } else {
      if (tree.nodeId && !tree.isFragment) {
        globalThis.__ink_remove_child(String(parentId), String(tree.nodeId));
      }
      return mountTree(newVNode, parentId);
    }
  }

  const newType = newVNode.type || 'ink-box';
  if (!tree.isHost || (tree.vNode && tree.vNode.type !== newType)) {
    if (tree.nodeId && !tree.isFragment) {
      globalThis.__ink_remove_child(String(parentId), String(tree.nodeId));
    }
    return mountTree(newVNode, parentId);
  }

  const props = newVNode.props || {};
  const propsCopy = {};
  for (const k in props) {
    if (k !== 'children') propsCopy[k] = props[k];
  }
  globalThis.__ink_commit_update(String(tree.nodeId), JSON.stringify(propsCopy));
  tree.vNode = newVNode;

  const newChildren = normalizeChildren(props.children);
  const oldChildren = tree.children;
  const maxLen = Math.max(oldChildren.length, newChildren.length);
  const reconciledChildren = [];

  for (let i = 0; i < maxLen; i++) {
    if (i < newChildren.length && i < oldChildren.length) {
      const reconciled = reconcileTree(oldChildren[i], newChildren[i], tree.nodeId);
      if (reconciled) reconciledChildren.push(reconciled);
    } else if (i < newChildren.length) {
      const mounted = mountTree(newChildren[i], tree.nodeId);
      if (mounted) reconciledChildren.push(mounted);
    } else if (i < oldChildren.length) {
      if (oldChildren[i].nodeId && !oldChildren[i].isFragment) {
        globalThis.__ink_remove_child(String(tree.nodeId), String(oldChildren[i].nodeId));
      }
    }
  }
  tree.children = reconciledChildren;
  return tree;
}

// ===================================================================
// 7. Component Instance
// ===================================================================

function ComponentInstance(fn, props) {
  this.fn = fn;
  this.props = props || {};
  this.hooks = [];
  this.mountedTree = null;
  this.parentId = null;
  this.rootId = null;
}

ComponentInstance.prototype.render = function() {
  currentInstance = this;
  currentHookIndex = 0;
  try {
    const result = this.fn(this.props);
    return result;
  } finally {
    currentInstance = null;
    currentHookIndex = 0;
    // Run effects that should run after this render
    for (const hook of this.hooks) {
      if (hook.type === 'effect' && hook._shouldRun) {
        hook._shouldRun = false;
        if (hook.cleanup) {
          try { hook.cleanup(); } catch (e) { console.error('Effect cleanup error:', e); }
        }
        try {
          const cleanup = hook.effect();
          hook.cleanup = typeof cleanup === 'function' ? cleanup : undefined;
          hook.hasRun = true;
        } catch (e) {
          console.error('Effect error:', e);
        }
      }
    }
  }
};

ComponentInstance.prototype.rerender = function() {
  if (!this.mountedTree || !this.rootId) return;
  const newOutput = this.render();
  const newTree = reconcileTree(this.mountedTree, newOutput, this.parentId);
  this.mountedTree = newTree;
  if (newTree) newTree.instance = this;
  globalThis.__ink_commit();
};

// ===================================================================
// 8. render() — Main Entry Point
// ===================================================================

function render(element, options) {
  options = options || {};

  const rootId = globalThis.__ink_create_root();
  const container = { rootId, tree: null, instance: null, unmounted: false };

  container.tree = mountTree(element, rootId);
  if (container.tree && container.tree.instance) {
    container.instance = container.tree.instance;
    container.instance.rootId = rootId;
    container.instance.parentId = rootId;
  }
  globalThis.__ink_commit();

  return {
    waitUntilExit: () => {
      return new Promise((resolve) => {
        const check = () => {
          if (globalThis.__ink_should_exit && globalThis.__ink_should_exit()) {
            resolve();
          } else {
            const st = globalThis.__ink_set_timeout || setTimeout;
            st(check, 50);
          }
        };
        check();
      });
    },
    unmount: () => {
      container.unmounted = true;
      globalThis.__ink_destroy_root(rootId);
    },
    rerender: (newElement) => {
      if (container.unmounted) return;
      container.tree = reconcileTree(container.tree, newElement, rootId);
      if (container.tree && container.tree.instance) {
        container.instance = container.tree.instance;
        container.instance.rootId = rootId;
        container.instance.parentId = rootId;
      }
      globalThis.__ink_commit();
    }
  };
}

// ===================================================================
// 9. Component Tags
// ===================================================================

const Box = 'ink-box';
const Text = 'ink-text';
const Static = 'ink-static';
const Newline = 'ink-newline';
const Spacer = 'ink-spacer';

// ===================================================================
// 10. Console Polyfill
// ===================================================================

if (!globalThis.console) {
  globalThis.console = {
    log: function() {
      const msg = Array.prototype.slice.call(arguments).map(String).join(' ') + '\n';
      try { globalThis.__ink_stdout_write(msg); } catch(e) {}
    },
    error: function() {
      const msg = '[ERROR] ' + Array.prototype.slice.call(arguments).map(String).join(' ') + '\n';
      try { globalThis.__ink_stderr_write(msg); } catch(e) {}
    },
    warn: function() {
      const msg = '[WARN] ' + Array.prototype.slice.call(arguments).map(String).join(' ') + '\n';
      try { globalThis.__ink_stdout_write(msg); } catch(e) {}
    },
    info: function() {
      const msg = '[INFO] ' + Array.prototype.slice.call(arguments).map(String).join(' ') + '\n';
      try { globalThis.__ink_stdout_write(msg); } catch(e) {}
    }
  };
}

// ===================================================================
// 11. Process Polyfill
// ===================================================================

// Microtask registry - stores Functions, not strings (avoids eval)
const microtaskCallbacks = [];

globalThis.__tb_invoke_microtasks = function() {
  // Drain and execute microtasks
  while (microtaskCallbacks.length > 0) {
    const callbacks = microtaskCallbacks.splice(0);
    for (const callback of callbacks) {
      try {
        if (typeof callback === 'function') {
          callback();
        }
      } catch (e) {
        console.error('Microtask error:', e);
      }
    }
  }
};

// setImmediate polyfill - same behavior as process.nextTick
// Schedules callback on next iteration of event loop
if (!globalThis.setImmediate) {
  globalThis.setImmediate = (cb, ...args) => {
    if (typeof cb === 'function') {
      // Use microtask queue for immediate execution
      // This matches Node.js behavior where setImmediate runs after I/O but before timers
      const wrapped = () => cb(...args);
      microtaskCallbacks.push(wrapped);
      return microtaskCallbacks.length; // Return "handle" (index for consistency)
    }
    return -1;
  };
  globalThis.clearImmediate = (handle) => {
    // For simplicity, we don't support cancellation
    // In production, we'd track handles and filter them out
  };
}

if (!globalThis.process) {
  globalThis.process = {
    stdout: { write: (s) => { try { globalThis.__ink_stdout_write(String(s)); } catch(e) {} } },
    stderr: { write: (s) => { try { globalThis.__ink_stderr_write(String(s)); } catch(e) {} } },
    nextTick: (cb, ...args) => {
      // Store function directly in JS array - no stringification needed
      if (typeof cb === 'function') {
        const wrapped = () => cb(...args);
        microtaskCallbacks.push(wrapped);
      }
    }
  };
}

// ===================================================================
// 12. Input Dispatch Wiring
// ===================================================================

globalThis.__tb_dispatch_key = function(key, ctrl, shift, alt) {
  for (const [id, state] of inputHandlers) {
    if (state.options && state.options.isActive === false) continue;
    try {
      state.handler(key, { ctrl, shift, alt, name: key, meta: false });
    } catch (e) {
      console.error('Input handler error:', e);
    }
  }
};

// Helper: Check if a point is inside a node's layout
function isPointInNode(nodeId, x, y) {
  const layout = globalThis.__ink_get_layout(nodeId);
  if (!layout) return false;
  // Layout: left, top, width, height
  return x >= layout.left && x < layout.left + layout.width &&
         y >= layout.top  && y < layout.top  + layout.height;
}

// Helper: Get all descendants of a node
function getNodeDescendants(nodeId) {
  const children = globalThis.__ink_get_node_children(nodeId) || [];
  let descendants = [];
  for (const childId of children) {
    descendants.push(childId);
    descendants = descendants.concat(getNodeDescendants(childId));
  }
  return descendants;
}

// Helper: Find the deepest node at a given position
function findDeepestNodeAt(x, y, rootId) {
  if (!rootId) return null;
  
  // Get all nodes and their layouts
  const candidates = [];
  
  // BFS to find all nodes containing the point, track depth
  function traverse(nodeId, depth) {
    if (isPointInNode(nodeId, x, y)) {
      candidates.push({ nodeId, depth });
    }
    const children = globalThis.__ink_get_node_children(nodeId) || [];
    for (const childId of children) {
      traverse(childId, depth + 1);
    }
  }
  
  traverse(rootId, 0);
  
  if (candidates.length === 0) return null;
  
  // Return the deepest (most specific) node
  candidates.sort((a, b) => b.depth - a.depth);
  return candidates[0].nodeId;
}

// Helper: Find which handler should receive the mouse event using hit-testing
function findMouseHandlerAt(x, y) {
  const rootId = globalThis.__ink_get_root_id();
  if (!rootId) return null;
  
  // Find the deepest node at this position
  const targetNodeId = findDeepestNodeAt(x, y, rootId);
  if (!targetNodeId) return null;
  
  // Find handlers whose node is an ancestor of (or equal to) the target
  // First, get all ancestors of target up to root
  const ancestors = new Set();
  let current = targetNodeId;
  while (current) {
    ancestors.add(current);
    // Get parent (we need to track parents - check if bridge provides this)
    const parent = globalThis.__ink_get_node_parent ? globalThis.__ink_get_node_parent(current) : null;
    current = parent;
  }
  
  // Find handlers whose nodeId is an ancestor of target
  let deepestHandler = null;
  let deepestDepth = -1;
  
  for (const [id, state] of inputHandlers) {
    if (state.options && state.options.isActive === false) continue;
    if (!state.nodeId) continue;
    
    if (ancestors.has(state.nodeId)) {
      // Check depth from root (closer to target = higher depth)
      // For simplicity, we'll just use the nodeId as a proxy for recency
      // A better approach would track depth per handler
      if (!deepestHandler || state.nodeId > deepestHandler.nodeId) {
        deepestHandler = state;
      }
    }
  }
  
  return deepestHandler;
}

// Fallback: mouse handlers without nodeId (legacy/global handlers)
const globalMouseHandlers = [];

globalThis.__tb_register_global_mouse_handler = function(handler) {
  globalMouseHandlers.push(handler);
  return globalMouseHandlers.length;
};

globalThis.__tb_dispatch_mouse = function(event) {
  const { column, row } = event;
  
  // Find the deepest handler at this position using hit-testing
  const handler = findMouseHandlerAt(column, row);
  
  if (handler) {
    try {
      handler.handler(event);
    } catch (e) {
      console.error('Mouse handler error:', e);
    }
  }
  
  // Also dispatch to global handlers (legacy behavior)
  for (const h of globalMouseHandlers) {
    try {
      h(event);
    } catch (e) {
      console.error('Global mouse handler error:', e);
    }
  }
};

// ===================================================================
// 13. Exports
// ===================================================================

const ink = {
  render,
  Box, Text, Static, Newline, Spacer,
  useState, useEffect, useRef, useMemo, useCallback, useContext,
  useInput, useApp, useStdin, useStdout, useStderr, useFocus, useFocusManager,
  measureElement,
  createElement,
  createContext,
  setTimeout: inkSetTimeout,
  clearTimeout: inkClearTimeout,
  setInterval: inkSetInterval,
  clearInterval: inkClearInterval,
};

globalThis.ink = ink;
globalThis.render = render;
globalThis.Box = Box;
globalThis.Text = Text;
globalThis.Static = Static;
globalThis.Newline = Newline;
globalThis.Spacer = Spacer;
globalThis.useState = useState;
globalThis.useEffect = useEffect;
globalThis.useRef = useRef;
globalThis.useMemo = useMemo;
globalThis.useCallback = useCallback;
globalThis.useContext = useContext;
globalThis.useInput = useInput;
globalThis.useApp = useApp;
globalThis.useStdin = useStdin;
globalThis.useStdout = useStdout;
globalThis.useStderr = useStderr;
globalThis.useFocus = useFocus;
globalThis.useFocusManager = useFocusManager;
globalThis.measureElement = measureElement;
globalThis.createElement = createElement;
globalThis.createContext = createContext;

if (typeof module !== 'undefined' && module.exports) {
  module.exports = ink;
}
