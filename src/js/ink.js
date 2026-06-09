// Ink API shim for TuiBridge
// Provides the same API as npm:ink but using React Reconciler + bridge functions

// Track render instances
const renderInstances = new Map();

// Current exit handler
let exitHandler = null;

// ========================================================================
// Component Types
// ========================================================================

const Box = 'ink-box';
const Text = 'ink-text';
const Static = 'ink-static';
const Newline = 'ink-newline';
const Spacer = 'ink-spacer';
const Provider = 'ink-provider';

// ========================================================================
// Focus Context (simplified)
// ========================================================================

const FocusContext = {
  active: true,
  focusManager: null,
};

// ========================================================================
// useInput hook implementation
// ========================================================================

function useInput(handler, options = {}) {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;
  
  useEffect(() => {
    const callback = (event) => {
      if (options.isActive !== false) {
        handlerRef.current(event.key || '', event);
      }
    };
    
    const id = globalThis.__ink_register_input(
      `function(handler) { return function(event) { handler(event); } }`
    );
    
    // Store the handler ID for cleanup
    globalThis.__ink_active_handlers = globalThis.__ink_active_handlers || {};
    globalThis.__ink_active_handlers[id] = handlerRef;
    
    return () => {
      globalThis.__ink_unregister_input(id);
      delete globalThis.__ink_active_handlers[id];
    };
  }, [options.isActive]);
}

// ========================================================================
// useApp hook implementation
// ========================================================================

function useApp() {
  return useMemo(() => ({
    exit: (err) => globalThis.__ink_exit(err ? 1 : 0),
    stdout: { 
      write: (data) => globalThis.__ink_stdout_write ? globalThis.__ink_stdout_write(data) : null 
    },
    stdin: { 
      isRawModeSupported: () => true 
    },
    stderr: { 
      write: (data) => globalThis.__ink_stderr_write ? globalThis.__ink_stderr_write(data) : null 
    },
  }), []);
}

// ========================================================================
// useStdin hook implementation  
// ========================================================================

function useStdin() {
  const stdinRef = useRef(null);
  
  if (!stdinRef.current) {
    stdinRef.current = {
      isRawMode: () => globalThis.__ink_stdin_is_raw ? globalThis.__ink_stdin_is_raw() : false,
    };
  }
  
  return stdinRef.current;
}

// ========================================================================
// useStdout hook implementation
// ========================================================================

function useStdout() {
  const stdoutRef = useRef(null);
  
  if (!stdoutRef.current) {
    stdoutRef.current = {
      columns: globalThis.__ink_get_terminal_size ? 
        (() => { const s = globalThis.__ink_get_terminal_size(); return parseInt(s.split(',')[0]); })() : 80,
      write: (data) => globalThis.__ink_stdout_write ? globalThis.__ink_stdout_write(data) : null,
    };
  }
  
  return stdoutRef.current;
}

// ========================================================================
// useStderr hook implementation
// ========================================================================

function useStderr() {
  const stderrRef = useRef(null);
  
  if (!stderrRef.current) {
    stderrRef.current = {
      write: (data) => globalThis.__ink_stderr_write ? globalThis.__ink_stderr_write(data) : null,
    };
  }
  
  return stderrRef.current;
}

// ========================================================================
// useFocus & useFocusManager hooks (simplified)
// ========================================================================

function useFocus() {
  return useMemo(() => ({
    isFocused: () => FocusContext.active,
  }), []);
}

function useFocusManager() {
  return useMemo(() => ({
    focus: () => { FocusContext.active = true; },
    blur: () => { FocusContext.active = false; },
    next: () => {},
    previous: () => {},
  }), []);
}

// ========================================================================
// measureElement function
// ========================================================================

function measureElement(ref) {
  if (!ref?.current?.id) return undefined;
  const result = globalThis.__ink_measure_element(String(ref.current.id));
  if (result === 'null' || !result) return undefined;
  const [w, h] = result.split(',').map(Number);
  return { width: w, height: h };
}

// ========================================================================
// Text component (handles children and styling)
// ========================================================================

function Text({ children, ...props }) {
  const text = Array.isArray(children) ? children.join('') : String(children || '');
  return text;
}

// ========================================================================
// Box component (container with flex layout)
// ========================================================================

function Box({ children, ...props }) {
  return children;
}

// ========================================================================
// Static component (for elements that don't re-render)
// In a full implementation, this would bypass reconciler updates
// ========================================================================

function Static({ children }) {
  return children;
}

// ========================================================================
// Newline component
// ========================================================================

function Newline() {
  return '\n';
}

