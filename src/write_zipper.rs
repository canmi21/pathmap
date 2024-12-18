use mutcursor::MutCursorRootedVec;

use crate::trie_node::*;
use crate::trie_map::BytesTrieMap;
use crate::empty_node::EmptyNode;
use crate::zipper::*;
use crate::zipper::zipper_priv::*;
use crate::zipper_tracking::*;
use crate::ring::{AlgebraicResult, DistributiveLattice, Lattice, COUNTER_IDENT, SELF_IDENT};

/// Implemented on [Zipper] types that allow modification of the trie
pub trait WriteZipper<V>: Zipper + WriteZipperPriv<V> {
    /// A [ZipperHead] that can be created from this zipper
    type ZipperHead<'z> where Self: 'z;

    /// Returns a refernce to the value at the zipper's focus, or `None` if there is no value
    ///
    /// NOTE: This method differs from [ReadOnlyZipper::get_value] in the lifetime of the return
    /// value.  A `WriteZipper` must return a value with a shorter lifetime because the zipper may
    /// modify or remove the value.
    fn get_value(&self) -> Option<&V>;

    /// Returns a mutable reference to a value at the zipper's focus, or None if no value exists
    fn get_value_mut(&mut self) -> Option<&mut V>;

    /// Returns a mutable reference to the value at the zipper's focus, inserting `default` if no
    /// value exists
    fn get_value_or_insert(&mut self, default: V) -> &mut V;

    /// Returns a mutable reference to the value at the zipper's focus, inserting the result of `func`
    /// if no value exists
    fn get_value_or_insert_with<F>(&mut self, func: F) -> &mut V where F: FnOnce() -> V;

    /// Sets the value at the zipper's focus
    ///
    /// Returns `Some(replaced_val)` if an existing value was replaced, otherwise returns `None` if
    /// the value was added without replacing anything.
    ///
    /// Panics if the zipper's focus is unable to hold a value
    fn set_value(&mut self, val: V) -> Option<V>;

    /// Removes the value at the zipper's focus.  Does not affect any onward branches.  Returns `Some(val)`
    /// with the value that was removed, otherwise returns `None`
    ///
    /// WARNING: This method may cause the trie to be pruned above the zipper's focus, and may result in
    /// [Zipper::path_exists] returning `false`, where it previously returned `true`
    fn remove_value(&mut self) -> Option<V>;

    /// Creates a [ZipperHead] at the zipper's current focus
    fn zipper_head<'z>(&'z mut self) -> Self::ZipperHead<'z>;

    /// Replaces the trie below the zipper's focus with the subtrie downstream from the focus of `read_zipper`
    ///
    /// If there is a value at the zipper's focus, it will not be affected.
    ///
    /// WARNING: If the `read_zipper` is not on an existing path (according to [Zipper::path_exists]) then the
    /// effect will be the same as [Self::remove_branches] causing the trie to be pruned below the graft location.
    /// Since dangling paths aren't allowed, This method may cause the trie to be pruned above the zipper's focus,
    /// and may lead to `path_exists` returning `false`, where it previously returned `true`
    fn graft<Z: Zipper<V=V>>(&mut self, read_zipper: &Z);

    /// Replaces the trie below the zipper's focus with the contents of a [BytesTrieMap], consuming the map
    ///
    /// If there is a value at the zipper's focus, it will not be affected.
    ///
    /// WARNING: If the `map` is empty then the effect will be the same as [Self::remove_branches] causing the
    /// trie to be pruned below the graft location.  Since dangling paths aren't allowed, This method may cause
    /// the trie to be pruned above the zipper's focus, and may lead to [Zipper::path_exists] returning `false`,
    /// where it previously returned `true`
    fn graft_map(&mut self, map: BytesTrieMap<V>);

    /// Joins (union of) the subtrie below the zipper's focus with the subtrie downstream from the focus of
    /// `read_zipper`
    ///
    /// Returns `true` if the join was sucessful, or `false` if `read_zipper` was at a nonexistent path.
    ///
    /// If the &self zipper is at a path that does not exist, this method behaves like graft.
    fn join<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool where V: Lattice;

    /// Joins (union of) the trie below the zipper's focus with the contents of a [BytesTrieMap],
    /// consuming the map
    ///
    /// Returns `true` if the join was sucessful, or `false` if `map` was empty.
    fn join_map(&mut self, map: BytesTrieMap<V>) -> bool where V: Lattice;

    /// Joins the subtrie below the focus of `src_zipper` with the subtrie below the focus of `self`,
    /// consuming `src_zipper`'s subtrie
    ///
    /// Returns `true` if the join was sucessful, or `false` if `src_zipper` was at a nonexistent path.
    fn join_into<Z: Zipper<V=V> + WriteZipper<V>>(&mut self, src_zipper: &mut Z) -> bool where V: Lattice;

    /// Collapses all the paths below the zipper's focus by removing the leading `byte_cnt` bytes from
    /// each path and joining together all of the downstream sub-paths
    ///
    /// Returns `true` if the focus has at least one downstream continuation, otherwise returns `false`.
    ///
    /// NOTE: This method may prune the path upstream of the focus of the operation resulted in removing all
    /// downstream paths.  This means that [Zipper::path_exists] may return `false` after this operation.
    fn drop_head(&mut self, byte_cnt: usize) -> bool where V: Lattice;

// GOAT, should we rename `drop_head` to `drop_prefix`?  Or rename `insert_prefix` to `insert_head`?
// GOAT QUESTION: Do we want to change the behavior to move the value as well?  Or do we want a variant
//  of this method that moves the value?  The main guiding idea behind not shifting the value was the desire
//  to preserve the property of being the inverse of drop_head.
    /// Inserts `prefix` in front of every downstream path at the focus
    ///
    /// This method does not affect a value at the focus, nor does it move the zipper's focus.
    ///
    /// NOTE: This is the inverse of [Self::drop_head], although it cannot perfectly undo `drop_head` because
    /// `drop_head` loses information about the prior nested structure.  However, `drop_head` will undo this
    /// operation.
    fn insert_prefix<K: AsRef<[u8]>>(&mut self, prefix: K) -> bool;

    /// Deleted the `n` bytes from the path above the zipper's focus, including any subtries that descend
    /// from the deleted branches
    ///
    /// Returns `true` if n upstream bytes were removed from the path, otherwise returns `false`.
    //
    // GOAT: TODO, make a diagram illustrating the behavior
    fn remove_prefix(&mut self, n: usize) -> bool;

    /// Meets (retains the intersection of) the subtrie below the zipper's focus with the subtrie downstream
    /// from the focus of `read_zipper`
    ///
    /// Returns `true` if the meet was sucessful, or `false` if either `self` of `read_zipper` is at a
    /// nonexistent path.
    fn meet<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool where V: Lattice;

    /// Experiment.  GOAT, document this
    fn meet_2<'z, ZA: Zipper<V=V>, ZB: Zipper<V=V>>(&mut self, rz_a: &ZA, rz_b: &ZB) -> bool where V: Lattice;

    /// Subtracts the subtrie downstream of the focus of `read_zipper` from the subtrie below the zipper's
    /// focus
    ///
    /// Returns `true` if the subtraction was sucessful, or `false` if either `self` of `read_zipper` is at a
    /// nonexistent path.
    ///
    /// GOAT QUESTION: Should we return false if the subtraction had no effect?
    /// E.g. nothing was removed, even though something possibly could have been with different args?
    fn subtract<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool where V: DistributiveLattice;

    /// Restricts paths in the subtrie downstream of the `self` focus to paths prefixed by a path to a value in
    /// `read_zipper`
    ///
    /// Returns `true` if the restriction was sucessful, or `false` if either `self` or `read_zipper` is at a
    /// nonexistent path.
    ///
    /// GOAT QUESTION: Should we return false if the restriction had no effect and the result was identity?
    /// E.g. nothing was removed, even though something possibly could have been with different args?
    fn restrict<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool;

    /// Populates the "stem" paths in `self` with the corresponding subtries in `read_zipper`
    ///
    /// NOTE: Any stem path without a corresponding path in `read_zipper` will be removed from `self`.
    ///
    /// GOAT, I feel like `restricting` might not be a very evocative name here.  The way I think of this
    /// operation is as a bunch of "stems" in the WriteZipper, that get their downstream contents populated
    /// by the corresponding paths in the ReadZipper.  Ideas for names are: "blossom", "fill_in", "expound",
    /// "populate", etc.  I avoided "bloom" and "expand" because those both have other connotations.
    fn restricting<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool;

    /// Removes all branches below the zipper's focus.  Does not affect the value if there is one.  Returns `true`
    /// if a branch was removed, otherwise returns `false`
    ///
    /// WARNING: This method may cause the trie to be pruned above the zipper's focus, and may result in
    /// [Zipper::path_exists] returning `false`, where it previously returned `true`
    fn remove_branches(&mut self) -> bool;

    /// Creates a new [BytesTrieMap] from the zipper's focus, removing all downstream branches from the zipper
    fn take_map(&mut self) -> Option<BytesTrieMap<V>>;

    /// Uses a 256-bit mask to remove multiple branches below the zipper's focus
    ///
    /// Key bytes for which the corresponding `mask` bit is `0` will be removed.
    ///
    /// WARNING: This method may cause the trie to be pruned above the zipper's focus, and may result in
    /// [Zipper::path_exists] returning `false`, where it previously returned `true`
    fn remove_unmasked_branches(&mut self, mask: [u64; 4]);
}

pub(crate) mod write_zipper_priv {
    use crate::trie_node::*;

    pub trait WriteZipperPriv<V> {
        fn take_focus(&mut self) -> Option<TrieNodeODRc<V>>;
    }
}
use write_zipper_priv::*;

// ***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---
// WriteZipperTracked
// ***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---

/// A [WriteZipper] for editing and adding paths and values in the trie
pub struct WriteZipperTracked<'a, 'path, V> {
    z: WriteZipperCore<'a, 'path, V>,
    _tracker: Option<ZipperTracker<TrackingWrite>>,
}

//The Drop impl ensures the tracker gets dropped at the right time
impl<V> Drop for WriteZipperTracked<'_, '_, V> {
    fn drop(&mut self) { }
}

impl<V: Clone + Send + Sync> Zipper for WriteZipperTracked<'_, '_, V> {
    type ReadZipperT<'a> = ReadZipperUntracked<'a, 'a, V> where Self: 'a;

    fn at_root(&self) -> bool { self.z.at_root() }
    fn reset(&mut self) { self.z.reset() }
    fn path(&self) -> &[u8] { self.z.path() }
    fn path_exists(&self) -> bool { self.z.path_exists() }
    fn is_value(&self) -> bool { self.z.is_value() }
    fn val_count(&self) -> usize { self.z.val_count() }
    fn child_count(&self) -> usize { self.z.child_count() }
    fn child_mask(&self) -> [u64; 4] { self.z.child_mask() }
    fn descend_to<K: AsRef<[u8]>>(&mut self, k: K) -> bool { self.z.descend_to(k) }
    fn descend_to_byte(&mut self, k: u8) -> bool { self.z.descend_to_byte(k) }
    fn descend_indexed_branch(&mut self, child_idx: usize) -> bool { self.z.descend_indexed_branch(child_idx) }
    fn descend_first_byte(&mut self) -> bool { self.z.descend_first_byte() }
    fn descend_until(&mut self) -> bool { self.z.descend_until() }
    fn to_sibling(&mut self, next: bool) -> bool { self.z.to_sibling(next) }
    fn to_next_sibling_byte(&mut self) -> bool { self.z.to_next_sibling_byte() }
    fn to_prev_sibling_byte(&mut self) -> bool { self.z.to_prev_sibling_byte() }
    fn ascend(&mut self, steps: usize) -> bool { self.z.ascend(steps) }
    fn ascend_byte(&mut self) -> bool { self.z.ascend_byte() }
    fn ascend_until(&mut self) -> bool { self.z.ascend_until() }
    fn fork_read_zipper<'a>(&'a self) -> Self::ReadZipperT<'a> {
        let new_root_val = self.get_value();
        let rz_core = ReadZipperCore::new_with_node_and_path_internal(self.z.focus_stack.top().unwrap().as_tagged(), &self.z.key.node_key(), None, new_root_val);
        Self::ReadZipperT::new_forked_with_inner_zipper(rz_core)
    }
    fn make_map(&self) -> Option<BytesTrieMap<Self::V>> { self.z.make_map() }
}

