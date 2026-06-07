//! Minimal React shim for the rquickjs dev path.
//!
//! This shim provides React hooks (useState, useContext, etc.)
//! and bridges to runts_ink for rendering.

/// React shim source code - injected before user code in the JS bundle.
pub const REACT_SHIM: &str = r#"var React = (function() {
    var currentHooks = null;
    var currentIdx = 0;

    function useState(initial) {
        var idx = currentIdx++;
        if (currentHooks[idx] === undefined) {
            currentHooks[idx] = typeof initial === 'function' ? initial() : initial;
        }
        var val = currentHooks[idx];
        function setState(v) { currentHooks[idx] = v; }
        return [val, setState];
    }

    function useReducer(reducer, init) {
        var idx = currentIdx++;
        if (currentHooks[idx] === undefined) {
            currentHooks[idx] = { state: typeof init === 'function' ? init() : init };
        }
        var val = currentHooks[idx];
        function dispatch(action) {
            var next = reducer(val.state, action);
            if (next !== val.state) {
                currentHooks[idx] = { state: next };
            }
        }
        return [val.state, dispatch];
    }

    function useEffect(fn, deps) {
        var idx = currentIdx++;
        var old = currentHooks[idx];
        if (!old || !depsEqual(old.deps, deps)) {
            currentHooks[idx] = { deps: deps };
            __runts_effects.push(fn);
            __runts_has_effects = true;
        }
    }

    function useLayoutEffect(fn, deps) {
        var idx = currentIdx++;
        var old = currentHooks[idx];
        if (!old || !depsEqual(old.deps, deps)) {
            currentHooks[idx] = { deps: deps };
            __runts_layout_effects.push(fn);
            __runts_has_layout_effects = true;
        }
    }

    function useCallback(fn, deps) {
        var idx = currentIdx++;
        var old = currentHooks[idx];
        if (!old || !depsEqual(old.deps, deps)) {
            currentHooks[idx] = { deps: deps, cb: fn };
        }
        return currentHooks[idx].cb;
    }

    function useMemo(fn, deps) {
        var idx = currentIdx++;
        var old = currentHooks[idx];
        if (!old || !depsEqual(old.deps, deps)) {
            currentHooks[idx] = { deps: deps, value: fn() };
        }
        return currentHooks[idx].value;
    }

    function useRef(initial) {
        var idx = currentIdx++;
        if (currentHooks[idx] === undefined) {
            currentHooks[idx] = { current: typeof initial === 'function' ? initial() : initial };
        }
        return currentHooks[idx];
    }

    // Context value stack: keyed by context object reference (===).
    // Provider pushes on entry, pops on exit.
    var __ctxStack = [];
    function createContext(defaultValue) {
        return {
            __defaultValue: defaultValue,
            __id: __ctxId++,
            Provider: function(p) {
                __ctxStack.push({ id: __ctxId, value: p.value });
                // Wrap children in a Box VNode so the bridge renders them.
                var boxProps = { children: p.children };
                return runts_ink.box(boxProps);
            }
        };
    }
    var __ctxId = 0;

    function useContext(ctx) {
        // Walk the stack from top to find a matching context id.
        for (var i = __ctxStack.length - 1; i >= 0; i--) {
            if (__ctxStack[i].id === ctx.__id) return __ctxStack[i].value;
        }
        return ctx.__defaultValue;
    }

    function memo(Component) {
        var cache = null;
        return function(props) {
            var deps = Object.keys(props).map(function(k) { return props[k]; });
            if (!cache || !depsEqual(deps, cache.deps)) {
                cache = { deps: deps, vnode: Component(props) };
            }
            return cache.vnode;
        };
    }

    function forwardRef(Component) {
        return function(props) {
            return Component(Object.assign({}, props, { __forwarded_ref: true }));
        };
    }

    function depsEqual(a, b) {
        if (!a || !b || a.length !== b.length) return false;
        for (var i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
        return true;
    }

    function withHooks(fn) {
        if (fn.__withHooks) return fn.__withHooks;
        var hooks = [];
        var wrapped = function(props) {
            currentHooks = hooks;
            currentIdx = 0;
            return fn(props);
        };
        fn.__withHooks = wrapped;
        wrapped.__withHooks = wrapped;
        return wrapped;
    }

    function flatten(arr) {
        var out = [];
        for (var i = 0; i < arr.length; i++) {
            if (Array.isArray(arr[i])) { out.push.apply(out, flatten(arr[i])); }
            else if (arr[i] != null) { out.push(arr[i]); }
        }
        return out;
    }

    function extractTextFromVNode(vnode) {
        if (!vnode || typeof vnode !== 'object') return null;
        if (vnode.Text) return vnode.Text.__content || '';
        if (vnode.Box && vnode.Box.__children) {
            var parts = [];
            for (var i = 0; i < vnode.Box.__children.length; i++) {
                var t = extractTextFromVNode(vnode.Box.__children[i]);
                if (t) parts.push(t);
            }
            return parts.join('');
        }
        if (vnode.Fragment && vnode.Fragment.__children) {
            var parts = [];
            for (var i = 0; i < vnode.Fragment.__children.length; i++) {
                var t = extractTextFromVNode(vnode.Fragment.__children[i]);
                if (t) parts.push(t);
            }
            return parts.join('');
        }
        return null;
    }

    function createElement(type, props, ...children) {
        props = props || {};
        children = flatten(children);
        props.children = children;
        if (children.length === 0) { props.children = []; }
        if (typeof type === 'function') {
            if (!type.__withHooks) type.__withHooks = withHooks(type);
            return type.__withHooks(props);
        }
        if (type === 'Box') return runts_ink.box(props);
        if (type === 'Text') {
            var parts = [];
            for (var i = 0; i < children.length; i++) {
                var c = children[i];
                if (typeof c === 'string' || typeof c === 'number') {
                    parts.push(String(c));
                } else if (c && typeof c === 'object') {
                    var text = extractTextFromVNode(c);
                    if (text) parts.push(text);
                }
            }
            delete props.children;
            return runts_ink.text(parts.join(''), props);
        }
        if (type === 'Newline') return runts_ink.newline();
        if (type === 'Spacer') return runts_ink.spacer();
        if (type === 'Fragment') return { Fragment: { __children: children } };
        return runts_ink.box(props);
    }

    return {
        createElement, useState, useReducer, useEffect, useLayoutEffect, useCallback, useMemo, useRef,
        createContext, useContext, memo, forwardRef, Fragment: 'Fragment', _withHooks: withHooks
    };
})();

