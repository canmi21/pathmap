use core::cell::UnsafeCell;

use crate::ring::{DistributiveLattice, Lattice, PartialDistributiveLattice, PartialQuantale};
use crate::trie_node::*;
use crate::zipper::*;
use num_traits::{zero, PrimInt};

/// A map type that uses byte slices `&[u8]` as keys
///
/// This type is implemented using some of the approaches explained in the
/// ["Bitwise trie with bitmap" Wikipedia article](https://en.wikipedia.org/wiki/Bitwise_trie_with_bitmap).
///
/// ```
/// # use pathmap::trie_map::BytesTrieMap;
/// let mut map = BytesTrieMap::<String>::new();
/// map.insert("one", "1".to_string());
/// map.insert("two", "2".to_string());
///
/// assert!(map.contains("one"));
/// assert_eq!(map.get("two"), Some(&"2".to_string()));
/// assert!(!map.contains("three"));
/// ```
pub struct BytesTrieMap<V> {
    root: UnsafeCell<TrieNodeODRc<V>>,
}

unsafe impl<V: Send + Sync> Send for BytesTrieMap<V> {}
unsafe impl<V: Send + Sync> Sync for BytesTrieMap<V> {}

impl<V: Clone + Send + Sync> Clone for BytesTrieMap<V> {
    fn clone(&self) -> Self {
        Self::new_with_root(self.root().clone())
    }
}

impl<V: Clone + Send + Sync> BytesTrieMap<V> {
    #[inline]
    pub(crate) fn root(&self) -> &TrieNodeODRc<V> {
        unsafe{ &*self.root.get() }
    }
    #[inline]
    pub(crate) fn root_mut(&mut self) -> &mut TrieNodeODRc<V> {
        self.root.get_mut()
    }

    /// Creates a new empty map
    pub fn new() -> Self {
        #[cfg(feature = "all_dense_nodes")]
        let root = TrieNodeODRc::new(crate::dense_byte_node::DenseByteNode::new());
        #[cfg(not(feature = "all_dense_nodes"))]
        let root = TrieNodeODRc::new(crate::line_list_node::LineListNode::new());
        Self::new_with_root(root)
    }

    /// Internal Method.  Creates a new BytesTrieMap with the supplied root node
    #[inline]
    pub(crate) fn new_with_root(root: TrieNodeODRc<V>) -> Self {
        Self { root: UnsafeCell::new(root) }
    }

    pub fn range<const BE : bool, R : PrimInt + std::ops::AddAssign + num_traits::ToBytes + std::fmt::Display>(start: R, stop: R, step: R, value: V) -> BytesTrieMap<V> {
        // #[cfg(feature = "all_dense_nodes")]
        // we can extremely efficiently generate ranges, but currently we're limited to range(0, BASE**j, k < BASE)
        // let root = crate::dense_byte_node::_so_range(step as u8, 4);
        // BytesTrieMap::<()>::new_with_root(root)
        //fallback

        //GOAT, this method is highly sub-optimal.  It should be possible to populate a range in log n time,
        // rather than linear time.  Adam has already written code for this, but it's specific to the DenseByteNode
        // and is commented out in that file
        let mut new_map = Self::new();
        let mut zipper = new_map.write_zipper();

        let mut i = start;
        let positive = step > zero();
        loop {
            if positive { if i >= stop { break } }
            else { if i <= step { break } }
            // println!("{}", i);
            if BE { zipper.descend_to(i.to_be_bytes()); }
            else { zipper.descend_to(i.to_le_bytes()); }
            zipper.set_value(value.clone());
            zipper.reset();

            i += step;
        }
        drop(zipper);

        new_map
    }

    /// Internal Method.  Removes and returns the root from a BytesTrieMap
    #[inline]
    pub(crate) fn into_root(self) -> Option<TrieNodeODRc<V>> {
        if !self.root().borrow().node_is_empty() {
            Some(self.root.into_inner())
        } else {
            None
        }
    }