impl<'a, 'path, V : Clone> zipper_priv::ZipperPriv for WriteZipperTracked<'a, 'path, V> {
    type V = V;
    fn get_focus(&self) -> AbstractNodeRef<Self::V> { self.z.get_focus() }
    fn try_borrow_focus(&self) -> Option<&dyn TrieNode<Self::V>> { self.z.try_borrow_focus() }
}

impl<'a, 'path, V: Clone + Send + Sync> WriteZipperTracked<'a, 'path, V> {
    //GOAT, this method currently isn't called
    // /// Creates a new zipper, with a path relative to a node
    // pub(crate) fn new_with_node_and_path(root_node: &'a mut TrieNodeODRc<V>, path: &'k [u8], tracker: ZipperTracker) -> Self {
    //     let core = WriteZipperCore::<'a, 'k, V>::new_with_node_and_path(root_node, path);
    //     Self { z: core, tracker, }
    // }
    /// Creates a new zipper, with a path relative to a node, assuming the path is fully-contained within
    /// the node
    ///
    /// NOTE: This method currently doesn't descend subnodes.  Use [Self::new_with_node_and_path] if you can't
    /// guarantee the path is within the supplied node.
    pub(crate) fn new_with_node_and_path_internal(root_node: &'a mut TrieNodeODRc<V>, root_val: Option<&'a mut Option<V>>, path: &'path [u8], tracker: ZipperTracker<TrackingWrite>) -> Self {
        let core = WriteZipperCore::<'a, 'path, V>::new_with_node_and_path_internal(root_node, root_val, path);
        Self { z: core, _tracker: Some(tracker), }
    }

    /// Consumes the `WriteZipperTracked`, and returns a [ReadZipperTracked] in its place
    ///
    /// The returned read zipper will have the same root and focus as the the consumed write zipper.
    pub fn into_read_zipper(mut self) -> ReadZipperTracked<'a, 'static, V> {
        let tracker = self._tracker.take().unwrap().into_reader();
        let root_node = self.z.focus_stack.take_root().unwrap().borrow();
        let root_path = &self.z.key.prefix_buf[..self.z.key.root_key.len()];
        let descended_path = &self.z.key.prefix_buf[self.z.key.root_key.len()..];
        let root_val = core::mem::take(&mut self.z.root_val);
        let root_val = root_val.and_then(|root_val| root_val.as_ref());

        let mut new_zipper = ReadZipperTracked::new_with_node_and_cloned_path(root_node, root_path, None, root_val, tracker);
        new_zipper.descend_to(descended_path);
        new_zipper
    }
}

impl<'a, V: Clone + Send + Sync> WriteZipper<V> for WriteZipperTracked<'a, '_, V> {
    type ZipperHead<'z> = ZipperHead<'z, 'a, V> where Self: 'z;
    fn get_value(&self) -> Option<&V> { self.z.get_value() }
    fn get_value_mut(&mut self) -> Option<&mut V> { self.z.get_value_mut() }
    fn get_value_or_insert(&mut self, default: V) -> &mut V { self.z.get_value_or_insert(default) }
    fn get_value_or_insert_with<F>(&mut self, func: F) -> &mut V where F: FnOnce() -> V { self.z.get_value_or_insert_with(func) }
    fn set_value(&mut self, val: V) -> Option<V> { self.z.set_value(val) }
    fn remove_value(&mut self) -> Option<V> { self.z.remove_value() }
    fn zipper_head<'z>(&'z mut self) -> Self::ZipperHead<'z> { self.z.zipper_head() }
    fn graft<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) { self.z.graft(read_zipper) }
    fn graft_map(&mut self, map: BytesTrieMap<V>) { self.z.graft_map(map) }
    fn join<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool where V: Lattice { self.z.join(read_zipper) }
    fn join_map(&mut self, map: BytesTrieMap<V>) -> bool where V: Lattice { self.z.join_map(map) }
    fn join_into<Z: Zipper<V=V> + WriteZipper<V>>(&mut self, src_zipper: &mut Z) -> bool where V: Lattice { self.z.join_into(src_zipper) }
    fn drop_head(&mut self, byte_cnt: usize) -> bool where V: Lattice { self.z.drop_head(byte_cnt) }
    fn insert_prefix<K: AsRef<[u8]>>(&mut self, prefix: K) -> bool { self.z.insert_prefix(prefix) }
    fn remove_prefix(&mut self, n: usize) -> bool { self.z.remove_prefix(n) }
    fn meet<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool where V: Lattice { self.z.meet(read_zipper) }
    fn meet_2<ZA: Zipper<V=V>, ZB: Zipper<V=V>>(&mut self, rz_a: &ZA, rz_b: &ZB) -> bool where V: Lattice { self.z.meet_2(rz_a, rz_b) }
    fn subtract<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool where V: DistributiveLattice { self.z.subtract(read_zipper) }
    fn restrict<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool { self.z.restrict(read_zipper) }
    fn restricting<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool { self.z.restricting(read_zipper) }
    fn remove_branches(&mut self) -> bool { self.z.remove_branches() }
    fn take_map(&mut self) -> Option<BytesTrieMap<V>> { self.z.take_map() }
    fn remove_unmasked_branches(&mut self, mask: [u64; 4]) { self.z.remove_unmasked_branches(mask) }
}

impl<V: Clone + Send + Sync> WriteZipperPriv<V> for WriteZipperTracked<'_, '_, V> {
    fn take_focus(&mut self) -> Option<TrieNodeODRc<V>> { self.z.take_focus() }
}

// ***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---
// WriteZipperUntracked
// ***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---

/// A [Zipper] for editing and adding paths and values in the trie, used when it is possible to statically
/// guarantee non-interference between zippers
pub struct WriteZipperUntracked<'a, 'k, V> {
    z: WriteZipperCore<'a, 'k, V>,
    /// We will still track the zipper in debug mode, because unsafe isn't permission to break the rules
    #[cfg(debug_assertions)]
    _tracker: Option<ZipperTracker<TrackingWrite>>,
}

//We only want a custom drop when we have a tracker
#[cfg(debug_assertions)]
impl<V> Drop for WriteZipperUntracked<'_, '_, V> {
    fn drop(&mut self) { }
}

impl<V: Clone + Send + Sync> Zipper for WriteZipperUntracked<'_, '_, V> {
    type ReadZipperT<'a> = ReadZipperUntracked<'a, 'a, V> where Self: 'a;

    fn at_root(&self) -> bool { self.z.at_root() }
    fn reset(&mut self) { self.z.reset() }
    fn path(&self) -> &[u8] { self.z.path() }
    fn path_exists(&self) -> bool { self.z.path_exists() }
    fn is_value(&self) -> bool { self.z.is_value() }
    fn val_count(&self) -> usize { self.z.val_count() }
    fn child_count(&self) -> usize { self.z.child_count() }
    fn child_mask(&self) -> [u64; 4] { self.z.child_mask() }
    fn descend_to<K: AsRef<[u8]>>(&mut self, k: K) -> bool { self.z.descend_to(k) }
    fn descend_to_byte(&mut self, k: u8) -> bool { self.z.descend_to_byte(k) }
    fn descend_indexed_branch(&mut self, child_idx: usize) -> bool { self.z.descend_indexed_branch(child_idx) }
    fn descend_first_byte(&mut self) -> bool { self.z.descend_first_byte() }
    fn descend_until(&mut self) -> bool { self.z.descend_until() }
    fn to_sibling(&mut self, next: bool) -> bool { self.z.to_sibling(next) }
    fn to_next_sibling_byte(&mut self) -> bool { self.z.to_next_sibling_byte() }
    fn to_prev_sibling_byte(&mut self) -> bool { self.z.to_prev_sibling_byte() }
    fn ascend(&mut self, steps: usize) -> bool { self.z.ascend(steps) }
    fn ascend_byte(&mut self) -> bool { self.z.ascend_byte() }
    fn ascend_until(&mut self) -> bool { self.z.ascend_until() }
    fn fork_read_zipper<'a>(&'a self) -> Self::ReadZipperT<'a> {
        let new_root_val = self.get_value();
        let rz_core = ReadZipperCore::new_with_node_and_path_internal(self.z.focus_stack.top().unwrap().as_tagged(), &self.z.key.node_key(), None, new_root_val);
        Self::ReadZipperT::new_forked_with_inner_zipper(rz_core)
    }
    fn make_map(&self) -> Option<BytesTrieMap<Self::V>> { self.z.make_map() }
}

impl<'a, 'k, V : Clone> zipper_priv::ZipperPriv for WriteZipperUntracked<'a, 'k, V> {
    type V = V;
    fn get_focus(&self) -> AbstractNodeRef<Self::V> { self.z.get_focus() }
    fn try_borrow_focus(&self) -> Option<&dyn TrieNode<Self::V>> { self.z.try_borrow_focus() }
}

impl <'a, 'k, V: Clone + Send + Sync> WriteZipperUntracked<'a, 'k, V> {
    /// Creates a new zipper, with a path relative to a node
    #[cfg(debug_assertions)]
    pub(crate) fn new_with_node_and_path(root_node: &'a mut TrieNodeODRc<V>, root_val: Option<&'a mut Option<V>>, path: &'k [u8], tracker: Option<ZipperTracker<TrackingWrite>>) -> Self {
        let core = WriteZipperCore::<'a, 'k, V>::new_with_node_and_path(root_node, root_val, path);
        Self { z: core, _tracker: tracker }
    }
    #[cfg(not(debug_assertions))]
    pub(crate) fn new_with_node_and_path(root_node: &'a mut TrieNodeODRc<V>, root_val: Option<&'a mut Option<V>>, path: &'k [u8]) -> Self {
        let core = WriteZipperCore::<'a, 'k, V>::new_with_node_and_path(root_node, root_val, path);
        Self { z: core }
    }
    /// See [WriteZipper::new_with_node_and_path_internal]
    #[cfg(debug_assertions)]
    pub(crate) fn new_with_node_and_path_internal(root_node: &'a mut TrieNodeODRc<V>, root_val: Option<&'a mut Option<V>>, path: &'k [u8], tracker: Option<ZipperTracker<TrackingWrite>>) -> Self {
        let core = WriteZipperCore::<'a, 'k, V>::new_with_node_and_path_internal(root_node, root_val, path);
        Self { z: core, _tracker: tracker }
    }
    #[cfg(not(debug_assertions))]
    pub(crate) fn new_with_node_and_path_internal(root_node: &'a mut TrieNodeODRc<V>, root_val: Option<&'a mut Option<V>>, path: &'k [u8]) -> Self {
        let core = WriteZipperCore::<'a, 'k, V>::new_with_node_and_path_internal(root_node, root_val, path);
        Self { z: core }
    }
    /// Consumes the `WriteZipperUntracked`, and returns a [ReadZipperUntracked] in its place
    ///
    /// The returned read zipper will have the same root and focus as the the consumed write zipper.
    pub fn into_read_zipper(mut self) -> ReadZipperUntracked<'a, 'static, V> {
        #[cfg(debug_assertions)]
        let tracker = self._tracker.take().map(|tracker| tracker.into_reader());
        let root_node = self.z.focus_stack.take_root().unwrap().borrow();
        let root_path = &self.z.key.prefix_buf[..self.z.key.root_key.len()];
        let descended_path = &self.z.key.prefix_buf[self.z.key.root_key.len()..];
        let root_val = core::mem::take(&mut self.z.root_val);
        let root_val = root_val.and_then(|root_val| root_val.as_ref());

        #[cfg(debug_assertions)]
        let mut new_zipper = ReadZipperUntracked::new_with_node_and_cloned_path(root_node, root_path, None, root_val, tracker);

        #[cfg(not(debug_assertions))]
        let mut new_zipper = ReadZipperUntracked::new_with_node_and_cloned_path(root_node, root_path, None, root_val);
        new_zipper.descend_to(descended_path);
        new_zipper
    }
}

