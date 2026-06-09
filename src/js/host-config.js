// Host config for React Reconciler targeting TuiBridge FFI
// This intercepts React's reconciler calls and redirects them to Rust

const hostConfig = {
  // ========================================================================
  // Creating Instances
  // ========================================================================
  
  createInstance(type, props) {
    const tag = type;
    const propsJson = JSON.stringify(props || {});
    const id = globalThis.__ink_create_node(tag, propsJson);
    return { id, type };
  },
  
  createTextInstance(text) {
    const id = globalThis.__ink_create_text_node(String(text));
    return { id, type: 'text', text: String(text) };
  },
  
  // ========================================================================
  // Tree Mutation
  // ========================================================================
  
  appendChild(parent, child) {
    globalThis.__ink_append_child(String(parent.id), String(child.id));
  },
  
  removeChild(parent, child) {
    globalThis.__ink_remove_child(String(parent.id), String(child.id));
  },
  
  insertBefore(parent, child, beforeChild) {
    globalThis.__ink_insert_before(
      String(parent.id),
      String(child.id),
      String(beforeChild.id)
    );
  },
  
  // ========================================================================
  // Updates
  // ========================================================================
  
  commitUpdate(instance, oldProps, newProps) {
    const propsJson = JSON.stringify(newProps || {});
    globalThis.__ink_commit_update(String(instance.id), propsJson);
  },
  
  commitTextUpdate(textInstance, oldText, newText) {
    globalThis.__ink_set_text(String(textInstance.id), String(newText));
  },
  
  // ========================================================================
  // Root & Reconciliation
  // ========================================================================
  
  getRootHostContext() {
    return null;
  },
  
  getChildHostContext() {
    return null;
  },
  
  prepareForCommit() {
    return null;
  },
  
  resetAfterCommit() {},
  
  // ========================================================================
  // Text & Element Measurement
  // ========================================================================
  
  getPublicInstance(instance) {
    return { id: instance.id };
  },
  
  // ========================================================================
  // Hydration (not supported)
  // ========================================================================
  
  prepareUpdate() {
    return true;
  },
  
  // ========================================================================
  // Scheduling (minimal implementation)
  // ========================================================================
  
  shouldSetTextContent() {
    return false;
  },
  
  clearContainer() {},
  
  noop: () => {},
};

if (typeof module !== 'undefined' && module.exports) {
  module.exports = { hostConfig };
}