    /// Creates a new [ReadZipper] starting at the root of a BytesTrieMap
    pub fn read_zipper(&self) -> ReadZipperUntracked<V> {
        #[cfg(debug_assertions)]
        {
            ReadZipperUntracked::new_with_node_and_path_internal(self.root().borrow().as_tagged(), &[], Some(0), None, None)
        }
        #[cfg(not(debug_assertions))]
        {
            ReadZipperUntracked::new_with_node_and_path_internal(self.root().borrow().as_tagged(), &[], Some(0), None)
        }
    }

    /// Creates a new [ReadZipper] with the specified path from the root of the map; This method is much more
    /// efficient than read_zipper_at_path, but means the resulting zipper is bound by the `'path` lifetime
    pub fn read_zipper_at_borrowed_path<'a, 'path>(&'a self, path: &'path[u8]) -> ReadZipperUntracked<'a, 'path, V> {
        #[cfg(debug_assertions)]
        {
            ReadZipperUntracked::new_with_node_and_path(self.root().borrow(), path.as_ref(), Some(path.len()), None)
        }
        #[cfg(not(debug_assertions))]
        {
            ReadZipperUntracked::new_with_node_and_path(self.root().borrow(), path.as_ref(), Some(path.len()))
        }
    }

    /// Creates a new [ReadZipper] with the specified path from the root of the map
    pub fn read_zipper_at_path<'a>(&'a self, path: &[u8]) -> ReadZipperUntracked<'a, 'static, V> {
        #[cfg(debug_assertions)]
        {
            ReadZipperUntracked::new_with_node_and_cloned_path(self.root().borrow(), path.as_ref(), Some(path.len()), None)
        }
        #[cfg(not(debug_assertions))]
        {
            ReadZipperUntracked::new_with_node_and_cloned_path(self.root().borrow(), path.as_ref(), Some(path.len()))
        }
    }

    /// Creates a new [WriteZipper] starting at the root of a BytesTrieMap
    pub fn write_zipper(&mut self) -> WriteZipperUntracked<V> {
        #[cfg(debug_assertions)]
        {
            WriteZipperUntracked::new_with_node_and_path_internal(self.root_mut(), &[], true, None)
        }
        #[cfg(not(debug_assertions))]
        {
            WriteZipperUntracked::new_with_node_and_path_internal(self.root_mut(), &[], true)
        }
    }

    /// Creates a new [WriteZipper] with the specified path from the root of the map
    pub fn write_zipper_at_path<'a, 'path>(&'a mut self, path: &'path[u8]) -> WriteZipperUntracked<'a, 'path, V> {
        #[cfg(debug_assertions)]
        {
            WriteZipperUntracked::new_with_node_and_path(self.root_mut(), path, None)
        }
        #[cfg(not(debug_assertions))]
        {
            WriteZipperUntracked::new_with_node_and_path(self.root_mut(), path)
        }
    }

    /// Creates a [ZipperHead] at the root of the map
    pub fn zipper_head(&mut self) -> ZipperHead<V> {
        let root = unsafe{ &mut *self.root.get() };
        ZipperHead::new(root)
    }

    /// Returns an iterator over all key-value pairs within the map
    ///
    /// NOTE: This is much less efficient than using the [read_zipper](Self::read_zipper) method
    pub fn iter<'a>(&'a self) -> impl Iterator<Item=(Vec<u8>, &'a V)> + 'a {
        self.read_zipper().into_iter()
    }

    /// Returns a [crate::old_cursor::PathMapCursor] to traverse all key-value pairs within the map. This
    /// is more efficient than using [iter](Self::iter), but is not compatible with the [Iterator] trait
    ///
    /// WARNING: This API will be deprecated in favor of the [read_zipper](Self::read_zipper) method
    pub fn cursor<'a>(&'a self) -> crate::old_cursor::PathMapCursor<'a, V> {
        crate::old_cursor::PathMapCursor::new(self)
    }

    /// Returns an [crate::old_cursor::AllDenseCursor], which behaves exactly like a [crate::old_cursor::PathMapCursor],
    /// but is only available with the `all_dense_nodes` feature.  This is mainly kept for benchmarking.
    pub fn all_dense_cursor<'a>(&'a self) -> crate::old_cursor::AllDenseCursor<'a, V> {
        crate::old_cursor::AllDenseCursor::new(self)
    }

    /// Returns `true` if the map contains a value at the specified key, otherwise returns `false`
    pub fn contains<K: AsRef<[u8]>>(&self, k: K) -> bool {
        let k = k.as_ref();

        //NOTE: Here is the old impl traversing without the zipper.  The zipper version appears to be
        // nearly the same perf.  All averages within 3% in both directions, and the zipper impl being
        // faster as often as the native (non-zipper) version
        // let (node, remaining_key) = traverse_to_leaf(self.root.borrow(), k);
        // node.node_contains_val(remaining_key)

        let zipper = self.read_zipper_at_borrowed_path(k);
        zipper.is_value()
    }

    /// Returns `true` if a path is contained within the map, or `false` otherwise
    pub fn contains_path<K: AsRef<[u8]>>(&self, k: K) -> bool {
        let k = k.as_ref();
        let zipper = self.read_zipper_at_borrowed_path(k);
        zipper.path_exists()
    }

    //GOAT, in light of PathMap's behavior holistically an a potential collision with WriteZipper::insert_prefix,
    // `insert` really should be called set_value
    /// Inserts `v` into the map at `k`.  Panics if `k` has a zero length
    ///
    /// Returns `Some(replaced_val)` if an existing value was replaced, otherwise returns `None` if
    /// the value was added to the map without replacing anything.
    pub fn insert<K: AsRef<[u8]>>(&mut self, k: K, v: V) -> Option<V> {
        let k = k.as_ref();

        //NOTE: Here is the old impl traversing without the zipper.  Kept here for benchmarking purposes
        // However, the zipper version is basically identical performance, within the margin of error
        // traverse_to_leaf_static_result(&mut self.root, k,
        // |node, remaining_key| node.node_set_val(remaining_key, v),
        // |_new_leaf_node, _remaining_key| None)

        let mut zipper = self.write_zipper_at_path(k);
        zipper.set_value(v)
    }

    //GOAT, make a separate `join_with` that is similar to `insert` except replaces V with a merged V rather
    // than replacing it

    /// Removes the value at `k` from the map and returns it, or returns None if there was no value at `k`
    pub fn remove<K: AsRef<[u8]>>(&mut self, k: K) -> Option<V> {
        let k = k.as_ref();
        //NOTE: we're descending the zipper rather than creating it at the path so it will be allowed to
        // prune the branches.  A WriteZipper can't move above its root, so it couldn't prune otherwise
        let mut zipper = self.write_zipper();
        zipper.descend_to(k);
        zipper.remove_value()
    }

    //GOAT-redo this with the WriteZipper::get_value_or_insert, although I may need an alternate function
    // that consumes the zipper in order to be allowed to return the correct lifetime
    //
    // pub fn update<K: AsRef<[u8]>, F: FnOnce()->V>(&mut self, k: K, default_f: F) -> &mut V {
    //     let k = k.as_ref();

    //     traverse_to_leaf_mut(&mut self.root, k,
    //     |node, remaining_key| node.node_update_val(remaining_key, Box::new(default_f)),
    //     |new_leaf_node, remaining_key| new_leaf_node.node_get_val_mut(remaining_key).unwrap())
    // }

    /// Returns `true` if the map is empty, otherwise returns `false`
    pub fn is_empty(&self) -> bool {
        self.root().borrow().node_is_empty()
    }

    /// Returns a reference to the value at the specified path
    pub fn get<K: AsRef<[u8]>>(&self, k: K) -> Option<&V> {
        let k = k.as_ref();

        //NOTE: Here is the old impl traversing without the zipper.  The zipper version appears to be
        // nearly the same perf.  All averages within 3% in both directions, and the zipper impl being
        // faster as often as the native (non-zipper) version
        // let (node, remaining_key) = traverse_to_leaf(self.root.borrow(), k);
        // node.node_get_val(remaining_key)

        let zipper = self.read_zipper_at_borrowed_path(k);
        zipper.get_value()
    }

    /// Returns the total number of values contained within the map
    ///
    /// WARNING: This is not a cheap method. It may have an order-N cost
    pub fn val_count(&self) -> usize {
        return val_count_below_root(self.root().borrow())
    }

    /// Returns a new `BytesTrieMap` where the paths in `self` are restricted by the paths leading to
    /// values in `other`
    pub fn restrict(&self, other: &Self) -> Self {
        match self.root().borrow().prestrict_dyn(other.root().borrow()) {
            Some(new_root) => Self::new_with_root(new_root),
            None => Self::new()
        }
    }
}