impl<'a, V: Clone + Send + Sync> WriteZipper<V> for WriteZipperUntracked<'a, '_, V> {
    type ZipperHead<'z> = ZipperHead<'z, 'a, V> where Self: 'z;
    fn get_value(&self) -> Option<&V> { self.z.get_value() }
    fn get_value_mut(&mut self) -> Option<&mut V> { self.z.get_value_mut() }
    fn get_value_or_insert(&mut self, default: V) -> &mut V { self.z.get_value_or_insert(default) }
    fn get_value_or_insert_with<F>(&mut self, func: F) -> &mut V where F: FnOnce() -> V { self.z.get_value_or_insert_with(func) }
    fn set_value(&mut self, val: V) -> Option<V> { self.z.set_value(val) }
    fn remove_value(&mut self) -> Option<V> { self.z.remove_value() }
    fn zipper_head<'z>(&'z mut self) -> Self::ZipperHead<'z> { self.z.zipper_head() }
    fn graft<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) { self.z.graft(read_zipper) }
    fn graft_map(&mut self, map: BytesTrieMap<V>) { self.z.graft_map(map) }
    fn join<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool where V: Lattice { self.z.join(read_zipper) }
    fn join_map(&mut self, map: BytesTrieMap<V>) -> bool where V: Lattice { self.z.join_map(map) }
    fn join_into<Z: Zipper<V=V> + WriteZipper<V>>(&mut self, src_zipper: &mut Z) -> bool where V: Lattice { self.z.join_into(src_zipper) }
    fn drop_head(&mut self, byte_cnt: usize) -> bool where V: Lattice { self.z.drop_head(byte_cnt) }
    fn insert_prefix<K: AsRef<[u8]>>(&mut self, prefix: K) -> bool { self.z.insert_prefix(prefix) }
    fn remove_prefix(&mut self, n: usize) -> bool { self.z.remove_prefix(n) }
    fn meet<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool where V: Lattice { self.z.meet(read_zipper) }
    fn meet_2<ZA: Zipper<V=V>, ZB: Zipper<V=V>>(&mut self, rz_a: &ZA, rz_b: &ZB) -> bool where V: Lattice { self.z.meet_2(rz_a, rz_b) }
    fn subtract<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool where V: DistributiveLattice { self.z.subtract(read_zipper) }
    fn restrict<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool { self.z.restrict(read_zipper) }
    fn restricting<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool { self.z.restricting(read_zipper) }
    fn remove_branches(&mut self) -> bool { self.z.remove_branches() }
    fn take_map(&mut self) -> Option<BytesTrieMap<V>> { self.z.take_map() }
    fn remove_unmasked_branches(&mut self, mask: [u64; 4]) { self.z.remove_unmasked_branches(mask) }
}

impl<V: Clone + Send + Sync> WriteZipperPriv<V> for WriteZipperUntracked<'_, '_, V> {
    fn take_focus(&mut self) -> Option<TrieNodeODRc<V>> { self.z.take_focus() }
}

// ***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---
// WriteZipperCore (the actual implementation)
// ***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---***---

//GOAT: Discussion on whether to keep rooted zippers, or streamline the WriteZipper code
//UPDATE: this discussion is moot if we decide all WriteZippers can update their roots
//RESOLVED: WriteZippers are able to manipulate root values.  There is also now a root value on the map itself
//
// * Arguments for keeping rooted WriteZippers
//  - A. An API that allows `let mut z = map.write_zipper_at_path()` folowed by `z.set_value` is convenient and
//    easy to explain.  Non-rooted zippers can't set values at their root.
//
//  - B. A non-rooted WriteZipper cannot modify its parent node, which means it cannot upgrade its root node
//    which means the root node needs to be able to accomodate any onward path, which means it needs to be
//    a DenseNode or similar.  This may mean unnuecessary node upgrading, for example at the root of a map;
//    ultimately this eats into the utility of light-weight maps.
//
//  - C. We'd need to reimplement the `PathMap::some_write_method` write methods to operate directly on the map nodes, rather
//    than do it via a temporarily-created WriteZipper
//
// * Arguments for streamlining
//  - A'. The convenient API isn't possible much of the time anyway, because a WriteZipper at the PathMap's
//    root, or a WriteZipper created via `write_zipper_at_exclusive_path` can never be rooted.  And therefore
//    it may be better to just say "WriteZippers cannot modify values at their root", instead of saying:
//    "WriteZipper created with... modify values at their root".
//
//  - B'. Obviously cutting branches and streamlining the WriteZipper code
//
//  - C'. The code that fixes up a zipper (WriteZipper::mend_root) is pure waste if the zipper is temporary;
//    e.g. just part of the implementation of `PathMap::some_write_method`
//

/// The core implementation of WriteZipper
pub(crate) struct WriteZipperCore<'a, 'k, V> {
    pub(crate) key: KeyFields<'k>,

    pub(crate) root_val: Option<&'a mut Option<V>>,

    /// The stack of node references.  We need a "rooted" Vec in case we need to upgrade the node at the root of the zipper
    pub(crate) focus_stack: MutCursorRootedVec<&'a mut TrieNodeODRc<V>, dyn TrieNode<V> + 'static>,
}

/// The part of the [WriteZipper] that contains the key-related fields.  So it can be borrowed separately
pub(crate) struct KeyFields<'path> {
    /// A reference to the part of the key within the root node that represents the zipper root
    root_key: SliceOrLen<'path>,
    /// Stores the entire path from the root node, including the bytes from root_key
    pub(crate) prefix_buf: Vec<u8>,
    /// Stores the lengths for each successive node's key
    pub(crate) prefix_idx: Vec<usize>,
}

impl<V: Clone + Send + Sync> Zipper for WriteZipperCore<'_, '_, V> {
    type ReadZipperT<'a> = () where Self: 'a;

    #[inline]
    fn at_root(&self) -> bool {
        self.key.prefix_buf.len() <= self.key.root_key.len()
    }

    fn reset(&mut self) {
        self.focus_stack.to_bottom();
        self.key.prefix_buf.truncate(self.key.root_key.len());
        self.key.prefix_idx.clear();
    }

    fn path(&self) -> &[u8] {
        if self.key.prefix_buf.len() > 0 {
            &self.key.prefix_buf[self.key.root_key.len()..]
        } else {
            &[]
        }
    }

    fn path_exists(&self) -> bool {
        let key = self.key.node_key();
        if key.len() > 0 {
            self.focus_stack.top().unwrap().node_contains_partial_key(key)
        } else {
            true
        }
    }

    fn is_value(&self) -> bool {
        self.focus_stack.top().unwrap().node_contains_val(self.key.node_key())
    }

    fn val_count(&self) -> usize {
        let focus = self.get_focus();
        if focus.is_none() {
            0
        } else {
            val_count_below_root(focus.borrow()) + (self.is_value() as usize)
        }
    }

    fn child_count(&self) -> usize {
        let focus_node = self.focus_stack.top().unwrap();
        let node_key = self.key.node_key();
        if node_key.len() == 0 {
            return focus_node.count_branches(b"");
        }
        match focus_node.node_get_child(node_key) {
            Some((consumed_bytes, child_node)) => {
                if node_key.len() >= consumed_bytes {
                    child_node.count_branches(&node_key[consumed_bytes..])
                } else {
                    0
                }
            },
            None => focus_node.count_branches(node_key)
        }
    }

    fn child_mask(&self) -> [u64; 4] {
        let focus_node = self.focus_stack.top().unwrap();
        let node_key = self.key.node_key();
        if node_key.len() == 0 {
            return focus_node.node_branches_mask(b"")
        }
        match focus_node.node_get_child(node_key) {
            Some((consumed_bytes, child_node)) => {
                if node_key.len() >= consumed_bytes {
                    child_node.node_branches_mask(&node_key[consumed_bytes..])
                } else {
                    [0; 4]
                }
            },
            None => focus_node.node_branches_mask(node_key)
        }
    }

    fn descend_to<K: AsRef<[u8]>>(&mut self, k: K) -> bool {
        let key = k.as_ref();
        self.key.prepare_buffers();
        self.key.prefix_buf.extend(key);
        self.descend_to_internal();
        let node_key = self.key.node_key();
        if node_key.len() > 0 {
            self.focus_stack.top().unwrap().node_contains_partial_key(node_key)
        } else {
            true
        }
    }

    fn descend_to_byte(&mut self, k: u8) -> bool {
        self.descend_to(&[k])
    }

    fn descend_indexed_branch(&mut self, child_idx: usize) -> bool {
        panic!()
    }

    fn descend_first_byte(&mut self) -> bool {
        panic!()
    }

    fn descend_until(&mut self) -> bool {
        panic!()
    }

    fn to_sibling(&mut self, next: bool) -> bool {
        panic!()
    }

    fn to_next_sibling_byte(&mut self) -> bool {
        panic!()
    }

    fn to_prev_sibling_byte(&mut self) -> bool {
        panic!()
    }

    fn ascend(&mut self, mut steps: usize) -> bool {
        loop {
            if self.key.node_key().len() == 0 {
                self.ascend_across_nodes();
            }
            if steps == 0 {
                return true
            }
            if self.at_root() {
                return false
            }
            debug_assert!(self.key.node_key().len() > 0);
            let cur_jump = steps.min(self.key.excess_key_len());
            self.key.prefix_buf.truncate(self.key.prefix_buf.len() - cur_jump);
            steps -= cur_jump;
        }
    }

    fn ascend_byte(&mut self) -> bool {
        self.ascend(1)
    }

    fn ascend_until(&mut self) -> bool {
        if self.at_root() {
            return false;
        }
        loop {
            self.ascend_within_node();
            if self.at_root() {
                return true;
            }
            if self.key.node_key().len() == 0 {
                self.ascend_across_nodes();
            }
            if self.child_count() > 1 {
                break;
            }
        }
        debug_assert!(self.key.node_key().len() > 0); //We should never finish with a zero-length node-key
        true
    }

    fn fork_read_zipper<'a>(&'a self) -> Self::ReadZipperT<'a> {
        panic!() //Don't fork the WriteZipperCore, fork the WriteZipperTracker or WriteZipperUntracked
    }

    fn make_map(&self) -> Option<BytesTrieMap<Self::V>> {
        self.get_focus().into_option().map(|node| BytesTrieMap::new_with_root(Some(node)))
    }
}

impl<'a, 'k, V : Clone> zipper_priv::ZipperPriv for WriteZipperCore<'a, 'k, V> {
    type V = V;

