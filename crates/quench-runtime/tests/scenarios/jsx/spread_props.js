// JSX spread props test
// This file tests JSX element with spread props

// Test element with expression in props
const props = { className: "test" };
const element = <div {...props}>Spread Props</div>;
element;
