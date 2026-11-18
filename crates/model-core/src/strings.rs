use std::collections::HashMap;

/// Index of a string in a [`StrTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct StrId(pub(crate) u32);

/// All strings of a model in one buffer: one allocation, 8 bytes of
/// overhead per distinct string.
#[derive(Debug, Clone)]
pub(crate) struct StrTable {
    bytes: String,
    /// `offsets[i]..offsets[i + 1]` is string `i`.
    offsets: Vec<usize>,
}

impl Default for StrTable {
    fn default() -> Self {
        StrTable {
            bytes: String::new(),
            offsets: vec![0],
        }
    }
}

impl StrTable {
    /// `id` must come from this table.
    pub(crate) fn get(&self, id: StrId) -> &str {
        let i = id.0 as usize;
        &self.bytes[self.offsets[i]..self.offsets[i + 1]]
    }

    fn push(&mut self, s: &str) -> StrId {
        let id = StrId((self.offsets.len() - 1) as u32);
        self.bytes.push_str(s);
        self.offsets.push(self.bytes.len());
        id
    }

    pub(crate) fn len(&self) -> usize {
        self.offsets.len() - 1
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.bytes.capacity() + self.offsets.capacity() * size_of::<usize>()
    }

    pub(crate) fn shrink_to_fit(&mut self) {
        self.bytes.shrink_to_fit();
        self.offsets.shrink_to_fit();
    }
}

/// Deduplicating [`StrTable`] writer, used while building.
#[derive(Debug, Default)]
pub(crate) struct Interner {
    ids: HashMap<Box<str>, StrId>,
    table: StrTable,
}

impl Interner {
    pub(crate) fn intern(&mut self, s: &str) -> StrId {
        if let Some(&id) = self.ids.get(s) {
            return id;
        }
        let id = self.table.push(s);
        self.ids.insert(s.into(), id);
        id
    }

    pub(crate) fn get(&self, s: &str) -> Option<StrId> {
        self.ids.get(s).copied()
    }

    pub(crate) fn str(&self, id: StrId) -> &str {
        self.table.get(id)
    }

    pub(crate) fn into_table(self) -> StrTable {
        let mut t = self.table;
        t.shrink_to_fit();
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interns_once() {
        let mut i = Interner::default();
        let a = i.intern("a");
        let empty = i.intern("");
        assert_eq!(i.intern("a"), a);
        assert_eq!(i.get("b"), None);
        let t = i.into_table();
        assert_eq!(t.len(), 2);
        assert_eq!(t.get(a), "a");
        assert_eq!(t.get(empty), "");
    }
}
