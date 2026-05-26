/**
 * runts Client Runtime
 * 
 * This is the client-side runtime for hydrating islands.
 * It's a minimal Preact-compatible runtime for use in the browser.
 * 
 * In production, this would be bundled with each island (~12KB gzip).
 */

// ============================================================================
// Signals - Fine-grained Reactivity
// ============================================================================

type Subscriber = () => void;

interface SignalState<T> {
  value: T;
  subscribers: Set<Subscriber>;
}

let currentEffect: (() => void) | null = null;
const effectCleanup = new Set<() => void>();

export function signal<T>(initialValue: T): Signal<T> {
  const state: SignalState<T> = {
    value: initialValue,
    subscribers: new Set(),
  };

  return {
    get value(): T {
      // Track subscription when accessed in effect
      if (currentEffect) {
        state.subscribers.add(currentEffect);
        effectCleanup.add(() => state.subscribers.delete(currentEffect));
      }
      return state.value;
    },
    set value(newValue: T) {
      if (!Object.is(state.value, newValue)) {
        state.value = newValue;
        // Notify all subscribers synchronously
        state.subscribers.forEach(fn => fn());
      }
    },
    peek: () => state.value,
  };
}

export function computed<T>(fn: () => T): Signal<T> {
  const sig = signal<T>(undefined as any);
  let currentValue: T;
  
  effect(() => {
    currentValue = fn();
    sig.value = currentValue;
  });
  
  return sig;
}

export function effect(fn: () => void | (() => void)): () => void {
  const cleanupFns: (() => void)[] = [];
  
  const wrappedFn = () => {
    currentEffect = wrappedFn;
    effectCleanup.delete(wrappedFn);
    const cleanup = fn();
    if (typeof cleanup === 'function') {
      cleanupFns.push(cleanup);
    }
  };
  
  wrappedFn();
  
  return () => {
    effectCleanup.delete(wrappedFn);
    cleanupFns.forEach(fn => fn());
  };
}

export function batch(fn: () => void): void {
  fn();
}

export function untrack<T>(fn: () => T): T {
  const prev = currentEffect;
  currentEffect = null;
  try {
    return fn();
  } finally {
    currentEffect = prev;
  }
}

// Signal type for exports
export interface Signal<T> {
  value: T;
  peek: () => T;
}

// ============================================================================
// Hooks - Preact-compatible state management
// ============================================================================

type StateSetter<T> = (value: T | ((prev: T) => T)) => void;

export function useState<T>(initialValue: T | (() => T)): [T, StateSetter<T>] {
  const sig = typeof initialValue === 'function'
    ? signal((initialValue as () => T)())
    : signal(initialValue);
  
  const setValue: StateSetter<T> = (value) => {
    if (typeof value === 'function') {
      sig.value = (value as (prev: T) => T)(sig.peek());
    } else {
      sig.value = value;
    }
  };
  
  return [sig.value, setValue];
}

export function useRef<T>(initialValue: T): { current: T } {
  return { current: initialValue };
}

export function useEffect(fn: () => void | (() => void), deps?: any[]): void {
  if (typeof window === 'undefined') return; // SSR guard - effects don't run on server
  
  let cleanup: (() => void) | void;
  let oldDeps: any[] | undefined;
  
  const effect = () => {
    if (deps !== undefined && oldDeps !== undefined) {
      // Check if deps changed using shallow equality
      if (deps.length === oldDeps.length && deps.every((d, i) => Object.is(d, oldDeps[i]))) {
        return;
      }
    }
    
    if (cleanup) {
      (cleanup as () => void)();
    }
    
    cleanup = fn();
    oldDeps = deps ? [...deps] : undefined;
  };
  
  // Schedule effect after paint (like React)
  if (typeof requestAnimationFrame !== 'undefined') {
    requestAnimationFrame(effect);
  } else {
    setTimeout(effect, 0);
  }
}

