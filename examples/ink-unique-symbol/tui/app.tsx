import React from 'react';
import { Box, Text } from 'ink';

// Branded type for UserId
type UserId = string & { readonly __brand: unique symbol };
const UserIdBrand = Symbol();
function createUserId(id: string): UserId {
  return id as UserId;
}

const userId = createUserId('user-123');

// Branded type for currency
type USD = number & { readonly __currency: unique symbol };
function usd(amount: number): USD {
  return amount as USD;
}

const price = usd(99.99);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>User ID: {userId}</Text>
      <Text>Price: ${price.toFixed(2)}</Text>
    </Box>
  );
}