var useState = React.useState;
var useReducer = React.useReducer;
var useEffect = React.useEffect;
var useLayoutEffect = React.useLayoutEffect;
var useCallback = React.useCallback;
var useMemo = React.useMemo;
var useRef = React.useRef;
var createContext = React.createContext;
var useContext = React.useContext;
var memo = React.memo;
var forwardRef = React.forwardRef;
var __runts_ink_render = function(app) { return app; };"#;

/// Post-shim code - appended after user code.
pub const POST_SHIM: &str = r#"
var process = process || { exit: function(code) { __runts_exit = true; __runts_exit_code = code || 0; } };
var __runts_effects = [];
var __runts_has_effects = false;
var __runts_layout_effects = [];
var __runts_has_layout_effects = false;
function __runts_render_with_effects(props) {
    var vnode;
    
    // Reset per-render state so context values don't bleed between renders.
    if (typeof __ctxStack !== 'undefined') __ctxStack.length = 0;
    
    // Render the component first
    if (typeof __runts_default === 'function') {
        vnode = __runts_default(props || {});
    } else if (typeof __runts_app !== 'undefined') {
        vnode = __runts_app;
    } else {
        throw new Error('No app found: use export default or render(<App />)');
    }
    
    // Run layout effects (synchronous, after render, before paint)
    // Layout effects can trigger state changes, so we may need to re-render
    var guard = 0;
    while (__runts_has_layout_effects && guard < 10) {
        __runts_has_layout_effects = false;
        if (typeof __ctxStack !== 'undefined') __ctxStack.length = 0;
        var layoutEffects = __runts_layout_effects;
        __runts_layout_effects = [];
        for (var i = 0; i < layoutEffects.length; i++) {
            if (typeof layoutEffects[i] === 'function') layoutEffects[i]();
        }
        // Re-render after layout effects if state changed
        if (typeof __runts_default === 'function') {
            vnode = __runts_default(props || {});
        }
        guard++;
    }
    if (typeof __runts_default === 'function') {
        vnode = __runts_default(props || {});
    } else if (typeof __runts_app !== 'undefined') {
        vnode = __runts_app;
    } else {
        throw new Error('No app found: use export default or render(<App />)');
    }
    var guard = 0;
    while (__runts_has_effects && guard < 10) {
        __runts_has_effects = false;
        if (typeof __ctxStack !== 'undefined') __ctxStack.length = 0;
        var effects = __runts_effects;
        __runts_effects = [];
        for (var i = 0; i < effects.length; i++) {
            if (typeof effects[i] === 'function') effects[i]();
        }
        if (typeof __runts_default === 'function') {
            vnode = __runts_default(props || {});
        }
        guard++;
    }
    return vnode;
}"#;