export function useLayoutEffect(fn: () => void | (() => void), deps?: any[]): void {
  if (typeof window === 'undefined') return; // SSR guard
  
  let cleanup: (() => void) | void;
  let oldDeps: any[] | undefined;
  
  const effect = () => {
    if (deps !== undefined && oldDeps !== undefined) {
      if (deps.length === oldDeps.length && deps.every((d, i) => Object.is(d, oldDeps[i]))) {
        return;
      }
    }
    
    if (cleanup) {
      (cleanup as () => void)();
    }
    
    cleanup = fn();
    oldDeps = deps ? [...deps] : undefined;
  };
  
  // Run synchronously before paint
  requestAnimationFrame(effect);
}

export function useMemo<T>(fn: () => T, deps: any[]): T {
  const [value, setValue] = useState<T>(() => fn());
  
  useEffect(() => {
    setValue(() => fn());
  }, deps);
  
  return value;
}

export function useCallback<T extends (...args: any[]) => any>(
  fn: T,
  deps: any[]
): T {
  return useMemo(() => fn, deps);
}

export function useReducer<S, A>(
  reducer: (state: S, action: A) => S,
  initialState: S
): [S, (action: A) => void] {
  const [state, setState] = useState<S>(initialState);
  
  const dispatch = (action: A) => {
    setState(prev => reducer(prev, action));
  };
  
  return [state, dispatch];
}

export function useContext<T>(context: Context<T>): T {
  // In a full implementation, this would read from a context provider
  // For now, return undefined (caller must handle)
  return undefined as T;
}

export function createContext<T>(defaultValue: T): Context<T> {
  return { 
    id: Math.random().toString(36).slice(2),
    defaultValue 
  };
}

export interface Context<T> {
  id: string;
  defaultValue: T;
}

// ============================================================================
// Island Hydration
// ============================================================================

export interface IslandInfo {
  name: string;
  id: string;
  hash: string;
  props: Record<string, any>;
  element: HTMLElement;
}

export interface HydrationResult {
  element: HTMLElement;
  unmount: () => void;
}

// Global registry of hydrated islands
const hydratedIslands = new Map<string, HydrationResult>();

// Find all islands in the DOM
export function findIslands(): IslandInfo[] {
  const elements = document.querySelectorAll('[data-island]');
  const islands: IslandInfo[] = [];
  
  for (const el of elements) {
    const htmlEl = el as HTMLElement;
    const name = htmlEl.getAttribute('data-island');
    const id = htmlEl.getAttribute('data-id');
    const hash = htmlEl.getAttribute('data-hash');
    
    if (name && id && hash) {
      // Try to get props from the element or a script tag
      let props: Record<string, any> = {};
      
      const propsAttr = htmlEl.getAttribute('data-props');
      if (propsAttr) {
        try {
          props = JSON.parse(propsAttr);
        } catch (e) {
          console.error(`[runts] Failed to parse props for island ${name}:`, e);
        }
      }
      
      islands.push({ 
        name, 
        id, 
        hash, 
        props, 
        element: htmlEl 
      });
    }
  }
  
  return islands;
}

// Create a hydration script for an island
function createHydrationScript(info: IslandInfo): string {
  return `
    import { signal, computed, effect, useState, useEffect, useRef, useMemo, useCallback } from '/_runts/runtime.js';
    
    // Find the element
    const el = document.querySelector('[data-id="${info.id}"]');
    if (!el) {
      console.error('[runts] Island element not found:', '${info.id}');
      throw new Error('Island element not found');
    }
    
    // Props
    const props = ${JSON.stringify(info.props)};
    
    // Placeholder content (from SSR)
    const placeholder = el.innerHTML;
    
    // Island module (loaded dynamically)
    const module = await import('/_runts/islands/${info.name}.js');
    
    // Create the island instance
    const instance = new module.default({
      props,
      placeholder,
      hydrate: true
    });
    
    // Render to the element
    instance.mount(el);
    
    // Return cleanup function
    return () => instance.unmount();
  `;
}

