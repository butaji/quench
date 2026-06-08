// ink-template-literal-types example — demonstrates template literal types
//
// Template literal types (TypeScript 4.1) allow constructing new string
// types using template literal syntax. They enable powerful type-level
// string manipulation.
//
// All types are erased at compile time, so there is no runtime impact.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Basic template literal types
type Color = 'red' | 'green' | 'blue';
type BgColor = `bg-${Color}`;

const bgRed: BgColor = 'bg-red';
const bgGreen: BgColor = 'bg-green';
const bgBlue: BgColor = 'bg-blue';

// Event names
type ClickEvent = 'click';
type HoverEvent = 'hover';
type EventName = `on${ClickEvent | HoverEvent}`;

const onClick: EventName = 'onclick';
const onHover: EventName = 'onhover';

// CSS-like property names
type Direction = 'top' | 'right' | 'bottom' | 'left';
type Margin = `margin-${Direction}`;

const marginTop: Margin = 'margin-top';
const marginLeft: Margin = 'margin-left';

// Path types
type ApiVersion = 'v1' | 'v2';
type ApiPath = `/api/${ApiVersion}/users`;

const path1: ApiPath = '/api/v1/users';
const path2: ApiPath = '/api/v2/users';

// Combined with utility types
type Props = 'color' | 'background';
type DataProps = `data-${Props}`;

const dataColor: DataProps = 'data-color';
const dataBackground: DataProps = 'data-background';

// String concatenation at type level
type Prefix = 'prefix-';
type Suffix = '-suffix';
type Combined = `${Prefix}${Color}${Suffix}`;

const combined1: Combined = 'prefix-red-suffix';
const combined2: Combined = 'prefix-blue-suffix';

export default function TemplateLiteralTypesDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Template Literal Types Demo</Text>
      <Text></Text>
      <Text>Background colors:</Text>
      <Text>  bgRed: {bgRed}</Text>
      <Text>  bgGreen: {bgGreen}</Text>
      <Text>  bgBlue: {bgBlue}</Text>
      <Text></Text>
      <Text>Event names:</Text>
      <Text>  onClick: {onClick}</Text>
      <Text>  onHover: {onHover}</Text>
      <Text></Text>
      <Text>Margin properties:</Text>
      <Text>  marginTop: {marginTop}</Text>
      <Text>  marginLeft: {marginLeft}</Text>
      <Text></Text>
      <Text>API paths:</Text>
      <Text>  path1: {path1}</Text>
      <Text>  path2: {path2}</Text>
      <Text></Text>
      <Text>Data attributes:</Text>
      <Text>  dataColor: {dataColor}</Text>
      <Text>  dataBackground: {dataBackground}</Text>
      <Text></Text>
      <Text>Combined strings:</Text>
      <Text>  combined1: {combined1}</Text>
      <Text>  combined2: {combined2}</Text>
    </Box>
  );
}