    fn get_focus(&self) -> AbstractNodeRef<Self::V> {
        self.focus_stack.top().unwrap().get_node_at_key(self.key.node_key())
    }
    fn try_borrow_focus(&self) -> Option<&dyn TrieNode<Self::V>> {
        let node_key = self.key.node_key();
        if node_key.len() == 0 {
            Some(self.focus_stack.top().unwrap())
        } else {
            match self.focus_stack.top().unwrap().node_get_child(node_key) {
                Some((consumed_bytes, child_node)) => {
                    debug_assert_eq!(consumed_bytes, node_key.len());
                    Some(child_node)
                },
                None => None
            }
        }
    }
}

impl <'a, 'path, V: Clone + Send + Sync> WriteZipperCore<'a, 'path, V> {
    /// Creates a new zipper, with a path relative to a node
    pub(crate) fn new_with_node_and_path(root_node: &'a mut TrieNodeODRc<V>, root_val: Option<&'a mut Option<V>>, path: &'path [u8]) -> Self {
        let (key, node) = node_along_path_mut(root_node, path, true);
        Self::new_with_node_and_path_internal(node, root_val, key)
    }
    /// See [WriteZipper::new_with_node_and_path_internal]
    pub(crate) fn new_with_node_and_path_internal(root_node: &'a mut TrieNodeODRc<V>, root_val: Option<&'a mut Option<V>>, path: &'path [u8]) -> Self {
        let mut focus_stack = MutCursorRootedVec::new(root_node);
        focus_stack.advance_from_root(|root| Some(root.make_mut()));
        debug_assert!((path.len() == 0) != (root_val.is_none())); //We must have either a path or a root_val, but never both
        Self {
            key: KeyFields::new(path),
            root_val,
            focus_stack,
        }
    }

    /// Internal method to borrow the node at the zipper's focus, splitting the node if necessary
    pub(crate) fn splitting_borrow_focus(&mut self) -> (&dyn TrieNode<V>, Option<&V>) {
        let self_ptr: *mut Self = self;
        let node = match self.try_borrow_focus() {
            Some(root) => root,
            None => {
                //SAFETY: This is another "We need Polonius" case.  We're finished with the borrow if we get here.
                let self_ref = unsafe{ &mut *self_ptr };
                self_ref.split_at_focus();
                self.try_borrow_focus().unwrap()
            },
        };
        let val = self.get_value();
        (node, val)
    }

    /// Internal method to ensure the focus begins at its own node, splitting the node if necessary
    pub(crate) fn split_at_focus(&mut self) {
        let sub_branch_added = self.in_zipper_mut_static_result(
            |node, key| {
                let new_node = if let Some(remaining) = node.take_node_at_key(key) {
                    remaining
                } else {
                    #[cfg(not(feature = "all_dense_nodes"))]
                    {
                        TrieNodeODRc::new(crate::line_list_node::LineListNode::new())
                    }
                    #[cfg(feature = "all_dense_nodes")]
                    {
                        TrieNodeODRc::new(crate::dense_byte_node::DenseByteNode::new())
                    }
                };
                node.node_set_branch(key, new_node)
            },
            |_, _| true);
        if sub_branch_added {
            self.mend_root();
            self.descend_to_internal();
        }
    }

    /// Internal method to re-borrow a WriteZipperCore without the `'path` lifetime
    fn as_static_path_zipper(&mut self) -> &mut WriteZipperCore<'a, 'static, V> {
        assert!(!self.key.root_key.is_slice());
        unsafe{ &mut *(self as *mut WriteZipperCore<V>).cast() }
    }

    //GOAT, the concept of a regularized zipper might be very useful for WriteZippers, so I may be able to delete this code
    // /// Ensures the zipper is in its regularized form
    // ///
    // /// Unlike a ReadZipper, a WriteZipper's regularized form is holding the parent node at the top of the
    // /// `focus_stack`, where `node_key()` contains the key necesary to access the zipper's focus.  The
    // /// reason is because the most common and expensive operations in a ReadZipper are moves and iteration,
    // /// while the most common operations in a WriteZipper are sets and grafts.  Therefore the regularized
    // /// form is the closest to what's needed to perform those ops
    // ///
    // /// Therefore, `node_key().len() == 0` is usually deregularized.
    // ///
    // /// There is a special case, however, when the `focus_stack.top()` is the zipper's root node.  A
    // /// "thread-safe" WriteZipper must be able to function without accessing the parent node, because
    // /// the parent node may be shared among multiple zippers.
    // #[inline]
    // fn is_regularized(&self) -> bool {
    //     let key_start = self.key.node_key_start();
    //     self.key.prefix_buf.len() > key_start || self.at_root()
    // }

