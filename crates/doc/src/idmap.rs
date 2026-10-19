//! A persistent map from object IDs to values: a 32-way trie of shared
//! nodes. Cloning is O(1); an edit copies the nodes on one path, O(log n),
//! and leaves every clone as it was.

use std::sync::Arc;

use crate::ObjectId;

const BITS: u32 = 5;
const WIDTH: usize = 1 << BITS;
const MASK: u64 = WIDTH as u64 - 1;

#[derive(Clone)]
#[allow(
    clippy::large_enum_variant,
    reason = "both variants hold 32 pointers when T is an Arc, as it is, and nodes live behind Arcs"
)]
enum Node<T> {
    Branch([Option<Arc<Node<T>>>; WIDTH]),
    Leaf([Option<T>; WIDTH]),
}

impl<T: Clone> Node<T> {
    fn empty(leaf: bool) -> Node<T> {
        if leaf {
            Node::Leaf(std::array::from_fn(|_| None))
        } else {
            Node::Branch(std::array::from_fn(|_| None))
        }
    }
}

/// The map. `levels` is the trie's height: keys below 32^levels fit.
#[derive(Clone)]
pub(crate) struct IdMap<T> {
    root: Option<Arc<Node<T>>>,
    levels: u32,
    len: usize,
}

impl<T: Clone> IdMap<T> {
    pub(crate) fn new() -> IdMap<T> {
        IdMap {
            root: None,
            levels: 1,
            len: 0,
        }
    }

    fn fits(&self, key: u64) -> bool {
        self.levels * BITS >= 64 || key >> (self.levels * BITS) == 0
    }

    pub(crate) fn get(&self, id: ObjectId) -> Option<&T> {
        if !self.fits(id.0) {
            return None;
        }
        let mut node = self.root.as_deref()?;
        let mut level = self.levels;
        loop {
            level -= 1;
            let slot = (id.0 >> (level * BITS) & MASK) as usize;
            match node {
                Node::Branch(children) => node = children[slot].as_deref()?,
                Node::Leaf(values) => return values[slot].as_ref(),
            }
        }
    }

    /// The value for `id`, copied out of any node a clone shares, so it can
    /// be changed in place.
    pub(crate) fn get_mut(&mut self, id: ObjectId) -> Option<&mut T> {
        self.get(id)?;
        let levels = self.levels;
        let mut node = Arc::make_mut(self.root.as_mut()?);
        let mut level = levels;
        loop {
            level -= 1;
            let slot = (id.0 >> (level * BITS) & MASK) as usize;
            match node {
                Node::Branch(children) => node = Arc::make_mut(children[slot].as_mut()?),
                Node::Leaf(values) => return values[slot].as_mut(),
            }
        }
    }

    /// Sets the value for `id`; returns the one it replaces.
    pub(crate) fn insert(&mut self, id: ObjectId, value: T) -> Option<T> {
        while !self.fits(id.0) {
            let old = self.root.take();
            let mut branch: [Option<Arc<Node<T>>>; WIDTH] = std::array::from_fn(|_| None);
            branch[0] = old;
            self.root = Some(Arc::new(Node::Branch(branch)));
            self.levels += 1;
        }
        let levels = self.levels;
        let mut node = Arc::make_mut(
            self.root
                .get_or_insert_with(|| Arc::new(Node::empty(levels == 1))),
        );
        let mut level = levels;
        loop {
            level -= 1;
            let slot = (id.0 >> (level * BITS) & MASK) as usize;
            match node {
                Node::Branch(children) => {
                    let child =
                        children[slot].get_or_insert_with(|| Arc::new(Node::empty(level == 1)));
                    node = Arc::make_mut(child);
                }
                Node::Leaf(values) => {
                    let old = values[slot].replace(value);
                    if old.is_none() {
                        self.len += 1;
                    }
                    return old;
                }
            }
        }
    }

    /// Removes the value for `id`; returns it.
    pub(crate) fn remove(&mut self, id: ObjectId) -> Option<T> {
        self.get(id)?;
        let levels = self.levels;
        let mut node = Arc::make_mut(self.root.as_mut()?);
        let mut level = levels;
        loop {
            level -= 1;
            let slot = (id.0 >> (level * BITS) & MASK) as usize;
            match node {
                Node::Branch(children) => node = Arc::make_mut(children[slot].as_mut()?),
                Node::Leaf(values) => {
                    let value = values[slot].take();
                    self.len -= 1;
                    return value;
                }
            }
        }
    }

    /// Every entry, in ID order.
    pub(crate) fn entries(&self) -> Vec<(ObjectId, &T)> {
        fn walk<'a, T>(node: &'a Node<T>, prefix: u64, out: &mut Vec<(ObjectId, &'a T)>) {
            match node {
                Node::Branch(children) => {
                    for (i, child) in children.iter().enumerate() {
                        if let Some(child) = child {
                            walk(child, prefix << BITS | i as u64, out);
                        }
                    }
                }
                Node::Leaf(values) => {
                    for (i, value) in values.iter().enumerate() {
                        if let Some(value) = value {
                            out.push((ObjectId(prefix << BITS | i as u64), value));
                        }
                    }
                }
            }
        }
        let mut out = Vec::with_capacity(self.len);
        if let Some(root) = &self.root {
            walk(root, 0, &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_gets_and_removes_across_levels() {
        let mut map = IdMap::new();
        for k in [0u64, 1, 31, 32, 1023, 1024, 40_000, u64::MAX] {
            assert_eq!(map.insert(ObjectId(k), k ^ 7), None);
        }
        assert_eq!(map.entries().len(), 8);
        for k in [0u64, 1, 31, 32, 1023, 1024, 40_000, u64::MAX] {
            assert_eq!(map.get(ObjectId(k)), Some(&(k ^ 7)));
        }
        assert_eq!(map.get(ObjectId(2)), None);
        assert_eq!(map.insert(ObjectId(32), 7), Some(32 ^ 7));
        assert_eq!(map.remove(ObjectId(32)), Some(7));
        assert_eq!(map.remove(ObjectId(32)), None);
        assert_eq!(map.entries().len(), 7);
        let keys: Vec<u64> = map.entries().iter().map(|(k, _)| k.0).collect();
        assert_eq!(keys, [0, 1, 31, 1023, 1024, 40_000, u64::MAX]);
    }

    #[test]
    fn clones_are_snapshots() {
        let mut map = IdMap::new();
        for k in 0..100u64 {
            map.insert(ObjectId(k), k);
        }
        let snapshot = map.clone();
        *map.get_mut(ObjectId(5)).unwrap() = 500;
        map.remove(ObjectId(6));
        map.insert(ObjectId(1000), 1);
        assert_eq!(snapshot.get(ObjectId(5)), Some(&5));
        assert_eq!(snapshot.get(ObjectId(6)), Some(&6));
        assert_eq!(snapshot.get(ObjectId(1000)), None);
        assert_eq!(snapshot.entries().len(), 100);
        assert_eq!(map.get(ObjectId(5)), Some(&500));
    }
}