// ========================================================================
// Spacer component
// ========================================================================

function Spacer({ width = 1, height = 1 }) {
  return ' '.repeat(width);
}

// ========================================================================
// setInterval/setTimeout polyfills (using Rust timers)
// ========================================================================

let timerId = 0;
const timers = new Map();

function inkSetTimeout(callback, delay) {
  const id = ++timerId;
  timers.set(id, { callback, delay, type: 'timeout' });
  // Timer will be polled/processed by the event loop
  return id;
}

function inkClearTimeout(id) {
  timers.delete(id);
}

function inkSetInterval(callback, interval) {
  const id = ++timerId;
  timers.set(id, { callback, interval, type: 'interval', lastRun: Date.now() });
  return id;
}

function inkClearInterval(id) {
  timers.delete(id);
}

// ========================================================================
// render() function - main entry point
// ========================================================================

function render(element, options = {}) {
  const rootId = globalThis.__ink_create_root();
  const container = { id: rootId, isContainer: true };
  
  // Build the tree from React element
  let unmountCallback = null;
  
  // In a full implementation, this would use react-reconciler
  // For now, we create a simple synchronous renderer
  
  function mountElement(el, parentId) {
    if (!el || el === null || el === false || el === undefined) {
      return;
    }
    
    // Handle arrays of children
    if (Array.isArray(el)) {
      el.forEach(child => mountElement(child, parentId));
      return;
    }
    
    // Handle strings/text
    if (typeof el === 'string' || typeof el === 'number') {
      const textId = globalThis.__ink_create_text_node(String(el));
      globalThis.__ink_append_child(String(parentId), String(textId));
      return;
    }
    
    // Handle objects with type (React elements)
    if (typeof el === 'object' && el.type) {
      const { type, props } = el;
      const children = props?.children;
      const newProps = { ...props };
      delete newProps.children;
      
      // Create the node
      const propsJson = JSON.stringify(newProps);
      const nodeId = globalThis.__ink_create_node(type, propsJson);
      
      // Append to parent
      globalThis.__ink_append_child(String(parentId), String(nodeId));
      
      // Mount children recursively
      if (children) {
        mountElement(children, nodeId);
      }
      
      return;
    }
  }
  
  // Mount the element tree
  mountElement(element, rootId);
  
  // Trigger initial layout and render
  globalThis.__ink_commit();
  
  // Return instance with cleanup methods
  return {
    waitUntilExit: () => {
      return new Promise((resolve) => {
        // Poll for exit condition
        const check = () => {
          if (globalThis.__ink_should_exit()) {
            resolve();
          } else {
            setTimeout(check, 50);
          }
        };
        check();
      });
    },
    
    unmount: () => {
      globalThis.__ink_destroy_root(rootId);
      timers.forEach((_, id) => {
        if (id) inkClearTimeout(id);
      });
      timers.clear();
    },
    
    rerender: (newElement) => {
      // Unmount and remount
      globalThis.__ink_destroy_root(rootId);
      const newRootId = globalThis.__ink_create_root();
      container.id = newRootId;
      mountElement(newElement, newRootId);
      globalThis.__ink_commit();
    },
  };
}

// ========================================================================
// Export everything
// ========================================================================

// Minimal React hooks (for simple components)
const React = {
  useState: (initial) => {
    const ref = useRef();
    if (!ref.current) {
      ref.current = [initial, () => {}];
    }
    return ref.current;
  },
  
  useEffect: () => {},
  useRef: (initial) => ({ current: initial }),
  useMemo: (fn) => fn(),
  useCallback: (fn) => fn,
  useContext: () => ({}),
  createContext: () => ({ Provider: ({children}) => children }),
};

const useState = React.useState;
const useEffect = React.useEffect;
const useRef = React.useRef;
const useMemo = React.useMemo;
const useCallback = React.useCallback;
const useContext = React.useContext;
const createContext = React.createContext;

// Export as ES module or global
if (typeof module !== 'undefined' && module.exports) {
  module.exports = {
    render,
    Box,
    Text,
    Static,
    Newline,
    Spacer,
    useInput,
    useApp,
    useStdin,
    useStdout,
    useStderr,
    useFocus,
    useFocusManager,
    measureElement,
    // Also expose React for convenience
    React,
    useState,
    useEffect,
    useRef,
    useMemo,
    useCallback,
    useContext,
    createContext,
    // Polyfills
    setTimeout: inkSetTimeout,
    clearTimeout: inkClearTimeout,
    setInterval: inkSetInterval,
    clearInterval: inkClearInterval,
  };
}
