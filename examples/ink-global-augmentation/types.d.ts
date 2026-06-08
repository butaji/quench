// Type declaration file demonstrating global and module augmentation.
// These are purely type-level constructs that are erased at runtime.

// Global augmentation — extends the global scope
declare global {
  interface Window {
    appVersion: string;
  }
  var __BUILD_TIME__: string;
}

// Module augmentation — extends an existing module
declare module 'ink' {
  interface BoxProps {
    'data-testid'?: string;
  }
  interface TextProps {
    'data-testid'?: string;
  }
}

// Empty export makes this a module (required for module augmentation)
export {};
