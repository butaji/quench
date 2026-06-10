// Table Demo — TuiBridge
// Renders tabular data with aligned columns
// Common pattern for displaying structured data

import { render, Box, Text, useState, useInput, useApp } from 'ink';

interface Column {
  key: string;
  title: string;
  width: number;
}

interface Row {
  [key: string]: string | number;
}

const COLUMNS: Column[] = [
  { key: 'name', title: 'Name', width: 12 },
  { key: 'status', title: 'Status', width: 10 },
  { key: 'cpu', title: 'CPU', width: 8 },
  { key: 'memory', title: 'Memory', width: 10 },
];

const INITIAL_ROWS: Row[] = [
  { name: 'nginx', status: 'running', cpu: 2.4, memory: 128 },
  { name: 'postgres', status: 'running', cpu: 8.1, memory: 512 },
  { name: 'redis', status: 'running', cpu: 1.2, memory: 64 },
  { name: 'app', status: 'stopped', cpu: 0, memory: 0 },
];

function padRight(str: string, width: number): string {
  const s = String(str);
  return s.padEnd(width, ' ');
}

function Table({ columns, rows }: { columns: Column[]; rows: Row[] }): JSX.Element {
  const totalWidth = columns.reduce((sum, col) => sum + col.width + 1, -1) + 2;

  // Header
  const headerCells = columns.map(col => (
    <Text key={col.key} bold width={col.width}>{padRight(col.title, col.width)}</Text>
  ));

  // Separator
  const separator = '─'.repeat(totalWidth);

  // Rows
  const dataRows = rows.map((row, rowIdx) => (
    <Box key={rowIdx}>
      {columns.map(col => {
        const value = row[col.key];
        const statusColor = col.key === 'status' 
          ? (value === 'running' ? 'green' : 'red')
          : undefined;
        return (
          <Text key={col.key} color={statusColor} width={col.width}>
            {padRight(String(value), col.width)}
          </Text>
        );
      })}
    </Box>
  ));

  return (
    <Box flexDirection="column">
      <Box>
        {headerCells}
      </Box>
      <Text dimColor>{separator}</Text>
      {dataRows}
    </Box>
  );
}

function TableDemo(): JSX.Element {
  const [rows, setRows] = useState(INITIAL_ROWS);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === 'r') {
      // Toggle nginx status
      setRows(current => current.map(row => {
        if (row.name === 'nginx') {
          return { ...row, status: row.status === 'running' ? 'stopped' : 'running' };
        }
        return row;
      }));
    }
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Table Demo</Text>
      <Text dimColor>[r] toggle nginx | [q] quit</Text>
      <Text> </Text>
      <Box borderStyle="single" padding={1}>
        <Table columns={COLUMNS} rows={rows} />
      </Box>
      <Text> </Text>
      <Text dimColor>Total rows: {rows.length}</Text>
    </Box>
  );
}

render(<TableDemo />);
