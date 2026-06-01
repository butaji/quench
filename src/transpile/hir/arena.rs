//! Arena allocator for HIR
//! 
//! Uses bumpalo for zero-GC, arena-first memory allocation.
//! All HIR objects are allocated from a single arena.
//! When file changes, drop the arena and recompile - no GC needed.

use bumpalo::Bump;

/// Arena index type - used instead of pointers for stable references
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArenaIndex(u32);

#[allow(dead_code)]
impl ArenaIndex {
    /// Create a new arena index
    pub fn new(idx: u32) -> Self {
        Self(idx)
    }

    /// Get the raw index value
    pub fn index(&self) -> u32 {
        self.0
    }

    /// Null index (not valid for any allocation)
    pub fn null() -> Self {
        Self(u32::MAX)
    }

    /// Check if this is the null index
    pub fn is_null(&self) -> bool {
        self.0 == u32::MAX
    }
}

/// Arena-allocated HIR storage
#[allow(dead_code)]
pub struct HirArena {
    /// The bump allocator
    bump: Bump,
    /// Next index to allocate
    next_index: u32,
}

#[allow(dead_code)]
impl HirArena {
    /// Create a new arena
    pub fn new() -> Self {
        Self {
            bump: Bump::new(),
            next_index: 0,
        }
    }
    
    /// Allocate a value and return its index
    pub fn alloc<T: ArenaAllocatable>(&mut self, value: T) -> T::Index {
        let idx = ArenaIndex(self.next_index);
        self.next_index += 1;
        value.alloc_on(self, idx)
    }
    
    /// Allocate a string and return its index
    pub fn alloc_string(&mut self, s: &str) -> ArenaIndex {
        let idx = ArenaIndex(self.next_index);
        self.next_index += 1;
        self.bump.alloc_str(s);
        idx
    }
    
    /// Allocate a Vec and return its index
    pub fn alloc_vec<T>(&mut self, vec: Vec<T>) -> ArenaIndex
    where
        T: ArenaAllocatable,
    {
        let idx = ArenaIndex(self.next_index);
        self.next_index += 1;
        // Allocate each element on the bump allocator and store them
        // The vec backing is heap-allocated but elements live on bump arena
        for item in vec {
            self.alloc(item);
        }
        idx
    }
    
    /// Get the total number of allocations
    pub fn allocation_count(&self) -> u32 {
        self.next_index
    }
    
    /// Reset the arena (drop all allocations)
    pub fn reset(&mut self) {
        self.bump = Bump::new();
        self.next_index = 0;
    }
}

impl Default for HirArena {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for types that can be arena-allocated
#[allow(dead_code)]
pub trait ArenaAllocatable: Sized {
    /// The index type for this type
    type Index;
    
    /// Allocate this value on the given arena with the given index
    fn alloc_on(self, arena: &mut HirArena, index: ArenaIndex) -> Self::Index;
}

/// Wrapper for arena-allocated strings
#[allow(dead_code)]
pub struct ArenaString {
    index: ArenaIndex,
}

#[allow(dead_code)]
impl ArenaString {
    pub fn index(&self) -> ArenaIndex {
        self.index
    }
}

impl ArenaAllocatable for String {
    type Index = ArenaIndex;
    
    fn alloc_on(self, arena: &mut HirArena, index: ArenaIndex) -> Self::Index {
        arena.alloc_string(&self);
        index
    }
}

/// Session-scoped arena management
#[allow(dead_code)]
pub struct ArenaSession {
    arena: HirArena,
}

#[allow(dead_code)]
impl ArenaSession {
    pub fn new() -> Self {
        Self {
            arena: HirArena::new(),
        }
    }

    /// Get mutable access to the arena for allocations
    pub fn arena(&mut self) -> &mut HirArena {
        &mut self.arena
    }

    /// Reset and start fresh (on file change)
    pub fn restart(&mut self) {
        self.arena.reset();
    }

    /// Get statistics about current arena usage
    pub fn stats(&self) -> ArenaStats {
        ArenaStats {
            allocations: self.arena.allocation_count(),
        }
    }
}

impl Default for ArenaSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Arena usage statistics
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ArenaStats {
    pub allocations: u32,
}