    /// See [WriteZipper::get_value]
    pub fn get_value(&self) -> Option<&V> {
        let node_key = self.key.node_key();
        if node_key.len() > 0 {
            self.focus_stack.top().unwrap().node_get_val(node_key)
        } else {
            debug_assert!(self.at_root());
            self.root_val.as_ref().and_then(|val| val.as_ref())
        }
    }
    /// See [WriteZipper::get_value_mut]
    pub fn get_value_mut(&mut self) -> Option<&mut V> {
        let node_key = self.key.node_key();
        if node_key.len() > 0 {
            self.focus_stack.top_mut().unwrap().node_get_val_mut(node_key)
        } else {
            debug_assert!(self.at_root());
            self.root_val.as_mut().and_then(|val| val.as_mut())
        }
    }
    /// See [WriteZipper::get_value_or_insert]
    pub fn get_value_or_insert(&mut self, default: V) -> &mut V {
        let created_subnode = self.in_zipper_mut_static_result(
            |node, key| {
                if !node.node_contains_val(key) {
                    node.node_set_val(key, default).map(|(_old_val, created_subnode)| created_subnode)
                } else {
                    Ok(false)
                }
            },
            |_, _| true);
        if created_subnode {
            self.mend_root();
            self.descend_to_internal();
        }
        self.get_value_mut().unwrap()
    }
    /// See [WriteZipper::get_value_or_insert_with]
    pub fn get_value_or_insert_with<F>(&mut self, func: F) -> &mut V
        where F: FnOnce() -> V
    {
        let created_subnode = self.in_zipper_mut_static_result(
            |node, key| {
                if !node.node_contains_val(key) {
                    node.node_set_val(key, func()).map(|(_old_val, created_subnode)| created_subnode)
                } else {
                    Ok(false)
                }
            },
            |_, _| true);
        if created_subnode {
            self.mend_root();
            self.descend_to_internal();
        }
        self.get_value_mut().unwrap()
    }
    /// See [WriteZipper::set_value]
    pub fn set_value(&mut self, val: V) -> Option<V> {
        if self.key.node_key().len() == 0 {
            debug_assert!(self.at_root());
            let root_val_ref = self.root_val.as_mut().unwrap();
            let mut temp_val = Some(val);
            core::mem::swap(*root_val_ref, &mut temp_val);
            return temp_val
        }
        let (old_val, created_subnode) = self.in_zipper_mut_static_result(
            |node, remaining_key| node.node_set_val(remaining_key, val),
            |_new_leaf_node, _remaining_key| (None, false));
        if created_subnode {
            self.mend_root();
            self.descend_to_internal();
        }
        old_val
    }
    /// See [WriteZipper::remove_value]
    pub fn remove_value(&mut self) -> Option<V> {
        if self.key.node_key().len() == 0 {
            debug_assert!(self.at_root());
            let root_val_ref = self.root_val.as_mut().unwrap();
            return core::mem::take(*root_val_ref)
        }
        let focus_node = self.focus_stack.top_mut().unwrap();
        if let Some(result) = focus_node.node_remove_val(self.key.node_key()) {
            if focus_node.node_is_empty() {
                self.prune_path();
            }
            Some(result)
        } else {
            None
        }
    }
    /// See [WriteZipper::zipper_head]
    pub fn zipper_head<'z>(&'z mut self) -> ZipperHead<'z, 'a, V> {
        self.key.prepare_buffers();
        ZipperHead::new_borrowed(self.as_static_path_zipper())
    }
    /// Consumes the WriteZipper, returning a ZipperHead
    ///
    /// NOTE: Currently this is an internal-only method to enable the [PathMap::zipper_head] method,
    /// although it might be convenient to expose it publicly.  We'd need to make sure the ZipperHead could
    /// carry along the tracker
    pub(crate) fn into_zipper_head(self) -> ZipperHead<'a, 'a, V> where 'path: 'static {
        ZipperHead::new_owned(self)
    }
    /// See [WriteZipper::graft]
    pub fn graft<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) {
        self.graft_internal(read_zipper.get_focus().into_option())
    }
    /// See [WriteZipper::graft_map]
    pub fn graft_map(&mut self, map: BytesTrieMap<V>) {
        self.graft_internal(map.into_root());
    }
    /// See [WriteZipper::join]
    pub fn join<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool where V: Lattice {
        let src = read_zipper.get_focus();
        if src.is_none() {
            return false
        }
        match self.get_focus().try_borrow() {
            Some(self_node) => {
                let joined = self_node.join_dyn(src.borrow());
                self.graft_internal(Some(joined));
                true
            },
            None => { self.graft_internal(src.into_option()); true }
        }
    }
    /// See [WriteZipper::join_map]
    pub fn join_map(&mut self, map: BytesTrieMap<V>) -> bool where V: Lattice {
        let src = match map.into_root() {
            Some(src) => src,
            None => return false
        };
        match self.get_focus().try_borrow() {
            Some(self_node) => {
                let joined = self_node.join_dyn(src.borrow());
                self.graft_internal(Some(joined));
                true
            },
            None => { self.graft_internal(Some(src)); true }
        }
    }
    /// See [WriteZipper::join_into]
    pub fn join_into<Z: Zipper<V=V> + WriteZipper<V>>(&mut self, src_zipper: &mut Z) -> bool where V: Lattice {
        match src_zipper.take_focus() {
            None => return false,
            Some(src) => {
                let new_node = match self.take_focus() {
                    Some(mut self_node) => {
                        match self_node.make_mut().join_into_dyn(src) {
                            Ok(()) => self_node,
                            Err(replacement_node) => replacement_node,
                        }
                    },
                    None => { src }
                };
                self.graft_internal(Some(new_node));
                true
            }
        }
        //GOAT!!!!!  We should prune the path at the source zipper, since we're effectively leaving behind an empty node
    }
    /// See [WriteZipper::drop_head]
    pub fn drop_head(&mut self, byte_cnt: usize) -> bool where V: Lattice {
        match self.get_focus().into_option() {
            Some(mut self_node) => {
                match self_node.make_mut().drop_head_dyn(byte_cnt) {
                    Some(new_node) => {
                        self.graft_internal(Some(new_node));
                        true
                    },
                    None => { false }
                }
            },
            None => { false }
        }
        //GOAT!!!!!  We should prune the path upstream, if we ended up removing all downstream paths
    }
    /// See [WriteZipper::insert_prefix]
    pub fn insert_prefix<K: AsRef<[u8]>>(&mut self, prefix: K) -> bool {
        let prefix = prefix.as_ref();
        match self.get_focus().into_option() {
            Some(mut focus_node) => {
                let focus_node = core::mem::take(&mut focus_node);
                let prefixed = make_parents(prefix, focus_node);
                self.graft_internal(Some(prefixed));
                true
            },
            None => { false }
        }
    }
    /// See [WriteZipper::remove_prefix]
    pub fn remove_prefix(&mut self, n: usize) -> bool {

        let downstream_node = self.get_focus().into_option()
            .map(|mut focus_node| core::mem::take(&mut focus_node));

        let fully_ascended = self.ascend(n);

        self.graft_internal(downstream_node);
        fully_ascended
    }
    /// See [WriteZipper::meet]
    pub fn meet<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool where V: Lattice {
        let src = read_zipper.get_focus();
        if src.is_none() {
            return false
        }
        match self.get_focus().try_borrow() {
            Some(self_node) => {
                match self_node.pmeet_dyn(src.borrow()) {
                    AlgebraicResult::Element(intersection) => self.graft_internal(Some(intersection)),
                    AlgebraicResult::None => self.graft_internal(None),
                    AlgebraicResult::Identity(mask) => {
                        if mask & SELF_IDENT > 0 {
                            //nothing to do
                        } else {
                            debug_assert_eq!(mask, COUNTER_IDENT); //It's gotta be self or other
                            self.graft_internal(Some(src.into_option().unwrap()))
                        }
                    },
                }
                true
            },
            None => false
        }
    }
    /// See [WriteZipper::meet_2]
    pub fn meet_2<ZA: Zipper<V=V>, ZB: Zipper<V=V>>(&mut self, rz_a: &ZA, rz_b: &ZB) -> bool where V: Lattice {
        let a_focus = rz_a.get_focus();
        let a = match a_focus.try_borrow() {
            Some(src) => src,
            None => return false
        };
        let b_focus = rz_b.get_focus();
        let b = match b_focus.try_borrow() {
            Some(src) => src,
            None => return false
        };
        match a.pmeet_dyn(b) {
            AlgebraicResult::Element(intersection) => self.graft_internal(Some(intersection)),
            AlgebraicResult::None => self.graft_internal(None),
            AlgebraicResult::Identity(mask) => {
                if mask & SELF_IDENT > 0 {
                    self.graft_internal(Some(a_focus.into_option().unwrap()))
                } else {
                    debug_assert_eq!(mask, COUNTER_IDENT); //It's gotta be a or b
                    self.graft_internal(Some(b_focus.into_option().unwrap()))
                }
            },
        }
        true
    }
    /// See [WriteZipper::subtract]
    pub fn subtract<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool where V: DistributiveLattice {
        let src = read_zipper.get_focus();
        if src.is_none() {
            return false
        }
        match self.get_focus().try_borrow() {
            Some(self_node) => {
                match self_node.psubtract_dyn(src.borrow()) {
                    AlgebraicResult::Element(diff) => self.graft_internal(Some(diff)),
                    AlgebraicResult::None => self.graft_internal(None),
                    AlgebraicResult::Identity(mask) => {
                        debug_assert_eq!(mask, SELF_IDENT); //subtract is non-commutative
                        //nothing to actually do
                    },
                }
                true
            },
            None => false
        }
    }
    /// See [WriteZipper::restrict]
    pub fn restrict<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool {
        let src = read_zipper.get_focus();
        if src.is_none() {
            return false
        }
        match self.get_focus().try_borrow() {
            Some(self_node) => {
                match self_node.prestrict_dyn(src.borrow()) {
                    AlgebraicResult::Element(restricted) => self.graft_internal(Some(restricted)),
                    AlgebraicResult::None => self.graft_internal(None),
                    AlgebraicResult::Identity(mask) => {
                        debug_assert_eq!(mask, SELF_IDENT); //restrict is non-commutative
                        //nothing to actually do
                    },
                }
                true
            },
            None => false
        }
    }
    /// See [WriteZipper::restricting]
    pub fn restricting<Z: Zipper<V=V>>(&mut self, read_zipper: &Z) -> bool {
        let src = read_zipper.get_focus();
        if src.is_none() {
            return false
        }
        match self.get_focus().try_borrow() {
            Some(self_node) => {
                match src.borrow().prestrict_dyn(self_node) {
                    AlgebraicResult::Element(restricted) => self.graft_internal(Some(restricted)),
                    AlgebraicResult::None => self.graft_internal(None),
                    AlgebraicResult::Identity(mask) => {
                        debug_assert_eq!(mask, SELF_IDENT); //restrict is non-commutative
                        self.graft_internal(src.into_option())
                    },
                }
                true
            },
            None => false
        }
    }
    /// See [WriteZipper::remove_branches]
    pub fn remove_branches(&mut self) -> bool {
        let node_key = self.key.node_key();
        if node_key.len() > 0 {
            let focus_node = self.focus_stack.top_mut().unwrap();
            if focus_node.node_remove_all_branches(node_key) {
                if focus_node.node_is_empty() {
                    self.prune_path();
                }
                true
            } else {
                false
            }
        } else {
            debug_assert_eq!(self.focus_stack.depth(), 1);
            if self.focus_stack.top().unwrap().node_is_empty() {
                return false
            } else {
                self.focus_stack.to_root();
                let stack_root = self.focus_stack.root_mut().unwrap();
                **stack_root = TrieNodeODRc::new(EmptyNode::new());
                self.focus_stack.advance_from_root(|root| Some(root.make_mut()));
                true
            }
        }
    }
    /// See [WriteZipper::take_map]
    pub fn take_map(&mut self) -> Option<BytesTrieMap<V>> {
        self.take_focus().map(|node| BytesTrieMap::new_with_root(Some(node)))
    }
    /// See [WriteZipper::remove_unmasked_branches]
    pub fn remove_unmasked_branches(&mut self, mask: [u64; 4]) {
        let focus_node = self.focus_stack.top_mut().unwrap();
        let node_key = self.key.node_key();
        if node_key.len() > 0 {
            match focus_node.node_get_child_mut(node_key) {
                Some((consumed_bytes, child_node)) => {
                    if node_key.len() >= consumed_bytes {
                        child_node.make_mut().node_remove_unmasked_branches(&node_key[consumed_bytes..], mask);
                        if child_node.borrow().node_is_empty() {
                            focus_node.node_remove_all_branches(&node_key[..consumed_bytes]);
                        }
                    } else {
                        //Zipper is positioned at non-existent node.  Removing anything from nothing is nothing
                    }
                },
                None => {
                    focus_node.node_remove_unmasked_branches(node_key, mask);
                }
            }
        } else {
            focus_node.node_remove_unmasked_branches(node_key, mask);
        }
        if focus_node.node_is_empty() {
            self.prune_path();
        }
    }

    /// Internal method, Removes and returns the node at the zipper's focus
    #[inline]
    fn take_focus(&mut self) -> Option<TrieNodeODRc<V>> {
        let focus_node = self.focus_stack.top_mut().unwrap();
        let node_key = self.key.node_key();
        if node_key.len() == 0 {
            debug_assert!(self.at_root());
            let mut replacement_node = TrieNodeODRc::new(EmptyNode::new());
            self.focus_stack.backtrack();
            let stack_root = self.focus_stack.root_mut().unwrap();
            core::mem::swap(*stack_root, &mut replacement_node);
            self.focus_stack.advance_from_root(|root| Some(root.make_mut()));
            if !replacement_node.borrow().node_is_empty() {
                Some(replacement_node)
            } else {
                None
            }
        } else {
            if let Some(new_node) = focus_node.take_node_at_key(node_key) {
                if focus_node.node_is_empty() {
                    self.prune_path();
                }
                Some(new_node)
            } else {
                None
            }
        }
    }

    /// Internal implementation of graft, and other methods that do the same thing
    #[inline]
    fn graft_internal(&mut self, src: Option<TrieNodeODRc<V>>) {
        match src {
            Some(src) => {
                debug_assert!(!src.borrow().node_is_empty());
                if self.key.node_key().len() > 0 {
                    //The focus_stack.top() is the parent node of the focus, so we'll replace its child
                    let sub_branch_added = self.in_zipper_mut_static_result(
                        |node, key| {
                            node.node_set_branch(key, src)
                        },
                        |_, _| true);
                    if sub_branch_added {
                        self.mend_root();
                        self.descend_to_internal();
                    }
                } else {
                    //The zipper is at the root, so we need to replace the root node
                    debug_assert!(self.at_root());
                    debug_assert_eq!(self.key.prefix_idx.len(), 0);
                    debug_assert_eq!(self.key.prefix_buf.len(), self.key.root_key.len());
                    debug_assert_eq!(self.focus_stack.depth(), 1);
                    self.focus_stack.to_root();
                    let stack_root = self.focus_stack.root_mut().unwrap();
                    **stack_root = src;
                    self.focus_stack.advance_from_root(|root| Some(root.make_mut()));
                }
            },
            None => { self.remove_branches(); }
        }
    }

    /// An internal function to attempt a mutable operation on a node, and replace the node if the node needed
    /// to be upgraded
    #[inline]
    pub(crate) fn in_zipper_mut_static_result<NodeF, RetryF, R>(&mut self, node_f: NodeF, retry_f: RetryF) -> R
        where
        NodeF: FnOnce(&mut dyn TrieNode<V>, &[u8]) -> Result<R, TrieNodeODRc<V>>,
        RetryF: FnOnce(&mut dyn TrieNode<V>, &[u8]) -> R,
    {
        let key = self.key.node_key();
        match node_f(self.focus_stack.top_mut().unwrap(), key) {
            Ok(result) => result,
            Err(replacement_node) => {
                replace_top_node(&mut self.focus_stack, &self.key, replacement_node);
                retry_f(self.focus_stack.top_mut().unwrap(), key)
            },
        }
    }

    /// Internal method to recursively prune empty nodes from the trie, starting at the focus, and working
    /// upward until a value or branch is encountered.
    ///
    /// This method does not move the zipper, but may cause [Self::path_exists] to return `false`
    ///
    /// WARNING: this is one of the few zipper methods that allocs a temp buffer and doesn't try and uphold
    /// the "constant-time" property, but it should still be cheaper, on average, compared with other methods
    /// to do the same thing.
    #[inline]
    fn prune_path(&mut self) {
        debug_assert!(self.focus_stack.top().unwrap().node_is_empty());
        if self.at_root() {
            return
        }

        let old_path = self.key.prefix_buf.clone();
        self.ascend_until();

        let onward_path = &old_path[self.key.prefix_buf.len()..];
        self.descend_to(&onward_path[0..1]);
        self.remove_branches();

        //Move back to the original location, although it will now be non-existent
        self.key.prefix_buf = old_path;
    }

    /// Internal method that regularizes the `focus_stack` if nodes were created above the root
    #[inline]
    pub(crate) fn mend_root(&mut self) {
        if self.key.prefix_idx.len() == 0 && self.key.root_key.len() > 1 {
            debug_assert_eq!(self.focus_stack.depth(), 1);
            let root_ref = self.focus_stack.take_root().unwrap();
            let node = if self.key.root_key.is_slice() {
                let (key, node) = node_along_path_mut(root_ref, &self.key.root_key.as_slice(), true);
                self.key.root_key.set_slice(key);
                node
            } else {
                let root_slice = &self.key.prefix_buf[..self.key.root_key.len()];
                let (key, node) = node_along_path_mut(root_ref, root_slice, true);
                let new_len = key.len();
                let bytes_to_remove = root_slice.len() - new_len;
                //TODO.  I can speed this up with unsafe.  This should be in stdlib!!
                for _ in 0..bytes_to_remove {
                    self.key.prefix_buf.remove(0);
                }
                self.key.root_key.set_len(new_len);
                node
            };
            self.focus_stack.replace_root(node);
            self.focus_stack.advance_from_root(|root| Some(root.make_mut()));
        }
    }

    /// Internal method to perform the part of `descend_to` that moves the focus node
    pub(crate) fn descend_to_internal(&mut self) {

        let mut key_start = self.key.node_key_start();
        //NOTE: this is a copy of the self.key.node_key() function, but we can't borrow the whole key structure in this code
        let mut key = if self.key.prefix_buf.len() > 0 {
            &self.key.prefix_buf[key_start..]
        } else {
            self.key.root_key.try_as_slice().unwrap_or(&[])
        };
        //Explanation: This 2 is based on the fact that a WriteZipper's focus_stack holds the parent node
        // to the focus, so we must have a `node_key` unless the zipper is at the root, and the minimum
        // `node_key` length is 1 byte
        if key.len() < 2 {
            return;
        }

        //Step until we get to the end of the key or find a leaf node
        self.focus_stack.advance_if_empty(|root| root.make_mut());
        while self.focus_stack.advance(|node| {
            if let Some((consumed_byte_cnt, next_node)) = node.node_get_child_mut(key) {
                if consumed_byte_cnt < key.len() {
                    key_start += consumed_byte_cnt;
                    self.key.prefix_idx.push(key_start);
                    key = &key[consumed_byte_cnt..];
                    Some(next_node.make_mut())
                } else {
                    None
                }
            } else {
                None
            }
        }) { }
    }
    /// Internal method which doesn't actually move the zipper, but ensures `self.node_key().len() > 0`
    /// WARNING, must never be called if `self.node_key().len() != 0`
    #[inline]
    fn ascend_across_nodes(&mut self) {
        debug_assert!(self.key.node_key().len() == 0);
        self.focus_stack.try_backtrack_node();
        self.key.prefix_idx.pop();
    }
    /// Internal method used to impement `ascend_until` when ascending within a node
    #[inline]
    fn ascend_within_node(&mut self) {
        let branch_key = self.focus_stack.top().unwrap().prior_branch_key(self.key.node_key());
        let new_len = self.key.root_key.len().max(self.key.node_key_start() + branch_key.len());
        self.key.prefix_buf.truncate(new_len);
    }
}

