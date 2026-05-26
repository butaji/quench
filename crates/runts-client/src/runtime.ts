/**
 * runts Client Runtime
 * 
 * Minimal JavaScript runtime for island hydration and signal synchronization.
 * Target: ~5KB gzipped for full functionality.
 * 
 * @module runts
 */

// ============================================================================
// Signal System
// ============================================================================

export interface SignalOptions<T> {
  equals?: (a: T, b: T) => boolean;
}

export class Signal<T> {
  #value: T;
  #subscribers: Set<(value: T) => void> = new Set();
  #equals: (a: T, b: T) => boolean;
  
  constructor(value: T, options?: SignalOptions<T>) {
    this.#value = value;
    this.#equals = options?.equals ?? ((a, b) => a === b);
  }
  
  get value(): T {
    return this.#value;
  }
  
  set value(newValue: T) {
    if (!this.#equals(this.#value, newValue)) {
      this.#value = newValue;
      this.#notify();
    }
  }
  
  peek(): T {
    return this.#value;
  }
  
  set(newValue: T): void {
    this.value = newValue;
  }
  
  update(fn: (value: T) => T): void {
    this.value = fn(this.#value);
  }
  
  subscribe(fn: (value: T) => void): () => void {
    this.#subscribers.add(fn);
    return () => this.#subscribers.delete(fn);
  }
  
  #notify(): void {
    for (const fn of this.#subscribers) {
      fn(this.#value);
    }
  }
}

export function signal<T>(initial: T, options?: SignalOptions<T>): Signal<T> {
  return new Signal(initial, options);
}

export function computed<T>(fn: () => T): Signal<T> {
  const sig = new Signal<T>(undefined as any);
  let dirty = true;
  let current: T;
  
  const recompute = () => {
    if (dirty) {
      current = fn();
      dirty = false;
    }
    return current;
  };
  
  // Create a reactive signal that auto-computes
  const computedSig = new Signal<T>(undefined as any, {
    equals: () => {
      const newVal = fn();
      if (newVal !== current) {
        current = newVal;
        return true;
      }
      return false;
    }
  });
  
  // Subscribe to dependencies
  effect(() => {
    current = fn();
    computedSig.value = current;
  });
  
  return computedSig;
}

export function effect(fn: () => void | (() => void)): () => void {
  const cleanup = fn();
  return () => {
    if (typeof cleanup === 'function') {
      cleanup();
    }
  };
}

// ============================================================================
// Batch Updates
// ============================================================================

let batchDepth = 0;
let batchedEffects: (() => void)[] = [];

export function batch(fn: () => void): void {
  batchDepth++;
  try {
    fn();
    flush();
  } finally {
    batchDepth--;
  }
}

function flush(): void {
  if (batchDepth > 0) return;
  const effects = batchedEffects.splice(0);
  for (const effect of effects) {
    effect();
  }
}

// ============================================================================
// Island Hydration
// ============================================================================

export enum IslandMode {
  Eager = 'eager',
  Lazy = 'lazy',
  Interaction = 'interaction',
  Visible = 'visible'
}

interface IslandInstance {
  id: string;
  name: string;
  props: any;
  mode: IslandMode;
  state: 'pending' | 'hydrating' | 'hydrated' | 'error';
  element: HTMLElement | null;
  cleanup?: () => void;
}

const islands: Map<string, IslandInstance> = new Map();
const islandHandlers: Map<string, (props: any) => void> = new Map();

// Initialize on DOM ready
if (typeof document !== 'undefined') {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
}

function init(): void {
  discoverIslands();
  setupIntersectionObserver();
  setupInteractionObserver();
}

function discoverIslands(): void {
  const elements = document.querySelectorAll('[data-island]');
  
  for (const el of elements) {
    if (!(el instanceof HTMLElement)) continue;
    
    const name = el.dataset.island!;
    const id = el.dataset.id!;
    const mode = (el.dataset.hydration as IslandMode) ?? IslandMode.Lazy;
    
    // Parse props from script tag
    const scriptId = `island-data-${id}`;
    const script = document.getElementById(scriptId);
    const props = script ? JSON.parse(script.textContent || '{}') : {};
    
    const instance: IslandInstance = {
      id,
      name,
      props,
      mode,
      state: 'pending',
      element: el
    };
    
    islands.set(id, instance);
    
    // Hydrate based on mode
    switch (mode) {
      case IslandMode.Eager:
        hydrateIsland(id);
        break;
      case IslandMode.Lazy:
      case IslandMode.Visible:
        // Will be hydrated by intersection observer
        break;
      case IslandMode.Interaction:
        // Will be hydrated on first interaction
        break;
    }
  }
}

let intersectionObserver: IntersectionObserver | null = null;
let interactionHandler: ((e: Event) => void) | null = null;

function setupIntersectionObserver(): void {
  intersectionObserver = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) {
          const el = entry.target as HTMLElement;
          const id = el.dataset.id;
          if (id) {
            const island = islands.get(id);
            if (island?.mode === IslandMode.Lazy || island?.mode === IslandMode.Visible) {
              hydrateIsland(id);
            }
          }
        }
      }
    },
    { rootMargin: '100px' }
  );
  
  // Observe all lazy/visible islands
  for (const island of islands.values()) {
    if (island.element && (island.mode === IslandMode.Lazy || island.mode === IslandMode.Visible)) {
      intersectionObserver?.observe(island.element);
    }
  }
}

