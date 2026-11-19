//! Compiler symbol interning
//! The compiler uses a 64-bit symbol id (SymId) in place of strings in most places.
//! These are interned into a SymbolTable.
//! There's also the ability to do SymId::new(str), which gives you a compile-time constant
//! SymId, usable in match statements or similar, but this doesn't intern the symbol.
//! The SymId::new is really only meant for places where you compare against a user-provided symbol,
//! such as dispatch tables in the compiler where you might need compile-time constant IDs.
//! Interning checks for hash collision and panics.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

/// Identifier for a symbol that's small and cheap to copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SymId(u64);

/// 64-bit hash with FNV1a
pub const fn fnv64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;
    let mut hash = FNV_OFFSET_BASIS;
    let mut i = 0;

    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        i += 1;
    }

    hash
}

impl SymId {
    /// Create a new symbol ID for the given string, without interning it.
    pub const fn new(s: &str) -> Self {
        Self(fnv64(s.as_bytes()))
    }
}

/// Table for interning and looking up strings.
#[derive(Default)]
pub struct SymbolTable {
    id_to_sym: BTreeMap<SymId, String>,
}

impl SymbolTable {
    /// Store string in the table if it doesn't exist, return ID.
    pub fn intern(&mut self, s: &str) -> SymId {
        let id = SymId::new(s);
        match self.id_to_sym.entry(id) {
            Entry::Occupied(e) => {
                assert_eq!(e.get().as_str(), s);
            }
            Entry::Vacant(e) => {
                e.insert(s.to_owned());
            }
        }
        id
    }

    /// Get the string from a previously interned ID.
    pub fn lookup(&self, id: SymId) -> &str {
        self.id_to_sym.get(&id).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::fnv64;
    use crate::intern::SymId;
    use crate::intern::SymbolTable;

    #[test]
    fn fnv1a() {
        assert_eq!(fnv64(b"abc"), 0xe71fa2190541574b);
    }

    #[test]
    fn interning() {
        let mut table = SymbolTable::default();
        let id_foo = table.intern("foo");
        let id_bar = table.intern("bar");

        assert_eq!("foo", table.lookup(id_foo));
        assert_eq!("bar", table.lookup(id_bar));
        assert_eq!("bar", table.lookup(SymId::new("bar")));
    }
}