// Hydrate a single island
export async function hydrateIsland(info: IslandInfo): Promise<HydrationResult> {
  // Skip if already hydrated
  if (hydratedIslands.has(info.id)) {
    return hydratedIslands.get(info.id)!;
  }
  
  console.log(`[runts] Hydrating island: ${info.name}`, info.props);
  
  try {
    // Create and execute hydration script
    const script = createHydrationScript(info);
    
    // Use a module script to run the hydration
    const blob = new Blob([script], { type: 'module' });
    const url = URL.createObjectURL(blob);
    
    const cleanup = await import(/* @vite-ignore */ url);
    URL.revokeObjectURL(url);
    
    const result: HydrationResult = {
      element: info.element,
      unmount: typeof cleanup === 'function' ? cleanup : () => {}
    };
    
    hydratedIslands.set(info.id, result);
    
    console.log(`[runts] Island hydrated: ${info.name}`);
    return result;
    
  } catch (e) {
    console.error(`[runts] Failed to hydrate island ${info.name}:`, e);
    return {
      element: info.element,
      unmount: () => {}
    };
  }
}

// Hydrate all islands
export async function hydrateAll(): Promise<HydrationResult[]> {
  const islands = findIslands();
  const results: HydrationResult[] = [];
  
  for (const info of islands) {
    const result = await hydrateIsland(info);
    results.push(result);
  }
  
  console.log(`[runts] Hydrated ${results.length} islands`);
  return results;
}

// Lazy hydration with IntersectionObserver
export function hydrateOnVisible(info: IslandInfo): void {
  if (typeof IntersectionObserver === 'undefined') {
    // Fallback: hydrate immediately
    hydrateIsland(info);
    return;
  }
  
  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) {
          observer.disconnect();
          hydrateIsland(info);
        }
      }
    },
    { rootMargin: '100px' }
  );
  
  observer.observe(info.element);
}

// Hydration on interaction (click, focus, hover)
export function hydrateOnInteraction(
  info: IslandInfo,
  events: string[] = ['click', 'focus', 'hover']
): void {
  let hydrated = false;
  
  const hydrate = () => {
    if (!hydrated) {
      hydrated = true;
      for (const event of events) {
        info.element.removeEventListener(event, hydrate);
      }
      hydrateIsland(info);
    }
  };
  
  for (const event of events) {
    info.element.addEventListener(event, hydrate, { once: true, passive: true });
  }
}

// ============================================================================
// Page Data Hydration
// ============================================================================

export function getPageData<T>(): T | null {
  const el = document.getElementById('__page_props');
  if (!el) return null;
  
  try {
    return JSON.parse(el.textContent || '{}');
  } catch (e) {
    console.error('[runts] Failed to parse page data:', e);
    return null;
  }
}

// ============================================================================
// Initialization & HMR
// ============================================================================

// Auto-hydrate on DOMContentLoaded
if (typeof document !== 'undefined') {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
      hydrateAll();
    });
  } else {
    // DOM already loaded, hydrate immediately
    hydrateAll();
  }
}

// HMR client - listen for file changes
if (typeof EventSource !== 'undefined') {
  const source = new EventSource('/_runts/hmr');
  
  source.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      
      switch (data.type) {
        case 'reload':
          console.log('[runts HMR] Full reload requested');
          window.location.reload();
          break;
          
        case 'change':
          console.log('[runts HMR] File changed:', data.path);
          // For now, do a full reload
          // A smarter implementation would do hot-module replacement
          window.location.reload();
          break;
          
        case 'error':
          console.error('[runts HMR] Error:', data.message);
          break;
          
        default:
          console.log('[runts HMR] Unknown event:', data);
      }
    } catch (e) {
      console.error('[runts HMR] Failed to parse event:', e);
    }
  };
  
  source.onerror = () => {
    console.warn('[runts HMR] Connection lost, retrying...');
    // EventSource will automatically reconnect
  };
}

// Export everything for use in islands
export default {
  // Signals
  signal,
  computed,
  effect,
  batch,
  untrack,
  
  // Hooks
  useState,
  useRef,
  useEffect,
  useLayoutEffect,
  useMemo,
  useCallback,
  useReducer,
  useContext,
  createContext,
  
  // Islands
  findIslands,
  hydrateIsland,
  hydrateAll,
  hydrateOnVisible,
  hydrateOnInteraction,
  
  // Utils
  getPageData,
};
