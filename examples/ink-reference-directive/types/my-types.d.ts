// Type declaration file for custom types

declare module "my-module" {
  export interface MyOptions {
    name: string;
    value: number;
  }

  export function doSomething(opts: MyOptions): void;
}

// Augment the window interface
interface Window {
  myCustomProperty: string;
}