function setupInteractionObserver(): void {
  interactionHandler = (e: Event) => {
    const target = e.target as HTMLElement;
    const islandEl = target.closest('[data-island]');
    
    if (islandEl instanceof HTMLElement) {
      const id = islandEl.dataset.id;
      if (id) {
        const island = islands.get(id);
        if (island?.mode === IslandMode.Interaction && island.state === 'pending') {
          // Remove this listener after first interaction
          document.removeEventListener('click', interactionHandler!, true);
          document.removeEventListener('focus', interactionHandler!, true);
          hydrateIsland(id);
        }
      }
    }
  };
  
  document.addEventListener('click', interactionHandler, true);
  document.addEventListener('focus', interactionHandler, true);
}

export async function hydrateIsland(id: string): Promise<void> {
  const island = islands.get(id);
  if (!island || island.state !== 'pending') return;
  
  island.state = 'hydrating';
  island.element?.setAttribute('data-hydrating', 'true');
  
  try {
    const handler = islandHandlers.get(island.name);
    if (handler) {
      handler(island.props);
      island.state = 'hydrated';
      island.element?.removeAttribute('data-hydrating');
      island.element?.setAttribute('data-hydrated', 'true');
    } else {
      throw new Error(`No handler registered for island: ${island.name}`);
    }
  } catch (err) {
    island.state = 'error';
    island.element?.setAttribute('data-error', String(err));
    console.error(`[runts] Failed to hydrate island ${id}:`, err);
  }
}

export function registerIsland(name: string, handler: (props: any) => void): void {
  islandHandlers.set(name, handler);
}

export function getIsland(id: string): IslandInstance | undefined {
  return islands.get(id);
}

// ============================================================================
// Event Delegation
// ============================================================================

export function setupEventDelegation(): void {
  document.addEventListener('click', handleClick, true);
  document.addEventListener('input', handleInput, true);
  document.addEventListener('change', handleChange, true);
  document.addEventListener('submit', handleSubmit, true);
  document.addEventListener('focus', handleFocus, true);
  document.addEventListener('blur', handleBlur, true);
  document.addEventListener('keydown', handleKeyDown, true);
  document.addEventListener('keyup', handleKeyUp, true);
}

function handleClick(e: Event): void {
  const target = e.target as HTMLElement;
  const delegated = target.closest('[data-on-click]');
  if (delegated) {
    const fnName = delegated.getAttribute('data-on-click');
    invokeHandler(fnName, e);
  }
}

function handleInput(e: Event): void {
  const target = e.target as HTMLInputElement;
  const delegated = target.closest('[data-on-input]');
  if (delegated) {
    const fnName = delegated.getAttribute('data-on-input');
    invokeHandler(fnName, e);
  }
}

function handleChange(e: Event): void {
  const target = e.target as HTMLElement;
  const delegated = target.closest('[data-on-change]');
  if (delegated) {
    const fnName = delegated.getAttribute('data-on-change');
    invokeHandler(fnName, e);
  }
}

function handleSubmit(e: Event): void {
  const target = e.target as HTMLElement;
  const delegated = target.closest('[data-on-submit]');
  if (delegated) {
    const fnName = delegated.getAttribute('data-on-submit');
    invokeHandler(fnName, e);
  }
}

function handleFocus(e: Event): void {
  const target = e.target as HTMLElement;
  const delegated = target.closest('[data-on-focus]');
  if (delegated) {
    const fnName = delegated.getAttribute('data-on-focus');
    invokeHandler(fnName, e);
  }
}

