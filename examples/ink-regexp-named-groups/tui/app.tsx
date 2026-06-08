// ink-regexp-named-groups example — demonstrates RegExp named capture groups
//
// This example exercises RegExp named capture groups including:
// - (?<name>...) syntax for named groups
// - .groups property for accessing named captures
// - Destructuring with named groups
// - match.groups with nullish coalescing
// - Multiple named groups in a single regex
// - Named groups with different regex flags
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Basic named group extraction
const emailRegex = /(?<user>[^@]+)@(?<domain>[^@]+)/;
const email = 'alice@example.com';
const emailMatch = email.match(emailRegex);
const emailUser = emailMatch?.groups?.user || 'none';
const emailDomain = emailMatch?.groups?.domain || 'none';

// Date parsing with named groups
const dateRegex = /(?<year>\d{4})-(?<month>\d{2})-(?<day>\d{2})/;
const dateStr = '2024-06-15';
const dateMatch = dateStr.match(dateRegex);
const dateYear = dateMatch?.groups?.year || 'none';
const dateMonth = dateMatch?.groups?.month || 'none';
const dateDay = dateMatch?.groups?.day || 'none';

// URL parsing with multiple named groups
const urlRegex = /(?<protocol>https?):\/\/(?<host>[^/]+)\/(?<path>.*)/;
const url = 'https://api.example.com/v1/users';
const urlMatch = url.match(urlRegex);
const protocol = urlMatch?.groups?.protocol || 'none';
const host = urlMatch?.groups?.host || 'none';
const path = urlMatch?.groups?.path || 'none';

// Named groups with replace
const camelCaseStr = 'hello_world_test';
const pascalCase = camelCaseStr.replace(
  /(?<prefix>[^_]+)_(?<char>[a-z])/g,
  (_, prefix, char) => prefix + char.toUpperCase()
);

const snakeToKebab = 'hello_world_test';
const kebabCase = snakeToKebab.replace(/_/g, '-');

// Accessing groups in match
const timeRegex = /(?<hours>\d{2}):(?<minutes>\d{2}):(?<seconds>\d{2})/;
const timeStr = '14:30:45';
const timeMatch = timeStr.match(timeRegex);
const hours = timeMatch?.groups?.hours || 'none';
const minutes = timeMatch?.groups?.minutes || 'none';
const seconds = timeMatch?.groups?.seconds || 'none';

// Named groups with quantifiers
const ipRegex = /(?<octet>\d{1,3})\.(?<octet2>\d{1,3})\.(?<octet3>\d{1,3})\.(?<octet4>\d{1,3})/;
const ip = '192.168.1.100';
const ipMatch = ip.match(ipRegex);
const ip1 = ipMatch?.groups?.octet || 'none';
const ip2 = ipMatch?.groups?.octet2 || 'none';
const ip3 = ipMatch?.groups?.octet3 || 'none';
const ip4 = ipMatch?.groups?.octet4 || 'none';

// Named groups in array destructuring
const text = 'John:25';
const [name, age] = text.match(/(?<name>\w+):(?<age>\d+)/)?.groups ? ['none', 'none'] : [name, age];
const nameVal = text.match(/(?<name>\w+):(?<age>\d+)/)?.groups?.name || 'none';
const ageVal = text.match(/(?<name>\w+):(?<age>\d+)/)?.groups?.age || 'none';

export default function RegexpNamedGroups() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">RegExp Named Capture Groups</Text>
      <Text></Text>
      <Text>Email parsing:</Text>
      <Text>  email: {email}</Text>
      <Text>  user: {emailUser}, domain: {emailDomain}</Text>
      <Text></Text>
      <Text>Date parsing:</Text>
      <Text>  date: {dateStr}</Text>
      <Text>  year: {dateYear}, month: {dateMonth}, day: {dateDay}</Text>
      <Text></Text>
      <Text>URL parsing:</Text>
      <Text>  url: {url}</Text>
      <Text>  protocol: {protocol}, host: {host}</Text>
      <Text>  path: {path}</Text>
      <Text></Text>
      <Text>Replace with named groups:</Text>
      <Text>  camelCase: {camelCaseStr}</Text>
      <Text>  pascalCase: {pascalCase}</Text>
      <Text>  snakeToKebab: {kebabCase}</Text>
      <Text></Text>
      <Text>Time parsing:</Text>
      <Text>  time: {timeStr}</Text>
      <Text>  hours: {hours}, min: {minutes}, sec: {seconds}</Text>
      <Text></Text>
      <Text>IP address:</Text>
      <Text>  ip: {ip}</Text>
      <Text>  octets: {ip1}.{ip2}.{ip3}.{ip4}</Text>
      <Text></Text>
      <Text>Destructured values:</Text>
      <Text>  name: {nameVal}, age: {ageVal}</Text>
    </Box>
  );
}
