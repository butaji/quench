// 😀 UTF-16 source offset prefix
 type Shape = { a: number; b?: string; readonly c: boolean };
 type Named = { name: string };
 class Base { x = 1; y?: string; }
 enum Direction { Up, Down }
 declare const unique: unique symbol;
 type Unique = typeof unique;
 type Box<T> = { value: T };
 type Pair<T, U> = [T, U];
 type Outer<T> = { inner: T };
 type Fn = (x: number) => string;
 type Ctor = new (x: number) => Shape;
 type ToText<T> = T extends string ? `text:${T}` : T extends number ? `${T}px` : never;
 type Node<T> = { value: T; next?: Node<T> };
 type Recursive<T> = T extends readonly (infer E)[] ? E : T;
 type Maybe<T> = T | undefined;
 type DeepReadonly<T> = { readonly [K in keyof T]: T[K] };
 declare function makeShape(): Shape;
export function probe000(p000: string) { return p000; }
export function probe001(p001: number) { return p001; }
export function probe002(p002: boolean) { return p002; }
export function probe003(p003: symbol) { return p003; }
export function probe004(p004: bigint) { return p004; }
export function probe005(p005: object) { return p005; }
export function probe006(p006: unknown) { return p006; }
export function probe007(p007: any) { return p007; }
export function probe008(p008: never) { return p008; }
export function probe009(p009: void) { return p009; }
export function probe010(p010: null) { return p010; }
export function probe011(p011: undefined) { return p011; }
export function probe012(p012: 1) { return p012; }
export function probe013(p013: "literal") { return p013; }
export function probe014(p014: true) { return p014; }
export function probe015(p015: 1 | 2) { return p015; }
export function probe016(p016: "a" | "b") { return p016; }
export function probe017(p017: string | number) { return p017; }
export function probe018(p018: number & { brand: true }) { return p018; }
export function probe019(p019: number[]) { return p019; }
export function probe020(p020: Array<string>) { return p020; }
export function probe021(p021: readonly string[]) { return p021; }
export function probe022(p022: [number, string]) { return p022; }
export function probe023(p023: readonly [number, string?]) { return p023; }
export function probe024(p024: [head: number, ...tail: string[]]) { return p024; }
export function probe025(p025: Box<number>) { return p025; }
export function probe026(p026: Box<Shape>) { return p026; }
export function probe027(p027: Pair<string, number>) { return p027; }
export function probe028(p028: Promise<Shape>) { return p028; }
export function probe029(p029: PromiseLike<number>) { return p029; }
export function probe030(p030: Shape) { return p030; }
export function probe031(p031: Named) { return p031; }
export function probe032(p032: Base) { return p032; }
export function probe033(p033: Direction) { return p033; }
export function probe034(p034: Direction.Up) { return p034; }
export function probe035(p035: typeof Direction) { return p035; }
export function probe036(p036: Unique) { return p036; }
export function probe037(p037: keyof Shape) { return p037; }
export function probe038(p038: Shape["a"]) { return p038; }
export function probe039(p039: Shape["b"]) { return p039; }
export function probe040(p040: Partial<Shape>) { return p040; }
export function probe041(p041: Required<Shape>) { return p041; }
export function probe042(p042: Readonly<Shape>) { return p042; }
export function probe043(p043: Pick<Shape, "a" | "c">) { return p043; }
export function probe044(p044: Omit<Shape, "b">) { return p044; }
export function probe045(p045: Record<string, number>) { return p045; }
export function probe046(p046: Record<"x" | "y", boolean>) { return p046; }
export function probe047(p047: Exclude<string | number | null, string>) { return p047; }
export function probe048(p048: Extract<string | number, string>) { return p048; }
export function probe049(p049: NonNullable<string | null>) { return p049; }
export function probe050(p050: ReturnType<typeof makeShape>) { return p050; }
export function probe051(p051: Parameters<typeof makeShape>) { return p051; }
export function probe052(p052: Awaited<Promise<Shape>>) { return p052; }
export function probe053(p053: ToText<string>) { return p053; }
export function probe054(p054: ToText<number>) { return p054; }
export function probe055(p055: `event:${string}`) { return p055; }
export function probe056(p056: `${number}px`) { return p056; }
export function probe057(p057: Fn) { return p057; }
export function probe058(p058: Ctor) { return p058; }
export function probe059(p059: (x: number, y?: string) => boolean) { return p059; }
export function probe060(p060: new (x: Shape) => Base) { return p060; }
export function probe061(p061: { x: number; y?: string }) { return p061; }
export function probe062(p062: { readonly x: number; [key: string]: unknown }) { return p062; }
export function probe063(p063: Shape | Named) { return p063; }
export function probe064(p064: Shape & Named) { return p064; }
export function probe065(p065: Box<string | number>) { return p065; }
export function probe066(p066: Box<never>) { return p066; }
export function probe067(p067: Box<unknown>) { return p067; }
export function probe068(p068: Box<any>) { return p068; }
export function probe069(p069: Outer<Box<number>>) { return p069; }
export function probe070(p070: Node<string>) { return p070; }
export function probe071(p071: Recursive<string[]>) { return p071; }
export function probe072(p072: Maybe<number>) { return p072; }
export function probe073(p073: DeepReadonly<Shape>) { return p073; }
export function probe074(p074: Direction | "other") { return p074; }
export function probe075(p075: 1 | 2 | 3) { return p075; }
export function probe076(p076: "north" | "south") { return p076; }
export function probe078(p078: number | undefined) { return p078; }
export function probe079(p079: string | null) { return p079; }
export function probe080(p080: boolean | undefined) { return p080; }
export function probe081(p081: readonly number[]) { return p081; }
export function probe082(p082: ReadonlyArray<Shape>) { return p082; }
export function probe083(p083: Map<string, Shape>) { return p083; }
export function probe084(p084: Set<number>) { return p084; }
export function probe085(p085: WeakMap<object, string>) { return p085; }
export function probe086(p086: Date) { return p086; }
export function probe087(p087: Error) { return p087; }
export function probe088(p088: RegExp) { return p088; }
export function probe089(p089: Int8Array) { return p089; }
export function probe090(p090: ArrayBuffer) { return p090; }
export function probe091(p091: Promise<void>) { return p091; }
export function probe092(p092: (this: Shape, value: number) => string) { return p092; }
