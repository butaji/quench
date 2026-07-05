// Spread in arrays
const arr1 = [1, 2];
const arr2 = [3, 4];
const combined = [...arr1, ...arr2];
combined.length === 4 && combined[0] === 1 && combined[3] === 4;
