// Type definitions file
// These types are imported using the import() type syntax

export interface User {
  name: string;
  age: number;
}

export interface Product {
  id: string;
  price: number;
}

export type ID = string | number;

export type Status = 'active' | 'inactive' | 'pending';

export interface Config {
  debug: boolean;
  maxItems: number;
}
