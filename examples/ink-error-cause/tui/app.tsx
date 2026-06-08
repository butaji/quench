// ink-error-cause example — demonstrates Error with cause.
//
// ES2022 introduced the `cause` property on Error objects, allowing
// errors to chain to their underlying cause.
//
// This example shows how to create errors with causes and how to
// access the cause property.

import React from 'react';
import { Box, Text } from 'ink';

// Create an error with a cause
const underlyingError = new Error('Connection refused');
underlyingError.name = 'NetworkError';

const errorWithCause = new Error('Failed to fetch data', { cause: underlyingError });
errorWithCause.name = 'FetchError';

// Error without cause
const simpleError = new Error('Simple error');

// Aggregate error with cause
const aggregateCause = new Error('Primary error');
aggregateCause.name = 'PrimaryError';

const aggregateError = new AggregateError(
  [new Error('Item 1 failed'), new Error('Item 2 failed')],
  'Multiple items failed',
  { cause: aggregateCause }
);

// Error subclasses
class ValidationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'ValidationError';
  }
}

const validationError = new ValidationError('Invalid input');
validationError.cause = new Error('Input was empty');

export default function ErrorCauseDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Error.cause Demo</Text>
      <Text dimColor>ES2022 error chaining</Text>
      <Text></Text>
      <Text>--- Error with cause ---</Text>
      <Text>message: {errorWithCause.message}</Text>
      <Text>name: {errorWithCause.name}</Text>
      <Text>has cause: {errorWithCause.cause ? 'true' : 'false'}</Text>
      <Text>cause message: {errorWithCause.cause?.message}</Text>
      <Text></Text>
      <Text>--- Error without cause ---</Text>
      <Text>message: {simpleError.message}</Text>
      <Text>has cause: {simpleError.cause ? 'true' : 'false'}</Text>
      <Text></Text>
      <Text>--- AggregateError with cause ---</Text>
      <Text>message: {aggregateError.message}</Text>
      <Text>name: {aggregateError.name}</Text>
      <Text>errors length: {aggregateError.errors.length}</Text>
      <Text>has cause: {aggregateError.cause ? 'true' : 'false'}</Text>
      <Text></Text>
      <Text>--- Custom error class ---</Text>
      <Text>message: {validationError.message}</Text>
      <Text>name: {validationError.name}</Text>
      <Text>has cause: {validationError.cause ? 'true' : 'false'}</Text>
      <Text>cause name: {validationError.cause?.name}</Text>
    </Box>
  );
}
