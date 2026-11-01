// 😀 UTF-16 project query fixture
import type { User, Branded, Choice, Keys, Box, Maybe, ReadonlyTuple, Handler, Direction, Tagged, Nested } from "./types";
import { unique } from "./types";

export function probe_user(param_user: User): void { void param_user; }
export function probe_branded(param_branded: Branded): void { void param_branded; }
export function probe_choice(param_choice: Choice): void { void param_choice; }
export function probe_keys(param_keys: Keys): void { void param_keys; }
export function probe_box(param_box: Box<number>): void { void param_box; }
export function probe_maybe(param_maybe: Maybe<string | undefined>): void { void param_maybe; }
export function probe_tuple(param_tuple: ReadonlyTuple): void { void param_tuple; }
export function probe_handler(param_handler: Handler<User>): void { void param_handler; }
export function probe_direction(param_direction: Direction): void { void param_direction; }
export function probe_tagged(param_tagged: Tagged): void { void param_tagged; }
export function probe_nested(param_nested: Nested.Value): void { void param_nested; }
export function probe_literal(param_literal: "ready"): void { void param_literal; }
export function probe_union(param_union: string | number): void { void param_union; }
export function probe_intersection(param_intersection: {a: number} & {b: string}): void { void param_intersection; }
export function probe_indexed(param_indexed: User["id"]): void { void param_indexed; }
export function probe_array(param_array: readonly (string | number)[]): void { void param_array; }
export function probe_optional(param_optional: number | undefined): void { void param_optional; }
export function probe_nullable(param_nullable: string | null): void { void param_nullable; }
export function probe_conditional(param_conditional: Maybe<number>): void { void param_conditional; }
export function probe_mapped(param_mapped: Partial<User>): void { void param_mapped; }
export function probe_record(param_record: Record<Keys, unknown>): void { void param_record; }
export function probe_promise(param_promise: Promise<User>): void { void param_promise; }
export function probe_awaited(param_awaited: Awaited<Promise<User>>): void { void param_awaited; }
export function probe_extract(param_extract: Extract<string | number, string>): void { void param_extract; }
export function probe_exclude(param_exclude: Exclude<string | number, string>): void { void param_exclude; }
export function probe_nonnullable(param_nonnullable: NonNullable<string | null>): void { void param_nonnullable; }
export function probe_template(param_template: `event:${string}`): void { void param_template; }
export function probe_function(param_function: (x: number, y?: string) => boolean): void { void param_function; }
export function probe_constructor(param_constructor: new (x: number) => User): void { void param_constructor; }
export function probe_predicate(param_predicate: User): void { void param_predicate; }
export function probe_thisarg(param_thisarg: ThisType<User>): void { void param_thisarg; }
export function probe_sym(param_sym: typeof unique): void { void param_sym; }
function overload(x: string): string;
function overload(x: number): number;
function overload(x: string | number) { return x; }
export function probe_optionalprop(param_optionalprop: { value?: string }): void { void param_optionalprop; }
export function probe_tupleRest(param_tupleRest: [string, ...number[]]): void { void param_tupleRest; }
export function probe_promiseLike(param_promiseLike: PromiseLike<number>): void { void param_promiseLike; }
export function probe_bigint(param_bigint: bigint): void { void param_bigint; }
export function probe_symbol(param_symbol: symbol): void { void param_symbol; }
export function probe_never(param_never: never): void { void param_never; }
export function probe_unknown(param_unknown: unknown): void { void param_unknown; }
export function probe_any(param_any: any): void { void param_any; }
export function probe_void(param_void: void): void { void param_void; }
export function probe_object(param_object: object): void { void param_object; }
export function probe_functionObj(param_functionObj: Function): void { void param_functionObj; }
export function probe_date(param_date: Date): void { void param_date; }
export function probe_regex(param_regex: RegExp): void { void param_regex; }
export function probe_map(param_map: Map<string, number>): void { void param_map; }
export function probe_set(param_set: Set<User>): void { void param_set; }
export function probe_weakmap(param_weakmap: WeakMap<object, User>): void { void param_weakmap; }
export function probe_iterator(param_iterator: IterableIterator<User>): void { void param_iterator; }
export function probe_thisType(param_thisType: ThisType<User>): void { void param_thisType; }
export function generic<T>(param_generic: T): void {}
export function constrained<T extends string | number>(param_constrained: T): void {}
export function defaulted<T = User>(param_defaulted: T): void {}
export function constgeneric<const T extends readonly unknown[]>(param_constgeneric: T): void {}