/// An internal function to replace the node at a the top of the focus stack
#[inline]
pub(crate) fn replace_top_node<V>(focus_stack: &mut MutCursorRootedVec<&mut TrieNodeODRc<V>, dyn TrieNode<V>>,
    key: &KeyFields, replacement_node: TrieNodeODRc<V>)
{
    focus_stack.backtrack();
    match focus_stack.top_mut() {
        Some(parent_node) => {
            let parent_key = key.parent_key();
            parent_node.node_replace_child(parent_key, replacement_node);
            focus_stack.advance(|node| node.node_get_child_mut(parent_key).map(|(_, child_node)| child_node.make_mut()));
        },
        None => {
            let stack_root = focus_stack.root_mut().unwrap();
            **stack_root = replacement_node;
            focus_stack.advance_from_root(|root| Some(root.make_mut()));
        }
    }
}

/// An internal function to replace the node at a the top of the focus stack
#[inline]
pub(crate) fn swap_top_node<V: Clone + Send + Sync, F>(focus_stack: &mut MutCursorRootedVec<&mut TrieNodeODRc<V>, dyn TrieNode<V>>,
    key: &KeyFields, func: F)
    where F: FnOnce(TrieNodeODRc<V>) -> Option<TrieNodeODRc<V>>
{
    focus_stack.backtrack();
    match focus_stack.top_mut() {
        Some(parent_node) => {
            let parent_key = key.parent_key();
            let existing_node = parent_node.take_node_at_key(parent_key).unwrap();
            match func(existing_node) {
                Some(replacement_node) => { parent_node.node_set_branch(parent_key, replacement_node).unwrap(); },
                None => {},
            }
            focus_stack.advance(|node| node.node_get_child_mut(parent_key).map(|(_, child_node)| child_node.make_mut()));
        },
        None => {
            let stack_root = focus_stack.root_mut().unwrap();
            let mut temp_node = TrieNodeODRc::new(EmptyNode::new());
            core::mem::swap(&mut temp_node, *stack_root);
            match func(temp_node) {
                Some(replacement_node) => { **stack_root = replacement_node; },
                None => { },
            }
            focus_stack.advance_from_root(|root| Some(root.make_mut()));
        }
    }
}

/// Internal function to create a parent path leading up to the supplied `child_node`
#[inline]
fn make_parents<V: Clone + Send + Sync>(path: &[u8], child_node: TrieNodeODRc<V>) -> TrieNodeODRc<V> {

    #[cfg(not(feature = "all_dense_nodes"))]
    {
        #[cfg(not(feature = "bridge_nodes"))]
        {
            let mut new_node = crate::line_list_node::LineListNode::new();
            new_node.node_set_branch(path, child_node).unwrap_or_else(|_| panic!());
            TrieNodeODRc::new(new_node)
        }
        #[cfg(feature = "bridge_nodes")]
        {
            let new_node = crate::bridge_node::BridgeNode::new(path, true, child_node.into());
            TrieNodeODRc::new(new_node)
        }
    }

    #[cfg(feature = "all_dense_nodes")]
    {
        let mut end = child_node;
        for i in (0..path.len()).rev() {
            let mut new_node = crate::dense_byte_node::DenseByteNode::new();
            new_node.set_child(path[i], end);
            end = TrieNodeODRc::new(new_node);
        }
        end
    }
}

