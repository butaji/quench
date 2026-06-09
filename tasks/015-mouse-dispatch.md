# Task 015: Mouse Dispatch

## Goal
Implement mouse event hit-testing and dispatch to deepest matching node.

## Acceptance Criteria
- [x] `dispatch_mouse(mouse)` converts crossterm MouseEvent to JS object.
- [x] Hit-test against Yoga-computed layout rects (`left`, `top`, `width`, `height`).
- [x] Dispatches to deepest node with registered input callback.
- [x] Unit test: build tree with known rects, simulate click at (x,y), verify correct node receives event.

## Implementation

### Mouse Event Dispatch (main.rs)

```rust
Some(Ok(Event::Mouse(mouse))) => {
    use crossterm::event::MouseEventKind;
    let kind_str = match mouse.kind {
        MouseEventKind::Down(_) => "press",
        MouseEventKind::Up(_) => "release",
        MouseEventKind::Drag(_) | MouseEventKind::Moved => "hold",
        MouseEventKind::ScrollUp => "wheelUp",
        MouseEventKind::ScrollDown => "wheelDown",
        _ => "unknown",
    };
    // ... dispatch to JS via __tb_dispatch_mouse
}
```

### Hit-Testing (runtime.js)

```javascript
// Find deepest node at position
function findDeepestNodeAt(x, y, rootId) {
  const candidates = [];
  function traverse(nodeId, depth) {
    if (isPointInNode(nodeId, x, y)) {
      candidates.push({ nodeId, depth });
    }
    const children = globalThis.__ink_get_node_children(nodeId) || [];
    for (const childId of children) {
      traverse(childId, depth + 1);
    }
  }
  traverse(rootId, 0);
  candidates.sort((a, b) => b.depth - a.depth);
  return candidates[0]?.nodeId;
}
```

### Dispatch to Deepest Handler

```javascript
globalThis.__tb_dispatch_mouse = function(event) {
  const { column, row } = event;
  const handler = findMouseHandlerAt(column, row);
  if (handler) {
    handler.handler(event);
  }
};
```

### Node Parent Tracking (bridge.rs)

Added `__ink_get_node_parent` for ancestor traversal during hit-testing.

## Dependencies
- Task 008, Task 013

## SPEC Reference
§7.3 Hit Testing (Mouse)