impl<V: Clone + Send + Sync, K: AsRef<[u8]>> FromIterator<(K, V)> for BytesTrieMap<V> {
    fn from_iter<I: IntoIterator<Item=(K, V)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (key, val) in iter {
            map.insert(key, val);
        }
        map
    }
}

impl<V: Clone + Lattice + Send + Sync> Lattice for BytesTrieMap<V> {
    fn join(&self, other: &Self) -> Self {
        Self::new_with_root(self.root().join(other.root()))
    }

    fn join_into(&mut self, other: Self) {
        if let Some(other_root) = other.into_root() {
            self.root_mut().join_into(other_root)
        }
    }

    fn meet(&self, other: &Self) -> Self {
        match self.root().meet(other.root()) {
            Some(new_root) => Self::new_with_root(new_root),
            None => Self::new()
        }
    }

    fn bottom() -> Self {
        BytesTrieMap::new()
    }
}

impl<V: Clone + Send + Sync + PartialDistributiveLattice> DistributiveLattice for BytesTrieMap<V> {
    fn subtract(&self, other: &Self) -> Self {
        Self::new_with_root(self.root().subtract(other.root()))
    }
}

impl<V: Clone + Send + Sync + PartialDistributiveLattice> PartialDistributiveLattice for BytesTrieMap<V> {
    fn psubtract(&self, other: &Self) -> Option<Self> {
        let s = self.root().subtract(other.root());
        if s.borrow().node_is_empty() { None }
        else { Some(Self::new_with_root(s)) }
    }
}