impl<'k> KeyFields<'k> {
    #[inline]
    fn new(path: &'k [u8]) -> Self {
        Self {
            root_key: path.into(),
            prefix_buf: vec![],
            prefix_idx: vec![],
        }
    }
    /// Internal method to ensure buffers to facilitate movement of zipper are allocated and initialized
    #[inline]
    fn prepare_buffers(&mut self) {
        if self.prefix_buf.capacity() == 0 {
            self.prefix_buf = Vec::with_capacity(EXPECTED_PATH_LEN);
            self.prefix_idx = Vec::with_capacity(EXPECTED_DEPTH);
            self.prefix_buf.extend(self.root_key.as_slice());
            self.root_key.make_len();
        }
    }
    /// Internal method returning the index to the key char beyond the path to the `self.focus_node`
    #[inline]
    pub(crate) fn node_key_start(&self) -> usize {
        self.prefix_idx.last().map(|i| *i).unwrap_or(0)
    }
    /// Internal method returning the key within the focus node
    #[inline]
    pub(crate) fn node_key(&self) -> &[u8] {
        if self.prefix_buf.len() > 0 {
            let key_start = self.node_key_start();
            &self.prefix_buf[key_start..]
        } else {
            self.root_key.try_as_slice().unwrap_or(&[])
        }
    }
    /// Internal method similar to `self.node_key().len()`, but returns the number of chars that can be
    /// legally ascended within the node, taking into account the root_key
    #[inline]
    fn excess_key_len(&self) -> usize {
        self.prefix_buf.len() - self.prefix_idx.last().map(|i| *i).unwrap_or(self.root_key.len())
    }
    /// Internal method returning the key that leads to `self.focus_node` within the parent
    #[inline]
    pub(crate) fn parent_key(&self) -> &[u8] {
        if self.prefix_buf.len() > 0 {
            let key_start = if self.prefix_idx.len() > 1 {
                unsafe{ *self.prefix_idx.get_unchecked(self.prefix_idx.len()-2) }
            } else {
                0
            };
            &self.prefix_buf[key_start..self.node_key_start()]
        } else {
            unreachable!()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::trie_map::*;
    use crate::zipper::*;

    #[test]
    fn write_zipper_set_value_test() {
        let mut map = BytesTrieMap::<usize>::new();
        let mut zipper = map.write_zipper_at_path(b"in");
        for i in 0usize..32 {
            zipper.descend_to_byte(0);
            zipper.descend_to(i.to_be_bytes());
            zipper.set_value(i);
            zipper.reset();
        }
        drop(zipper);

        // for (k, v) in map.iter() {
        //     println!("{:?} {v}", k);
        // }

        let mut zipper = map.read_zipper_at_path(b"in\0");
        for i in 0usize..32 {
            zipper.descend_to(i.to_be_bytes());
            assert_eq!(*zipper.get_value().unwrap(), i);
            zipper.reset();
        }
        drop(zipper);
    }

    #[test]
    fn write_zipper_get_or_insert_value_test() {
        let mut map = BytesTrieMap::<u64>::new();
        map.write_zipper_at_path(b"Drenths").get_value_or_insert(42);
        assert_eq!(map.get(b"Drenths"), Some(&42));

        *map.write_zipper_at_path(b"Drenths").get_value_or_insert(42) = 24;
        assert_eq!(map.get(b"Drenths"), Some(&24));

        let mut zipper = map.write_zipper_at_path(b"Drenths");
        *zipper.get_value_or_insert(42) = 0;
        assert_eq!(zipper.get_value(), Some(&0))
    }

    #[test]
    fn write_zipper_iter_copy_test() {
        const N: usize = 32;

        let mut map = BytesTrieMap::<usize>::new();
        let mut zipper = map.write_zipper_at_path(b"in\0");
        for i in 0..N {
            zipper.descend_to(i.to_be_bytes());
            zipper.set_value(i);
            zipper.reset();
        }
        drop(zipper);

        let zipper_head = map.zipper_head();
        {
            let mut sanity_counter = 0;
            let mut writer_z = unsafe{ zipper_head.write_zipper_at_exclusive_path_unchecked(b"out\0") };
            let mut reader_z = unsafe{ zipper_head.read_zipper_at_path_unchecked(b"in\0") };
            while let Some(val) = reader_z.to_next_val() {
                writer_z.descend_to(reader_z.path());
                writer_z.set_value(*val * 65536);
                writer_z.reset();
                sanity_counter += 1;
            }
            assert_eq!(sanity_counter, N);
        }
        drop(zipper_head);

        assert_eq!(map.val_count(), N*2);
        let mut in_path = b"in\0".to_vec();
        let mut out_path = b"out\0".to_vec();
        for i in 0..N {
            in_path.truncate(3);
            in_path.extend(i.to_be_bytes());
            assert_eq!(map.get(&in_path), Some(&i));
            out_path.truncate(4);
            out_path.extend(i.to_be_bytes());
            assert_eq!(map.get(&out_path), Some(&(i * 65536)));
        }
    }

    #[test]
    fn write_zipper_graft_test() {
        let a_keys = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        let mut a: BytesTrieMap<i32> = a_keys.iter().enumerate().map(|(i, k)| (k, i as i32)).collect();

        let b_keys = ["ad", "d", "ll", "of", "om", "ot", "ugh", "und"];
        let b: BytesTrieMap<i32> = b_keys.iter().enumerate().map(|(i, k)| (k, (i + 1000) as i32)).collect();

        let mut wz = a.write_zipper_at_path(b"ro");
        let rz = b.read_zipper();
        wz.graft(&rz);
        drop(wz);

        //Test that the original keys were left alone, above the graft point
        assert_eq!(a.get(b"arrow").unwrap(), &0);
        assert_eq!(a.get(b"bow").unwrap(), &1);
        assert_eq!(a.get(b"cannon").unwrap(), &2);

        //Test that the pruned keys are gone
        assert_eq!(a.get(b"roman"), None);
        assert_eq!(a.get(b"romulus"), None);
        assert_eq!(a.get(b"rom'i"), None);

        //More keys after but above the graft point weren't harmed
        assert_eq!(a.get(b"rubens").unwrap(), &7);
        assert_eq!(a.get(b"ruber").unwrap(), &8);
        assert_eq!(a.get(b"rubicundus").unwrap(), &10);

        //And test that the new keys were grafted into place
        assert_eq!(a.get(b"road").unwrap(), &1000);
        assert_eq!(a.get(b"rod").unwrap(), &1001);
        assert_eq!(a.get(b"roll").unwrap(), &1002);
        assert_eq!(a.get(b"roof").unwrap(), &1003);
        assert_eq!(a.get(b"room").unwrap(), &1004);
        assert_eq!(a.get(b"root").unwrap(), &1005);
        assert_eq!(a.get(b"rough").unwrap(), &1006);
        assert_eq!(a.get(b"round").unwrap(), &1007);
    }

    #[test]
    fn write_zipper_join_test() {
        let a_keys = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        let mut a: BytesTrieMap<u64> = a_keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        assert_eq!(a.val_count(), 12);

        let b_keys = ["road", "rod", "roll", "roof", "room", "root", "rough", "round"];
        let b: BytesTrieMap<u64> = b_keys.iter().enumerate().map(|(i, k)| (k, (i + 1000) as u64)).collect();
        assert_eq!(b.val_count(), 8);

        let mut wz = a.write_zipper_at_path(b"ro");
        let mut rz = b.read_zipper();
        rz.descend_to(b"ro");
        wz.join(&rz);
        drop(wz);

        //Test that the original keys were left alone, above the graft point
        assert_eq!(a.val_count(), 20);
        assert_eq!(a.get(b"arrow").unwrap(), &0);
        assert_eq!(a.get(b"bow").unwrap(), &1);
        assert_eq!(a.get(b"cannon").unwrap(), &2);

        //Test that the blended downstream keys are still there
        assert_eq!(a.get(b"roman").unwrap(), &3);
        assert_eq!(a.get(b"romulus").unwrap(), &6);
        assert_eq!(a.get(b"rom'i").unwrap(), &11);

        //More keys after but above the graft point weren't harmed
        assert_eq!(a.get(b"rubens").unwrap(), &7);
        assert_eq!(a.get(b"ruber").unwrap(), &8);
        assert_eq!(a.get(b"rubicundus").unwrap(), &10);

        //And test that the new keys were grafted into place
        assert_eq!(a.get(b"road").unwrap(), &1000);
        assert_eq!(a.get(b"rod").unwrap(), &1001);
        assert_eq!(a.get(b"roll").unwrap(), &1002);
        assert_eq!(a.get(b"roof").unwrap(), &1003);
        assert_eq!(a.get(b"room").unwrap(), &1004);
        assert_eq!(a.get(b"root").unwrap(), &1005);
        assert_eq!(a.get(b"rough").unwrap(), &1006);
        assert_eq!(a.get(b"round").unwrap(), &1007);
    }

    #[test]
    fn write_zipper_join_into_test() {
        let keys = ["a:arrow", "a:bow", "a:cannon", "a:roman", "a:romane", "a:romanus", "a:romulus", "a:rubens", "a:ruber", "a:rubicon", "a:rubicundus", "a:rom'i",
            "b:road", "b:rod", "b:roll", "b:roof", "b:room", "b:root", "b:rough", "b:round"];
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        assert_eq!(map.val_count(), 20);

        assert_eq!(map.val_count(), 20);
        assert_eq!(map.get(b"a:arrow").unwrap(), &0);
        assert_eq!(map.get(b"a:bow").unwrap(), &1);
        assert_eq!(map.get(b"a:cannon").unwrap(), &2);
        assert_eq!(map.get(b"a:roman").unwrap(), &3);
        assert_eq!(map.get(b"a:romulus").unwrap(), &6);
        assert_eq!(map.get(b"a:rom'i").unwrap(), &11);
        assert_eq!(map.get(b"a:rubens").unwrap(), &7);
        assert_eq!(map.get(b"a:ruber").unwrap(), &8);
        assert_eq!(map.get(b"a:rubicundus").unwrap(), &10);
        assert_eq!(map.get(b"b:road").unwrap(), &12);
        assert_eq!(map.get(b"b:rod").unwrap(), &13);
        assert_eq!(map.get(b"b:roll").unwrap(), &14);
        assert_eq!(map.get(b"b:roof").unwrap(), &15);
        assert_eq!(map.get(b"b:room").unwrap(), &16);
        assert_eq!(map.get(b"b:root").unwrap(), &17);
        assert_eq!(map.get(b"b:rough").unwrap(), &18);
        assert_eq!(map.get(b"b:round").unwrap(), &19);

        let head = map.zipper_head();
        let mut a = head.write_zipper_at_exclusive_path(b"a:").unwrap();
        let mut b = head.write_zipper_at_exclusive_path(b"b:").unwrap();
        assert_eq!(a.val_count(), 12);
        assert_eq!(b.val_count(), 8);

        a.join_into(&mut b);
        assert_eq!(a.val_count(), 20);
        assert_eq!(b.val_count(), 0);

        drop(a);
        drop(b);
        drop(head);

        //Test the keys are where we expect them to be, and not where they should not be
        assert_eq!(map.val_count(), 20);
        assert_eq!(map.get(b"a:arrow").unwrap(), &0);
        assert_eq!(map.get(b"a:bow").unwrap(), &1);
        assert_eq!(map.get(b"a:cannon").unwrap(), &2);
        assert_eq!(map.get(b"a:roman").unwrap(), &3);
        assert_eq!(map.get(b"a:romulus").unwrap(), &6);
        assert_eq!(map.get(b"a:rom'i").unwrap(), &11);
        assert_eq!(map.get(b"a:rubens").unwrap(), &7);
        assert_eq!(map.get(b"a:ruber").unwrap(), &8);
        assert_eq!(map.get(b"a:rubicundus").unwrap(), &10);
        assert_eq!(map.get(b"a:road").unwrap(), &12);
        assert_eq!(map.get(b"a:rod").unwrap(), &13);
        assert_eq!(map.get(b"a:roll").unwrap(), &14);
        assert_eq!(map.get(b"a:roof").unwrap(), &15);
        assert_eq!(map.get(b"a:room").unwrap(), &16);
        assert_eq!(map.get(b"a:root").unwrap(), &17);
        assert_eq!(map.get(b"a:rough").unwrap(), &18);
        assert_eq!(map.get(b"a:round").unwrap(), &19);

        assert_eq!(map.get(b"b:road"), None);
        assert_eq!(map.get(b"b:round"), None);
    }

    #[test]
    fn write_zipper_movement_test() {
        let keys = ["romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();

        let mut wz = map.write_zipper_at_path(b"ro");
        assert_eq!(wz.child_count(), 1);
        assert!(wz.descend_to(b"manus"));
        assert_eq!(wz.path(), b"manus");
        assert_eq!(wz.child_count(), 0);
        wz.reset();
        assert_eq!(wz.path(), b"");
        assert_eq!(wz.child_count(), 1);
        assert!(wz.descend_to(b"mulus"));
        assert_eq!(wz.path(), b"mulus");
        assert_eq!(wz.child_count(), 0);
        assert!(wz.ascend_until());
        assert_eq!(wz.path(), b"m");
        assert_eq!(wz.child_count(), 3);

        //Make sure we can't ascend above the zipper's root with ascend_until
        assert!(wz.ascend_until());
        assert_eq!(wz.path(), b"");
        assert!(!wz.ascend_until());

        //Test step-wise `ascend`
        wz.descend_to(b"manus");
        assert_eq!(wz.path(), b"manus");
        assert_eq!(wz.ascend(1), true);
        assert_eq!(wz.path(), b"manu");
        assert_eq!(wz.ascend(5), false);
        assert_eq!(wz.path(), b"");
        assert_eq!(wz.at_root(), true);
        wz.descend_to(b"mane");
        assert_eq!(wz.path(), b"mane");
        assert_eq!(wz.ascend(3), true);
        assert_eq!(wz.path(), b"m");
        assert_eq!(wz.child_count(), 3);
    }

    #[test]
    fn write_zipper_compound_join_test() {
        let mut map = BytesTrieMap::<u64>::new();

        let b_keys = ["alligator", "giraffe", "gazelle", "gadfly"];
        let b: BytesTrieMap<u64> = b_keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();

        let mut wz = map.write_zipper();
        let mut rz = b.read_zipper();
        rz.descend_to(b"alli");
        wz.graft(&rz);
        rz.reset();
        assert!(wz.join(&rz));
        drop(wz);

        assert_eq!(map.val_count(), 5);
        let values: Vec<String> = map.iter().map(|(path, _)| String::from_utf8_lossy(&path[..]).to_string()).collect();
        assert_eq!(values, vec!["alligator", "gadfly", "gator", "gazelle", "giraffe"]);
    }

    #[test]
    fn write_zipper_remove_branches_test() {
        let keys = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i",
            "abcdefghijklmnopqrstuvwxyz"];
        let mut map: BytesTrieMap<i32> = keys.iter().enumerate().map(|(i, k)| (k, i as i32)).collect();

        let mut wz = map.write_zipper_at_path(b"roman");
        wz.remove_branches();
        drop(wz);

        //Test that the original keys were left alone, above the graft point
        assert_eq!(map.get(b"arrow").unwrap(), &0);
        assert_eq!(map.get(b"bow").unwrap(), &1);
        assert_eq!(map.get(b"cannon").unwrap(), &2);
        assert_eq!(map.get(b"rom'i").unwrap(), &11);

        //Test that the value is ok
        assert_eq!(map.get(b"roman").unwrap(), &3);

        //Test that the pruned keys are gone
        assert_eq!(map.get(b"romane"), None);
        assert_eq!(map.get(b"romanus"), None);

        let mut wz = map.write_zipper();
        wz.descend_to(b"ro");
        assert!(wz.path_exists());
        wz.remove_branches();
        assert!(!wz.path_exists());
        drop(wz);

        let mut wz = map.write_zipper();
        wz.descend_to(b"abcdefghijklmnopq");
        assert!(wz.path_exists());
        assert_eq!(wz.path(), b"abcdefghijklmnopq");
        wz.remove_branches();
        assert!(!wz.path_exists());
        assert_eq!(wz.path(), b"abcdefghijklmnopq");
        drop(wz);

        assert!(!map.contains_path(b"abcdefghijklmnopq"));
        assert!(!map.contains_path(b"abc"));
    }

    #[test]
    fn write_zipper_drop_head_test1() {
        let keys = [
            "123:abc:Bob",
            "123:def:Jim",
            "123:ghi:Pam",
            "123:jkl:Sue",
            "123:dog:Bob:Fido",
            "123:cat:Jim:Felix",
            "123:dog:Pam:Bandit",
            "123:owl:Sue:Cornelius"];
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut wz = map.write_zipper_at_path(b"123:");

        wz.drop_head(4);
        drop(wz);

        let ref_keys: Vec<&[u8]> = vec![
            b"123:Bob",
            b"123:Bob:Fido",
            b"123:Jim",
            b"123:Jim:Felix",
            b"123:Pam",
            b"123:Pam:Bandit",
            b"123:Sue",
            b"123:Sue:Cornelius"];
        assert_eq!(map.iter().map(|(k, _v)| k).collect::<Vec<Vec<u8>>>(), ref_keys);
    }

    #[test]
    fn write_zipper_drop_head_long_key_test() {

        //A single long key
        let key = b"12345678901234567890123456789012345678901234567890";
        let mut map: BytesTrieMap<u64> = BytesTrieMap::<u64>::new();
        map.insert(key, 42);
        for i in 0..key.len() {
            assert_eq!(map.get(&key[i..]), Some(&42));
            let mut wz = map.write_zipper();
            wz.drop_head(1);
        }

        //A slightly more complicated tree
        let keys: Vec<&[u8]> = vec![
            b"12345678901234567890123456789012345678901234567890",
            b"12345ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrs",
            b"1234567890FGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrs",
            b"123456789012345KLMNOPQRSTUVWXYZabcdefghijklmnopqrs",
            b"12345678901234567890PQRSTUVWXYZabcdefghijklmnopqrs",
            b"1234567890123456789012345UVWXYZabcdefghijklmnopqrs",
            b"123456789012345678901234567890Zabcdefghijklmnopqrs",
            b"12345678901234567890123456789012345efghijklmnopqrs",
            b"1234567890123456789012345678901234567890jklmnopqrs",
            b"123456789012345678901234567890123456789012345opqrs", ];
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        for i in 0..keys[0].len() {
            assert_eq!(map.get(&keys[0][i..]), Some(&0));
            if i < 45 {
                assert_eq!(map.get(&keys[9][i..]), Some(&9));
            }
            if i > 10 {
                assert_eq!(map.val_count(), 11-(i/5));
            }
            let mut wz = map.write_zipper();
            wz.drop_head(1);
        }
    }

    #[test]
    fn write_zipper_drop_head_test2() {
        let keys: Vec<Vec<u8>> = vec![
            vec![1, 2, 4, 65, 2, 42, 237, 3, 1, 173, 165, 3, 16, 200, 213, 4, 0, 166, 47, 81, 4, 0, 167, 216, 181, 4, 6, 125, 178, 225, 4, 6, 142, 119, 117, 4, 64, 232, 214, 129, 4, 65, 128, 13, 13, 4, 65, 144],
            vec![1, 2, 4, 69, 2, 13, 183],
        ];
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut wz = map.write_zipper_at_path(&[1]);
        wz.drop_head(3);
        drop(wz);

        assert_eq!(map.get(&vec![1, 2, 42, 237, 3, 1, 173, 165, 3, 16, 200, 213, 4, 0, 166, 47, 81, 4, 0, 167, 216, 181, 4, 6, 125, 178, 225, 4, 6, 142, 119, 117, 4, 64, 232, 214, 129, 4, 65, 128, 13, 13, 4, 65, 144]), Some(&0));
        assert_eq!(map.get(&vec![1, 2, 13, 183]), Some(&1));
        assert_eq!(map.val_count(), 2);

        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut wz = map.write_zipper_at_path(&[1]);
        wz.drop_head(27);
        drop(wz);

        assert_eq!(map.get(&vec![1, 178, 225, 4, 6, 142, 119, 117, 4, 64, 232, 214, 129, 4, 65, 128, 13, 13, 4, 65, 144]), Some(&0));
        assert_eq!(map.val_count(), 1);
    }

    #[test]
    fn write_zipper_insert_prefix_test() {
        let keys = [
            "123:Bob:Fido",
            "123:Jim:Felix",
            "123:Pam:Bandit",
            "123:Sue:Cornelius"];
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut wz = map.write_zipper_at_path(b"123:");

        wz.insert_prefix(b"pet:");
        drop(wz);

        // let paths: Vec<String> = map.iter().map(|(k, _)| String::from_utf8_lossy(&k[..]).to_string()).collect();
        let ref_keys: Vec<&[u8]> = vec![
            b"123:pet:Bob:Fido",
            b"123:pet:Jim:Felix",
            b"123:pet:Pam:Bandit",
            b"123:pet:Sue:Cornelius"];
        assert_eq!(map.iter().map(|(k, _v)| k).collect::<Vec<Vec<u8>>>(), ref_keys);

        // Test that drop_head undoes insert_prefix
        let mut wz = map.write_zipper();
        wz.insert_prefix(b"people:");
        //let paths: Vec<String> = map.iter().map(|(k, _)| String::from_utf8_lossy(&k[..]).to_string()).collect();
        wz.drop_head(b"people:".len());
        drop(wz);

        assert_eq!(map.iter().map(|(k, _v)| k).collect::<Vec<Vec<u8>>>(), ref_keys);
    }

    #[test]
    fn write_zipper_remove_prefix_test() {
        let keys = [
            "123:Bob.Fido",
            "123:Jim.Felix",
            "123:Pam.Bandit",
            "123:Sue.Cornelius"];

        //Test where we don't bottom-out the zipper
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut wz = map.write_zipper_at_path(b"123");

        wz.descend_to(b":Pam");
        assert_eq!(wz.remove_prefix(4), true);
        drop(wz);

        assert_eq!(map.val_count(), 1);
        assert_eq!(map.get(b"123.Bandit"), Some(&2));

        //Test where we *do* exactly bottom-out the zipper
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut wz = map.write_zipper_at_path(b"123:");

        wz.descend_to(b"Pam.");
        assert_eq!(wz.remove_prefix(4), true);
        drop(wz);

        assert_eq!(map.val_count(), 1);
        assert_eq!(map.get(b"123:Bandit"), Some(&2));

        //Now test where we crash into the bottom of the zipper
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut wz = map.write_zipper_at_path(b"123:");

        wz.descend_to(b"Pam.");
        assert_eq!(wz.remove_prefix(9), false);
        drop(wz);

        assert_eq!(map.val_count(), 1);
        assert_eq!(map.get(b"123:Bandit"), Some(&2));
    }

    #[test]
    fn write_zipper_map_test() {
        let keys = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();

        let mut wr = map.write_zipper();
        wr.descend_to(b"rom");
        let sub_map = wr.take_map().unwrap();
        drop(wr);

        let sub_map_keys: Vec<String> = sub_map.iter().map(|(k, _v)| String::from_utf8_lossy(&k).to_string()).collect();
        assert_eq!(sub_map_keys, ["'i", "an", "ane", "anus", "ulus"]);
        let map_keys: Vec<String> = map.iter().map(|(k, _v)| String::from_utf8_lossy(&k).to_string()).collect();
        assert_eq!(map_keys, ["arrow", "bow", "cannon", "rubens", "ruber", "rubicon", "rubicundus"]);

        let mut wr = map.write_zipper();
        wr.descend_to(b"c");
        wr.join_map(sub_map);
        drop(wr);

        let map_keys: Vec<String> = map.iter().map(|(k, _v)| String::from_utf8_lossy(&k).to_string()).collect();
        assert_eq!(map_keys, ["arrow", "bow", "c'i", "can", "cane", "cannon", "canus", "culus", "rubens", "ruber", "rubicon", "rubicundus"]);
    }

    #[test]
    fn write_zipper_mask_children_and_values() {
        let keys = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i",
            "abcdefghijklmnopqrstuvwxyz"];
        let mut map: BytesTrieMap<i32> = keys.iter().enumerate().map(|(i, k)| (k, i as i32)).collect();

        let mut wr = map.write_zipper();

        let mut m = [0, 0, 0, 0];
        for b in "abc".bytes() { m[((b & 0b11000000) >> 6) as usize] |= 1u64 << (b & 0b00111111); }
        wr.remove_unmasked_branches(m);
        drop(wr);

        let result = map.iter().map(|(k, _v)| String::from_utf8_lossy(&k).to_string()).collect::<Vec<_>>();

        assert_eq!(result, ["abcdefghijklmnopqrstuvwxyz", "arrow", "bow", "cannon"]);
    }

    #[test]
    fn write_zipper_mask_children_and_values_at_path() {
        let keys = [
            "123:abc:Bob",
            "123:def:Jim",
            "123:ghi:Pam",
            "123:jkl:Sue",
            "123:dog:Bob:Fido",
            "123:cat:Jim:Felix",
            "123:dog:Pam:Bandit",
            "123:owl:Sue:Cornelius"];
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();

        let mut wr = map.write_zipper();
        wr.descend_to("123:".as_bytes());
        // println!("{:?}", wr.child_mask());

        let mut m = [0, 0, 0, 0];
        for b in "dco".bytes() { m[((b & 0b11000000) >> 6) as usize] |= 1u64 << (b & 0b00111111); }
        wr.remove_unmasked_branches(m);
        m = [0, 0, 0, 0];
        wr.descend_to("d".as_bytes());
        for b in "o".bytes() { m[((b & 0b11000000) >> 6) as usize] |= 1u64 << (b & 0b00111111); }
        wr.remove_unmasked_branches(m);
        drop(wr);

        let result = map.iter().map(|(k, _v)| String::from_utf8_lossy(&k).to_string()).collect::<Vec<_>>();
        assert_eq!(result, [
            "123:cat:Jim:Felix",
            "123:dog:Bob:Fido",
            "123:dog:Pam:Bandit",
            "123:owl:Sue:Cornelius"]);

        let keys = [
            "a1",
            "a2",
            "a1a",
            "a1b",
            "a1a1",
            "a1a2",
            "a1a1a",
            "a1a1b"];
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut wr = map.write_zipper_at_path(b"a1");
        // println!("{:?}", wr.child_mask());

        m = [0, 0, 0, 0];
        for b in "b".bytes() { m[((b & 0b11000000) >> 6) as usize] |= 1u64 << (b & 0b00111111); }
        wr.remove_unmasked_branches(m);
        drop(wr);

        let result = map.iter().map(|(k, _v)| String::from_utf8_lossy(&k).to_string()).collect::<Vec<_>>();
        assert_eq!(result, [
            "a1",
            "a1b",
            "a2"]);
    }

    #[test]
    fn write_zipper_remove_unmask_branches() {
        let keys = ["Wilson", "Taft", "Roosevelt", "McKinley", "Cleveland", "Harrison", "Arthur", "Garfield"];
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();

        let mut wr = map.write_zipper();
        wr.remove_unmasked_branches([0xFF, !(1<<(b'M'-64)), 0xFF, 0xFF]);
        //McKinley didn't make it
        wr.descend_to("McKinley");
        assert_eq!(wr.get_value(), None);

        wr.reset();
        wr.descend_to("Roos");
        assert_eq!(wr.path_exists(), true);
        wr.remove_unmasked_branches([0xFF, !(1<<(b'i'-64)), 0xFF, 0xFF]);
        //Missed Roosevelt
        wr.descend_to("evelt");
        assert_eq!(wr.get_value(), Some(&2));

        wr.reset();
        wr.descend_to("Garf");
        assert_eq!(wr.path_exists(), true);
        wr.remove_unmasked_branches([0xFF, !(1<<(b'i'-64)), 0xFF, 0xFF]);
        wr.descend_to("ield");
        //Garfield was removed
        assert_eq!(wr.get_value(), None);
    }
    #[test]
    fn write_zipper_test_zipper_conversion() {
        let keys = [
            "123:dog:Bob:Fido",
            "123:cat:Jim:Felix",
            "123:dog:Pam:Bandit",
            "123:owl:Sue:Cornelius"];
        let mut map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();

        // Simplistic test where the WZ is untracker, created with a statically safe method
        let mut wz = map.write_zipper_at_path(b"12");
        assert_eq!(wz.path(), b"");
        wz.descend_to(b"3:");
        assert_eq!(wz.path(), b"3:");

        let mut rz = wz.into_read_zipper();
        assert_eq!(rz.path(), b"3:");
        rz.reset();
        assert_eq!(rz.descend_to(b"3:dog:"), true);
        assert_eq!(rz.child_count(), 2);
        drop(rz);

        // ZipperHead test, to make sure the tracker is doing the right thing when converted
        let zh = map.zipper_head();
        let mut wz = zh.write_zipper_at_exclusive_path(b"12").unwrap();
        assert_eq!(wz.path(), b"");
        wz.descend_to(b"3:");
        assert_eq!(wz.path(), b"3:");

        let mut rz = wz.into_read_zipper();
        assert_eq!(rz.path(), b"3:");
        rz.reset();
        assert_eq!(rz.descend_to(b"3:dog:"), true);
        assert_eq!(rz.child_count(), 2);

        assert!(zh.write_zipper_at_exclusive_path(b"1").is_err());
        assert!(zh.write_zipper_at_exclusive_path(b"12").is_err());
        assert!(zh.write_zipper_at_exclusive_path(b"123").is_err());

        let mut rz2 = zh.read_zipper_at_borrowed_path(b"1").unwrap();
        assert_eq!(rz2.path(), b"");
        assert_eq!(rz2.descend_to(b"23:dog:"), true);
        assert_eq!(rz.child_count(), 2);

        let rz3 = zh.read_zipper_at_borrowed_path(b"123:").unwrap();
        assert_eq!(rz3.child_count(), 3);
    }

}
