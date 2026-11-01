export interface User { id: number; name: string; active: boolean; }
export type Branded = number & { readonly __brand: unique symbol };
export type Choice = {kind: "ok"; value: number} | {kind: "err"; message: string};
export type Keys = keyof User;
export type Box<T> = { readonly value: T };
export type Maybe<T> = T extends null | undefined ? never : T;
export type ReadonlyTuple = readonly [string, number?];
export type Handler<T> = (value: T) => Promise<Maybe<T>>;
export enum Direction { Up, Down }
export declare const unique: unique symbol;
export type Tagged = { [unique]: true; value: number };
export namespace Nested { export interface Value { n: number } }