function handleBlur(e: Event): void {
  const target = e.target as HTMLElement;
  const delegated = target.closest('[data-on-blur]');
  if (delegated) {
    const fnName = delegated.getAttribute('data-on-blur');
    invokeHandler(fnName, e);
  }
}

function handleKeyDown(e: Event): void {
  const target = e.target as HTMLElement;
  const delegated = target.closest('[data-on-key-down]');
  if (delegated) {
    const fnName = delegated.getAttribute('data-on-key-down');
    invokeHandler(fnName, e);
  }
}

function handleKeyUp(e: Event): void {
  const target = e.target as HTMLElement;
  const delegated = target.closest('[data-on-key-up]');
  if (delegated) {
    const fnName = delegated.getAttribute('data-on-key-up');
    invokeHandler(fnName, e);
  }
}

function invokeHandler(fnName: string | null, e: Event): void {
  if (!fnName) return;
  
  // Look up handler in global scope
  const handler = (window as any)[fnName];
  if (typeof handler === 'function') {
    try {
      handler(e);
    } catch (err) {
      console.error(`[runts] Error in handler ${fnName}:`, err);
    }
  }
}

// ============================================================================
// HTML Utilities
// ============================================================================

export function escapeHtml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

export function html(strings: TemplateStringsArray, ...values: any[]): string {
  let result = '';
  for (let i = 0; i < strings.length; i++) {
    result += strings[i];
    if (i < values.length) {
      const value = values[i];
      if (value == null) {
        result += '';
      } else if (typeof value === 'string') {
        result += escapeHtml(value);
      } else if (typeof value === 'number' || typeof value === 'boolean') {
        result += String(value);
      } else if (Array.isArray(value)) {
        result += value.map(v => 
          typeof v === 'string' ? escapeHtml(v) : String(v)
        ).join('');
      } else {
        result += String(value);
      }
    }
  }
  return result;
}

// ============================================================================
// State Management
// ============================================================================

interface StoreState {
  [key: string]: any;
}

const stores: Map<string, StoreState> = new Map();

export function createStore<T extends StoreState>(initial: T): T {
  const store = reactive(initial);
  stores.set('default', store);
  return store as T;
}

export function getStore<T extends StoreState>(name = 'default'): T | undefined {
  return stores.get(name) as T | undefined;
}

export function reactive<T extends object>(obj: T): T {
  return new Proxy(obj, {
    get(target, key, receiver) {
      const value = Reflect.get(target, key, receiver);
      if (typeof value === 'object' && value !== null) {
        return reactive(value);
      }
      return value;
    },
    set(target, key, value, receiver) {
      const result = Reflect.set(target, key, value, receiver);
      if (result) {
        // Trigger updates
        notifyChange(String(key));
      }
      return result;
    }
  });
}

function notifyChange(key: string): void {
  // Dispatch custom event for debugging/inspection
  if (typeof document !== 'undefined') {
    document.dispatchEvent(new CustomEvent('store-change', { 
      detail: { key } 
    }));
  }
}

// ============================================================================
// Routing (Client-side)
// ============================================================================

export function navigate(url: string, options?: { replace?: boolean }): void {
  if (typeof history !== 'undefined') {
    if (options?.replace) {
      history.replaceState(null, '', url);
    } else {
      history.pushState(null, '', url);
    }
    // Dispatch popstate to trigger route handlers
    window.dispatchEvent(new PopStateEvent('popstate'));
  }
}

export function onMount(fn: () => void): void {
  if (typeof document !== 'undefined') {
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', fn);
    } else {
      fn();
    }
  }
}

// ============================================================================
// Initialization
// ============================================================================

export function initRuntime(): void {
  setupEventDelegation();
  discoverIslands();
  
  // Signal that runtime is ready
  if (typeof window !== 'undefined') {
    (window as any).__RUNTS_READY__ = true;
    window.dispatchEvent(new Event('runts-ready'));
  }
}

// Auto-initialize
if (typeof document !== 'undefined') {
  initRuntime();
}

// ============================================================================
// Exports
// ============================================================================

export default {
  Signal,
  signal,
  computed,
  effect,
  batch,
  html,
  escapeHtml,
  registerIsland,
  hydrateIsland,
  getIsland,
  IslandMode,
  createStore,
  getStore,
  reactive,
  navigate,
  onMount
};
