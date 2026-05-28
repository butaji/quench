// ============================================================================
// JSX COMPONENTS
// ============================================================================

// Simple functional component
function Button({ children, onClick }: { children: string; onClick?: () => void }) {
  return <button onClick={onClick}>{children}</button>;
}

// Component with props
function Greeting({ name, age }: { name: string; age: number }) {
  return (
    <div>
      <h1>Hello, {name}!</h1>
      <p>You are {age} years old.</p>
    </div>
  );
}

// Component with conditional rendering
function Conditional({ show, children }: { show: boolean; children: any }) {
  if (!show) return null;
  return <div class="conditional">{children}</div>;
}

// Component with list rendering
function List({ items }: { items: string[] }) {
  return (
    <ul>
      {items.map((item, index) => (
        <li key={index}>{item}</li>
      ))}
    </ul>
  );
}

// Nested components
function Card({ title, content }: { title: string; content: string }) {
  return (
    <div class="card">
      <CardHeader title={title} />
      <CardBody text={content} />
    </div>
  );
}

function CardHeader({ title }: { title: string }) {
  return <div class="card-header">{title}</div>;
}

function CardBody({ text }: { text: string }) {
  return <div class="card-body">{text}</div>;
}

// Fragment usage
function FragmentExample({ a, b, c }: { a: string; b: string; c: string }) {
  return (
    <>
      <div>{a}</div>
      <div>{b}</div>
      <div>{c}</div>
    </>
  );
}

// Spread props
function SpreadProps(props: { className: string; id?: string; children: any }) {
  return <div {...props}>{props.children}</div>;
}

// Boolean attributes
function BooleanAttr({ disabled, readonly }: { disabled: boolean; readonly: boolean }) {
  return (
    <input 
      type="text" 
      disabled={disabled} 
      readOnly={readonly} 
    />
  );
}

// Component with children as props
function Container({ children }: { children: any }) {
  return <div class="container">{children}</div>;
}