impl<V: Clone + Send + Sync> PartialQuantale for BytesTrieMap<V> {
    fn prestrict(&self, other: &Self) -> Option<Self> where Self: Sized {
        self.root().prestrict(other.root()).map(|r| Self::new_with_root(r) )
    }
}

impl<V: Clone + Send + Sync> Default for BytesTrieMap<V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::ring::Lattice;
    use crate::trie_map::*;

    #[test]
    fn map_test() {
        let mut map = BytesTrieMap::new();
        //NOW: map contains an empty ListNode

        map.insert("aaaaa", "aaaaa");
        assert_eq!(map.get("aaaaa").unwrap(), &"aaaaa");
        //NOW: map contains a ListNode with slot_0 filled by a value

        map.insert("bbbbb", "bbbbb");
        assert_eq!(map.get("bbbbb").unwrap(), &"bbbbb");
        //NOW: map contains a ListNode with slot_0 and slot_1 filled by values

        map.insert("ccccc", "ccccc");
        assert_eq!(map.get("aaaaa").unwrap(), &"aaaaa");
        assert_eq!(map.get("bbbbb").unwrap(), &"bbbbb");
        assert_eq!(map.get("ccccc").unwrap(), &"ccccc");
        //NOW: map contains a DenseByteNode, with 3 separate ListNodes, each containing one value

        map.insert("ddddd", "ddddd");
        assert_eq!(map.get("ddddd").unwrap(), &"ddddd");
        //NOW: map contains a DenseByteNode, with 4 separate ListNodes, each containing one value

        map.insert("abbbb", "abbbb");
        assert_eq!(map.get("abbbb").unwrap(), &"abbbb");
        //NOW: Dense("a"..) -> List("aaaa", "bbbb")

        map.insert("aaaab", "aaaab");
        assert_eq!(map.get("aaaaa").unwrap(), &"aaaaa");
        assert_eq!(map.get("bbbbb").unwrap(), &"bbbbb");
        assert_eq!(map.get("abbbb").unwrap(), &"abbbb");
        assert_eq!(map.get("aaaab").unwrap(), &"aaaab");
        //NOW: Dense("a"..) -> List("aaa", "bbbb") -> List("a", "b")

        map.insert("aaaac", "aaaac");
        assert_eq!(map.get("aaaaa").unwrap(), &"aaaaa");
        assert_eq!(map.get("aaaab").unwrap(), &"aaaab");
        assert_eq!(map.get("aaaac").unwrap(), &"aaaac");
        //NOW: Dense("a"..) -> List("aaa", "bbbb") -> Dense("a", "b", "c")

        map.insert("acaaa", "acaaa");
        assert_eq!(map.get("aaaaa").unwrap(), &"aaaaa");
        assert_eq!(map.get("aaaab").unwrap(), &"aaaab");
        assert_eq!(map.get("aaaac").unwrap(), &"aaaac");
        assert_eq!(map.get("abbbb").unwrap(), &"abbbb");
        assert_eq!(map.get("acaaa").unwrap(), &"acaaa");
        //NOW: Dense("a"..) -> Dense("a", "b", "c") a-> List("aa") -> Dense("a", "b", "c")
        //                                          b-> List("bbb")
        //                                          c-> List("aaa")
    }

    #[test]
    fn long_key_map_test() {
        let mut map = BytesTrieMap::new();

        map.insert("aaaaaaaaaa01234567890123456789", 30);
        assert_eq!(map.get("aaaaaaaaaa01234567890123456789").unwrap(), &30);

        map.insert("bbbbbbbbbb012345678901234567891", 31);
        assert_eq!(map.get("bbbbbbbbbb012345678901234567891").unwrap(), &31);

        map.insert("cccccccccc012345678901234567890123456789", 40);
        assert_eq!(map.get("cccccccccc012345678901234567890123456789").unwrap(), &40);

        map.insert("dddddddddd01234567890123456789012345678901234", 45);
        assert_eq!(map.get("dddddddddd01234567890123456789012345678901234").unwrap(), &45);

        map.insert("eeeeeeeeee01234567890123456789012345678901234567890123456789012345678901234567890123456789", 90);
        assert_eq!(map.get("eeeeeeeeee01234567890123456789012345678901234567890123456789012345678901234567890123456789").unwrap(), &90);
    }

    #[test]
    fn map_contains_path_test() {
        let mut btm = BytesTrieMap::new();
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });

        assert_eq!(btm.contains_path(b"can"), true);
        assert_eq!(btm.contains_path(b"cannon"), true);
        assert_eq!(btm.contains_path(b"cannonade"), false);
        assert_eq!(btm.contains_path(b""), true);
    }

    #[test]
    fn map_remove_test() {
        let mut map = BytesTrieMap::new();
        map.insert("aaaaa", "aaaaa");
        map.insert("bbbbb", "bbbbb");
        map.insert("ccccc", "ccccc");
        map.insert("ddddd", "ddddd");
        map.insert("abbbb", "abbbb");
        map.insert("aaaab", "aaaab");
        map.insert("aaaac", "aaaac");
        map.insert("acaaa", "acaaa");
        assert_eq!(map.val_count(), 8);

        assert_eq!(map.remove(b"aaaaa"), Some("aaaaa"));
        assert_eq!(map.val_count(), 7);
        assert_eq!(map.remove(b"acaaa"), Some("acaaa"));
        assert_eq!(map.val_count(), 6);
        assert_eq!(map.remove(b"cccccnot-a-real-key"), None);
        assert_eq!(map.val_count(), 6);
        assert_eq!(map.remove(b"aaaac"), Some("aaaac"));
        assert_eq!(map.val_count(), 5);
        assert_eq!(map.remove(b"aaaab"), Some("aaaab"));
        assert_eq!(map.val_count(), 4);
        assert_eq!(map.remove(b"abbbb"), Some("abbbb"));
        assert_eq!(map.val_count(), 3);
        assert_eq!(map.remove(b"ddddd"), Some("ddddd"));
        assert_eq!(map.val_count(), 2);
        assert_eq!(map.remove(b"ccccc"), Some("ccccc"));
        assert_eq!(map.val_count(), 1);
        assert_eq!(map.remove(b"bbbbb"), Some("bbbbb"));
        assert_eq!(map.val_count(), 0);
        assert!(map.is_empty());
    }

    #[test]
    fn map_update_test() {
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        let mut btm: BytesTrieMap<u64> = rs.into_iter().enumerate().map(|(i, k)| (k, i as u64)).collect();

        let mut zipper = btm.write_zipper_at_path(b"cannon");
        assert_eq!(zipper.get_value_or_insert(42), &2);
        drop(zipper);

        let mut zipper = btm.write_zipper_at_path(b"dagger");
        assert_eq!(zipper.get_value_or_insert(42), &42);
    }

    #[test]
    fn map_join_test() {
        let mut a = BytesTrieMap::<usize>::new();
        let mut b = BytesTrieMap::<usize>::new();
        let rs = ["Abbotsford", "Abbottabad", "Abcoude", "Abdul Hakim", "Abdulino", "Abdullahnagar", "Abdurahmoni Jomi", "Abejorral", "Abelardo Luz"];
        for (i, path) in rs.into_iter().enumerate() {
            if i % 2 == 0 {
                a.insert(path, i);
            } else {
                b.insert(path, i);
            }
        }

        let joined = a.join(&b);
        for (path, i) in joined.iter() {
            // println!("{} {}", std::str::from_utf8(&path).unwrap(), i);
            assert_eq!(rs[*i].as_bytes(), &path);
        }
        assert_eq!(joined.val_count(), rs.len());
    }

    #[test]
    fn cursor_test() {
        let table = ["A", "Bcdef", "Ghij", "Klmnopqrst"];
        let btm: BytesTrieMap<usize> = table.iter().enumerate().map(|(n, s)| (s, n)).collect();
        let mut cursor = btm.cursor();
        while let Some((k, v)) = cursor.next() {
            // println!("{}, {v}", std::str::from_utf8(k).unwrap());
            assert_eq!(k, table[*v].as_bytes());
        }
    }
}

//GOAT, decide what to do with root values on a zipper...  Options are:
//  1. document that root value access on a threaded WriteZipper will panic, and insert debug_asserts
//  2. make a special case to handle it.  This will require making an UnsafeCell around the value storage
//    location in the upstream node.  Since we don't want to pay for the UnsafeCell in every node, the
//    it's going to introduce a lot of complexity.  There are 2 sub-options to special-case it.
//   2a. Make a special type of parent with an UnsafeCell around its values, and ensure we always upgrade
//     the parent to that node type when creating the zipper
//   2b. Hold a shadow of the value within the WriteZipper, and re-integrate that value when the
//     WriteZipper is dropped.  Obviously I don't love the complexity of this approach either
//
// For code cleanliness reasons, I am strongly leaning towards 1.  Keep it illegal.
//
//UPDATE: I tried an UnsafeCells in every node.  The perf costs make this a non-starter.  (~30% penalty
// for almost every benchmark)  If we do need to handle this case, I think I have a preference for a special
// type of parent node that has an UnsafeCell around each child ptr and each value.
//
