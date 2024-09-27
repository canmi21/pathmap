
//! # Zipper Usage
//!
//! A zipper represents a cursor in a trie, and has a location called the focus.  A zipper can be moved
//! within the trie in order to access the trie for reading and/or writing.  A zipper's focus may not be
//! moved above the zipper's root.
//!
//! ## Move by Absolute Distance or by Trie Features
//!
//! Zippers may be moved either by stepping an absolute number of elements, or by jumping to features
//! such as forks and values.  In general, moving by jumping will be faster because it can skip over
//! multiple key bytes with one jump.
//!
//! The stepping methods are:
//! - [descend_indexed_branch](zipper::Zipper::descend_indexed_branch)
//! - [ascend](zipper::Zipper::ascend)
//! - [to_sibling](zipper::Zipper::to_sibling)
//!
//! The jumping methods are:
//! - [descend_to](zipper::Zipper::descend_to)
//! - [descend_until](zipper::Zipper::descend_until)
//! - [ascend_until](zipper::Zipper::ascend_until)
//!

use crate::trie_node::*;
use crate::trie_map::BytesTrieMap;

pub use crate::write_zipper::*;
use crate::zipper_tracking::*;

/// An interface common to all zippers, to support moving the zipper, reading elements, iterating across
/// the trie
pub trait Zipper<'a>: zipper_priv::ZipperPriv {

    /// Returns `true` if the zipper cannot ascend further, otherwise returns `false`
    fn at_root(&self) -> bool;

    /// Resets the zipper's focus back to the root
    fn reset(&mut self);

    /// Returns the path from the zipper's root to the current focus
    fn path(&self) -> &[u8];

    /// Returns `true` if the zipper's focus is on a path within the trie, otherwise `false`
    fn path_exists(&self) -> bool;

    /// Returns `true` if there is a value at the zipper's focus, otherwise `false`
    fn is_value(&self) -> bool;

    /// Returns the total number of values contained at and below the zipper's focus, including the focus itself
    ///
    /// WARNING: This is not a cheap method. It may have an order-N cost
    fn val_count(&self) -> usize;

    /// Returns the number of child branches from the focus node
    ///
    /// Returns 0 if the focus is on a leaf
    fn child_count(&self) -> usize;

    /// Returns 256-bit mask indicating which children exist from the branch at the zipper's focus
    ///
    /// Returns an empty mask if the focus is on a leaf or non-existent path
    fn child_mask(&self) -> [u64; 4];

    /// Moves the zipper deeper into the tree, to the `key` specified relative to the current zipper focus
    ///
    /// Returns `true` if the zipper points to an existing path within the tree, otherwise `false`.  The
    /// zipper's location will be updated, regardless of whether or not the path exists within the tree.
    fn descend_to<K: AsRef<[u8]>>(&mut self, k: K) -> bool;

    /// Descends the zipper's focus one step into a child branch uniquely identified by `child_idx`
    ///
    /// `child_idx` must within the range `0..child_count()` or this method will do nothing and return `false`
    ///
    /// WARNING: The branch represented by a given index is not guaranteed to be stable across modifications
    /// to the trie.  This method should only be used as part of a directed traversal operation, but
    /// index-based paths may not be stored as locations within the trie.
    fn descend_indexed_branch(&mut self, idx: usize) -> bool;

    /// Descends the zipper's focus until a branch or a value is encountered.  Returns `true` if the focus
    /// moved otherwise returns `false`
    fn descend_until(&mut self) -> bool;

    /// Ascends the zipper `steps` steps.  Returns `true` if the zipper sucessfully moved `steps`
    ///
    /// If the root is fewer than `n` steps from the zipper's position, then this method will stop at
    /// the root and return `false`
    fn ascend(&mut self, steps: usize) -> bool;

    /// Ascends the zipper to the nearest upstream branch point or value.  Returns `true` if the zipper
    /// focus moved upwards, otherwise returns `false` if the zipper was already at the root
    fn ascend_until(&mut self) -> bool;

    /// Moves the zipper's focus to a sibling at the same level.  Returns `true` if the focus was changed,
    /// otherwise returns `false`
    ///
    /// This method is equivalent to calling [Self::ascend] with `1`, followed by [Self::descend_indexed_branch]
    /// where the index passed is 1 more or less than the index of the current focus position.
    ///
    /// If `next` is `true` then the zipper will be advanced otherwise it will be moved backwards.
    fn to_sibling(&mut self, next: bool) -> bool;

    /// Returns a new read-only Zipper, with the new zipper's root being at the zipper's current focus
    fn fork_zipper(&self) -> ReadZipper<Self::V>;

    /// Returns a new [BytesTrieMap] containing everything below the zipper's focus
    fn make_map(&self) -> Option<BytesTrieMap<Self::V>>;

}

pub(crate) mod zipper_priv {
    use crate::trie_node::*;

    pub trait ZipperPriv {
        type V;

        fn get_focus(&self) -> AbstractNodeRef<Self::V>;
    }
}
use zipper_priv::*;

/// Size of node stack to preallocate in the zipper
pub(crate) const EXPECTED_DEPTH: usize = 16;

/// Size in bytes to preallocate path storage in the zipper
pub(crate) const EXPECTED_PATH_LEN: usize = 64;

/// A [Zipper] that is unable to modify the trie
pub struct ReadZipper<'a, 'k, V> {
    /// A reference to the entire origin path, of which `root_key` is the final subset, or None for a relative zipper
    origin_path: &'k [u8],
    /// The byte offset in `origin_path` from the root node to the zipper's root.
    /// `root_key = origin_path[root_key_offset..]`
    root_key_offset: Option<usize>,
    /// A special-case to access a value at the root node, because that value would be otherwise inaccessible
    root_val: Option<&'a V>,
    /// A reference to the root node
    focus_node: TaggedNodeRef<'a, V>,
    /// An iter token corresponding to the location of the `node_key` within the `focus_node`, or NODE_ITER_INVALID
    /// if iteration is not in-process
    focus_iter_token: u128,
    /// Stores the entire path from the root node, including the bytes from `root_key`
    prefix_buf: Vec<u8>,
    /// Stores a stack of parent node references.  Does not include the focus_node
    /// The tuple contains: `(node_ref, iter_token, key_offset_in_prefix_buf)`
    ancestors: Vec<(TaggedNodeRef<'a, V>, u128, usize)>,
    /// Tracks the outstanding zippers to ensure there are no conflicts
    zipper_tracker: ZipperTracker,
}

impl<V> Drop for ReadZipper<'_, '_, V> {
    fn drop(&mut self) { }
}

impl<'a, 'k, V> Clone for ReadZipper<'a, 'k, V> where V: Clone {
    fn clone(&self) -> Self {
        Self {
            origin_path: self.origin_path,
            root_key_offset: self.root_key_offset,
            root_val: self.root_val.clone(),
            focus_node: self.focus_node.clone(),
            focus_iter_token: NODE_ITER_INVALID,
            prefix_buf: self.prefix_buf.clone(),
            ancestors: self.ancestors.clone(),
            zipper_tracker: self.zipper_tracker.new_read_path(self.origin_path),
        }
    }
}

impl<'a, 'k, V: Clone> Zipper<'a> for ReadZipper<'a, 'k, V> {

    fn at_root(&self) -> bool {
        self.prefix_buf.len() <= self.origin_path.len()
    }

    fn reset(&mut self) {
        self.ancestors.truncate(1);
        match self.ancestors.pop() {
            Some((node, _tok, _prefix_len)) => {
                self.focus_node = node;
                self.focus_iter_token = NODE_ITER_INVALID;
            },
            None => {}
        }
        self.prefix_buf.truncate(self.origin_path.len());
    }

    #[inline]
    fn path(&self) -> &[u8] {
        if self.prefix_buf.len() > 0 {
            &self.prefix_buf[self.origin_path.len()..]
        } else {
            &[]
        }
    }

    fn path_exists(&self) -> bool {
        let key = self.node_key();
        if key.len() > 0 {
            self.focus_node.node_contains_partial_key(key)
        } else {
            true
        }
    }

    fn is_value(&self) -> bool {
        self.is_value_internal()
    }

    fn val_count(&self) -> usize {
        if self.node_key().len() == 0 {
            val_count_below_root(self.focus_node.borrow()) + (self.is_value() as usize)
        } else {
            let focus = self.get_focus();
            if focus.is_none() {
                0
            } else {
                val_count_below_root(focus.borrow()) + (self.is_value() as usize)
            }
        }
    }

    fn child_count(&self) -> usize {
        debug_assert!(self.is_regularized());
        self.focus_node.count_branches(self.node_key())
    }

    fn child_mask(&self) -> [u64; 4] {
        debug_assert!(self.is_regularized());
        self.focus_node.node_branches_mask(self.node_key())
    }

    fn descend_to<K: AsRef<[u8]>>(&mut self, k: K) -> bool {
        self.prepare_buffers();
        debug_assert!(self.is_regularized());

        self.prefix_buf.extend(k.as_ref());
        let mut key_start = self.node_key_start();
        let mut key = &self.prefix_buf[key_start..];

        //Step until we get to the end of the key or find a leaf node
        while let Some((consumed_byte_cnt, next_node)) = self.focus_node.node_get_child(key) {
            key_start += consumed_byte_cnt;
            self.ancestors.push((self.focus_node.clone(), self.focus_iter_token, key_start));
            self.focus_node = next_node.as_tagged();
            self.focus_iter_token = NODE_ITER_INVALID;
            if consumed_byte_cnt < key.len() {
                key = &key[consumed_byte_cnt..]
            } else {
                return true;
            };
        }
        self.focus_node.node_contains_partial_key(key)
    }

    fn descend_indexed_branch(&mut self, child_idx: usize) -> bool {
        self.prepare_buffers();
        debug_assert!(self.is_regularized());

        match self.focus_node.nth_child_from_key(self.node_key(), child_idx) {
            (Some(prefix), Some(child_node)) => {
                self.prefix_buf.push(prefix);
                self.ancestors.push((self.focus_node.clone(), self.focus_iter_token, self.prefix_buf.len()));
                self.focus_node = child_node.as_tagged();
                self.focus_iter_token = NODE_ITER_INVALID;
                true
            },
            (Some(prefix), None) => {
                self.prefix_buf.push(prefix);
                true
            },
            (None, _) => false
        }
    }

    fn descend_until(&mut self) -> bool {
        debug_assert!(self.is_regularized());
        let mut moved = false;
        while self.child_count() == 1 {
            moved = true;
            self.descend_first();
            if self.is_value_internal() {
                break;
            }
        }
        moved
    }

    fn to_sibling(&mut self, next: bool) -> bool {
        self.prepare_buffers();
        if self.node_key().len() != 0 {
            match self.focus_node.get_sibling_of_child(self.node_key(), next) {
                (Some(prefix), Some(child_node)) => {
                    *self.prefix_buf.last_mut().unwrap() = prefix;
                    self.ancestors.push((self.focus_node.clone(), self.focus_iter_token, self.prefix_buf.len()));
                    self.focus_node = child_node.as_tagged();
                    self.focus_iter_token = NODE_ITER_INVALID;
                    true
                },
                (Some(prefix), None) => {
                    *self.prefix_buf.last_mut().unwrap() = prefix;
                    true
                },
                (None, _) => false
            }
        } else {
            let mut should_pop = false;
            let result = match self.ancestors.last() {
                None => { false }
                Some((parent, _iter_tok, _prefix_offset)) => {
                    match parent.get_sibling_of_child(self.parent_key(), next) {
                        (Some(prefix), Some(child_node)) => {
                            *self.prefix_buf.last_mut().unwrap() = prefix;
                            self.focus_node = child_node.as_tagged();
                            self.focus_iter_token = NODE_ITER_INVALID;
                            true
                        },
                        (Some(prefix), None) => {
                            *self.prefix_buf.last_mut().unwrap() = prefix;
                            should_pop = true;
                            true
                        },
                        (None, _) => {
                            false
                        }
                    }
                }
            };
            if should_pop {
                let (focus_node, iter_tok, _prefix_offset) = self.ancestors.pop().unwrap();
                self.focus_node = focus_node;
                self.focus_iter_token = iter_tok;
            }
            result
        }
    }

    fn ascend(&mut self, mut steps: usize) -> bool {
        while steps > 0 {
            if self.excess_key_len() == 0 {
                match self.ancestors.pop() {
                    Some((node, iter_tok, _prefix_offset)) => {
                        self.focus_node = node;
                        self.focus_iter_token = iter_tok;
                    },
                    None => return false
                };
            }
            let cur_jump = steps.min(self.excess_key_len());
            self.prefix_buf.truncate(self.prefix_buf.len() - cur_jump);
            steps -= cur_jump;
        }
        true
    }

    fn ascend_until(&mut self) -> bool {
        if self.at_root() {
            return false;
        }

        //See if the branch point is in the parent node
        if self.node_key().len() == 0 {
            self.ascend_across_nodes();
        }
        self.ascend_within_node();
        if self.child_count() == 1 {
            self.ascend_until();
        }
        true
    }

    fn fork_zipper(&self) -> ReadZipper<V> {
        let new_root_val = self.get_value();
        let (new_root_path, new_root_key_offset) = match &self.root_key_offset {
            Some(_) => {
                let new_root_path = self.origin_path().unwrap();
                let new_root_key_offset = new_root_path.len() - self.node_key().len();
                (new_root_path, Some(new_root_key_offset))
            },
            None => (self.origin_path, None)
        };
        let zipper_tracker = self.zipper_tracker.new_read_path(&self.origin_path);
        ReadZipper::new_with_node_and_path_internal(self.focus_node.clone(), new_root_path, new_root_key_offset, new_root_val, zipper_tracker)
    }

    fn make_map(&self) -> Option<BytesTrieMap<Self::V>> {
        self.get_focus().into_option().map(|node| BytesTrieMap::new_with_root(node))
    }
}

impl<'a, 'k, V : Clone> zipper_priv::ZipperPriv for ReadZipper<'a, 'k, V> {
    type V = V;

    fn get_focus(&self) -> AbstractNodeRef<Self::V> {
        self.focus_node.get_node_at_key(self.node_key())
    }
}

impl<'a, 'k, V : Clone> ReadZipper<'a, 'k, V> {

    /// Creates a new zipper, with a path relative to a node
    pub(crate) fn new_with_node_and_path(root_node: &'a dyn TrieNode<V>, path: &'k [u8], mut root_key_offset: Option<usize>, zipper_tracker: ZipperTracker) -> Self {
        let mut key = path;
        let mut node = root_node;
        let mut val = None;

        //Step until we get to the end of the key or find a leaf node
        if key.len() > 0 {
            while let Some((consumed_byte_cnt, next_node)) = node.node_get_child(key) {
                if consumed_byte_cnt < key.len() {
                    node = next_node;
                    key = &key[consumed_byte_cnt..];
                } else {
                    val = node.node_get_val(key);
                    node = next_node;
                    key = &[];
                    break;
                };
            }
        }

        let zipper_path = match root_key_offset.as_mut() {
            Some(root_key_offset) => {
                *root_key_offset -= key.len();
                path
            },
            None => key
        };

        Self::new_with_node_and_path_internal(node.as_tagged(), zipper_path, root_key_offset, val, zipper_tracker)
    }
    /// Creates a new zipper, with a path relative to a node, assuming the path is fully-contained within
    /// the node
    ///
    /// NOTE: This method currently doesn't descend subnodes.  Use [Self::new_with_node_and_path] if you can't
    /// guarantee the path is within the supplied node.
    pub(crate) fn new_with_node_and_path_internal(root_node: TaggedNodeRef<'a, V>, path: &'k [u8], root_key_offset: Option<usize>, root_val: Option<&'a V>, zipper_tracker: ZipperTracker) -> Self {
        Self {
            origin_path: path,
            root_key_offset,
            root_val,
            focus_node: root_node,
            focus_iter_token: NODE_ITER_INVALID,
            prefix_buf: vec![],
            ancestors: vec![],
            zipper_tracker,
        }
    }

    /// Returns the length of the `self.path()`, saving a couple instructions, but is internal because it may panic
    #[inline(always)]
    fn path_len(&self) -> usize {
        self.prefix_buf.len() - self.origin_path.len()
    }

    /// Returns a refernce to the value at the zipper's focus, or `None` if there is no value
    pub fn get_value(&self) -> Option<&'a V> {
        let key = self.node_key();
        if key.len() > 0 {
            self.focus_node.node_get_val(key)
        } else {
            if let Some((parent, _iter_tok, _prefix_offset)) = self.ancestors.last() {
                parent.node_get_val(self.parent_key())
            } else {
                self.root_val.clone() //Just clone the ref, not the value itself
            }
        }
    }

    /// Returns the path beginning from the origin to the current focus.  Returns `None` if the zipper
    /// is relative and does not have an origin path
    pub fn origin_path(&self) -> Option<&[u8]> {
        if self.root_key_offset.is_some() {
            if self.prefix_buf.len() > 0 {
                Some(&self.prefix_buf)
            } else {
                Some(&self.origin_path)
            }
        } else {
            None
        }
    }

    /// Ensures the zipper is in its regularized form
    ///
    /// Q: What the heck is "regularized form"?!?!?!
    /// A: The same zipper position may be representated with multiple configurations of the zipper's
    ///  field variables.  Consider the path: `abcd`, where the zipper points to `c`.  This could be
    ///  represented with the `focus_node` of `c` and a `node_key()` of `[]`; called the zipper's
    ///  regularized form.  Alternatively it could be represented with the `focus_node` of `b` and a
    ///  `node_key()` of `c`, which is called a deregularized form.
    fn regularize(&mut self) {
        debug_assert!(self.prefix_buf.len() > self.node_key_start()); //If this triggers, we have uninitialized buffers
        if let Some((_consumed_byte_cnt, next_node)) = self.focus_node.node_get_child(self.node_key()) {
            self.ancestors.push((self.focus_node.clone(), self.focus_iter_token, self.prefix_buf.len()));
            self.focus_node = next_node.as_tagged();
            self.focus_iter_token = self.focus_node.new_iter_token();
        }
    }

    /// Ensures the zipper is in a deregularized form.  See docs for [Self::regularize]
    #[inline]
    fn deregularize(&mut self) {
        if self.prefix_buf.len() == self.node_key_start() {
            self.ascend_across_nodes();
        }
    }

    /// Returns `true` if the zipper is in a regularized form, otherwise returns the `false`
    ///
    /// See docs for [Self::regularize].
    #[inline]
    fn is_regularized(&self) -> bool {
        let key_start = self.node_key_start();
        if self.prefix_buf.len() > key_start {
            self.focus_node.node_get_child(self.node_key()).is_none()
        } else {
            true
        }
    }

    /// Identical in effect to [descend_to](Self::descend_to) with a 1-byte key argument
    pub fn descend_to_byte(&mut self, k: u8) -> bool {
        self.prepare_buffers();
        debug_assert!(self.is_regularized());

        self.prefix_buf.push(k);
        if let Some((_consumed_byte_cnt, next_node)) = self.focus_node.node_get_child(self.node_key()) {
            self.ancestors.push((self.focus_node.clone(), self.focus_iter_token, self.prefix_buf.len()));
            self.focus_node = next_node.as_tagged();
            self.focus_iter_token = NODE_ITER_INVALID;
            return true;
        }
        self.focus_node.node_contains_partial_key(self.node_key())
    }

    /// Ascends the zipper up a single byte.  Equivalent to [`ascend(1)`](Self::ascend)
    pub fn ascend_byte(&mut self) -> bool {
        if self.excess_key_len() == 0 {
            match self.ancestors.pop() {
                Some((node, iter_tok, _prefix_offset)) => {
                    self.focus_node = node;
                    self.focus_iter_token = iter_tok;
                },
                None => return false
            };
        }
        self.prefix_buf.pop();
        true
    }

    /// Systematically advances to the next value accessible from the zipper, traversing in a depth-first
    /// order
    ///
    /// Returns a reference to the value, or `None` if the focus has reached the root.
    pub fn to_next_val(&mut self) -> Option<&'a V> {
        self.prepare_buffers();
        loop {
            if self.focus_iter_token == NODE_ITER_INVALID {
                let (cur_tok, _full_key) = self.focus_node.iter_token_for_path(self.node_key());
                self.focus_iter_token = cur_tok;
            }

            let (new_tok, key_bytes, child_node, value) = if self.focus_iter_token != NODE_ITER_FINISHED {
                self.focus_node.next_items(self.focus_iter_token)
            } else {
                (NODE_ITER_FINISHED, &[] as &[u8], None, None)
            };

            if new_tok != NODE_ITER_FINISHED {
                self.focus_iter_token = new_tok;

                let key_start = self.node_key_start();
                self.prefix_buf.truncate(key_start);
                self.prefix_buf.extend(key_bytes);

                match child_node {
                    None => {},
                    Some(rec) => {
                        self.ancestors.push((self.focus_node.clone(), new_tok, self.prefix_buf.len()));
                        self.focus_node = rec.borrow().as_tagged();
                        self.focus_iter_token = self.focus_node.new_iter_token();
                    },
                }

                match value {
                    Some(v) => return Some(v),
                    None => {}
                }
            } else {
                //Ascend
                if let Some((focus_node, iter_tok, prefix_offset)) = self.ancestors.pop() {
                    self.focus_node = focus_node;
                    self.focus_iter_token = iter_tok;
                    self.prefix_buf.truncate(prefix_offset);
                } else {
                    let new_len = self.origin_path.len().max(self.root_key_offset.unwrap_or(0));
                    self.focus_iter_token = NODE_ITER_INVALID;
                    self.prefix_buf.truncate(new_len);
                    return None
                }
            }
        }
    }

    /// Advances the zipper, one byte at a time, to visit every existing path within the trie in a 
    /// depth-first order
    ///
    /// Returns `true` if the position of the zipper has moved, or `false` if the zipper has returned
    /// to the root
    pub fn to_next_step(&mut self) -> bool {

        //If we're at a leaf ascend until we're not and jump to the next sibling
        if self.child_count() == 0 {
            //We can stop ascending when we succeed in moving to a sibling
            while !self.to_next_sibling_byte() {
                if !self.ascend(1) {
                    return false;
                }
            }
        } else {
            return self.descend_first_byte()
        }
        true
    }

    /// Descends the zipper's focus 'k' bytes, following the first child at each branch, and continuing with
    /// depth-first exploration until a path that is `k` bytes from the focus has been found
    ///
    /// Returns `true` if the zipper has sucessfully descended `k` steps, or `false` otherwise.  If this method
    /// returns `false` then the zipper will be in its original position.
    ///
    /// WARNING: This is not a constant-time operation, and may be as bad as `order n` with respect to the paths
    /// below the zipper's focus.  Although a typical cost is `order log n` or better.
    ///
    /// See: [Self::to_next_k_path]
    pub fn descend_first_k_path(&mut self, mut k: usize) -> bool {
        self.prepare_buffers();
        debug_assert!(self.is_regularized());

        let node_key = self.node_key();
        let node_key_len = node_key.len();
        let (cur_tok, full_key) = self.focus_node.iter_token_for_path(node_key);
        self.focus_iter_token = cur_tok;

        //Check if we can descend within this node
        if full_key.len() > node_key_len {
            if full_key.len() >= node_key_len+k {
                self.prefix_buf.extend(&full_key[node_key_len..node_key_len+k]);
                if full_key.len() == node_key_len+k {
                    self.regularize();
                }
                return true;
            } else {
                self.prefix_buf.extend(&full_key[node_key_len..]);
                k -= full_key.len()-node_key_len;
                self.regularize();
            }
        }

        self.k_path_internal(k, self.prefix_buf.len())
    }

    /// Moves the zipper's focus to the next location with the same path length as the current focus, following
    /// a depth-first exploration from a common root `k` steps above the current focus
    ///
    /// Returns `true` if the zipper has sucessfully moved to a new location at the same level, or `false` if
    /// no further locations exist.  If this method returns `false` then the zipper will be ascended `k` stept
    /// to the common root.
    ///
    /// WARNING: This is not a constant-time operation, and may be as bad as `order n` with respect to the paths
    /// below the zipper's focus.  Although a typical cost is `order log n` or better.
    ///
    /// See: [Self::descend_first_k_path]
    pub fn to_next_k_path(&mut self, k: usize) -> bool {
        let base_idx = if self.path_len() > k {
            self.prefix_buf.len() - k
        } else {
            self.origin_path.len()
        };
        //De-regularize the zipper
        debug_assert!(self.is_regularized());
        self.deregularize();
        self.k_path_internal(k, base_idx)
    }

    /// Internal method that implements both `k_path...` methods above
    #[inline]
    fn k_path_internal(&mut self, k: usize, base_idx: usize) -> bool {
        loop {
            debug_assert!(self.prefix_buf.len() <= base_idx+k);
            debug_assert!(self.prefix_buf.len() >= base_idx);
            debug_assert!(self.focus_iter_token != NODE_ITER_INVALID);

            if self.focus_iter_token == NODE_ITER_FINISHED {
                //This branch means we need to ascend or we're finished with the iteration and will
                // return a result at `path_len == base_idx`

                //Have we reached the root of this k_path iteration?
                if self.node_key_start() <= base_idx  {
                    self.focus_iter_token = NODE_ITER_FINISHED;
                    self.prefix_buf.truncate(base_idx);
                    return false
                }

                if let Some((focus_node, iter_tok, prefix_offset)) = self.ancestors.pop() {
                    self.focus_node = focus_node;
                    self.focus_iter_token = iter_tok;
                    self.prefix_buf.truncate(prefix_offset);
                } else {
                    let new_len = self.origin_path.len().max(self.root_key_offset.unwrap_or(0));
                    self.focus_iter_token = NODE_ITER_INVALID;
                    self.prefix_buf.truncate(new_len);
                    return false
                }
            }

            //Move the zipper to the next sibling position, if we can
            let (new_tok, key_bytes, child_node, _value) = self.focus_node.next_items(self.focus_iter_token);

            if new_tok != NODE_ITER_FINISHED {

                //Check to see if the iteration has modified more characters than allowed by `k`
                let key_start = self.node_key_start();
                if key_start < base_idx {
                    let fixed_len = self.prefix_buf.len() - base_idx;
                    if fixed_len > key_bytes.len() || &key_bytes[..fixed_len] != &self.prefix_buf[key_start..] {
                        self.prefix_buf.truncate(base_idx);
                        return false;
                    }
                }

                self.focus_iter_token = new_tok;

                //If we got here, it means we're either going to continue to descend, or return a
                // result at `path_len == base_idx+k`
                let key_start = self.node_key_start();
                self.prefix_buf.truncate(key_start);
                self.prefix_buf.extend(key_bytes);

                if self.prefix_buf.len() <= k+base_idx {
                    match child_node {
                        None => {},
                        Some(rec) => {
                            self.ancestors.push((self.focus_node.clone(), new_tok, self.prefix_buf.len()));
                            self.focus_node = rec.borrow().as_tagged();
                            self.focus_iter_token = self.focus_node.new_iter_token();
                        },
                    }
                } else {
                    self.prefix_buf.truncate(k+base_idx);
                }

                //See if we have a result to return
                if self.prefix_buf.len() == k+base_idx {
                    return true;
                }
            } else {
                self.focus_iter_token = NODE_ITER_FINISHED;
            }
        }
    }

    // //GOAT, ALTERNATIVE IMPLEMENTATION.  Performance is roughly equal between the two, but the other
    // //   implementation was chosen because it initializes the iter_token in preparation for subsequent iteration
    // pub fn descend_first_byte(&mut self) -> bool {
    //     self.prepare_buffers();
    //     debug_assert!(self.is_regularized());
    //     match self.focus_node.first_child_from_key(self.node_key()) {
    //         (Some(prefix), Some(child_node)) => {
    //             match prefix.len() {
    //                 0 => {
    //                     panic!(); //GOAT, I don't think we will hit this
    //                     //If we're at the root of the new node, descend to the first child
    //                     self.descend_first_byte()
    //                 },
    //                 1 => {
    //                     //Step to a new node
    //                     self.prefix_buf.push(prefix[0]);
    //                     self.ancestors.push((self.focus_node.clone(), self.focus_iter_token, self.prefix_buf.len()));
    //                     self.focus_iter_token = self.focus_node.new_iter_token();
    //                     self.focus_node = child_node.as_tagged();
    //                     true
    //                 },
    //                 _ => {
    //                     //Stay within the same node, and just grow the path
    //                     self.prefix_buf.push(prefix[0]);
    //                     true
    //                 }
    //             }
    //         },
    //         (Some(prefix), None) => {
    //             //Stay within the same node
    //             self.prefix_buf.push(prefix[0]);
    //             true
    //         },
    //         (None, _) => false
    //     }
    // }

    /// Descends the zipper's focus one step into the first child branch in a depth-first traversal
    ///
    /// Returns `true` if the zipper's focus has descended, or `false` if the zipper was already at a leaf.
    ///
    /// NOTE: This method should have identical behavior to [descend_indexed_branch(0)](Self::descend_indexed_branch),
    /// although with slightly less overhead.  Use this method in conjunction with [to_next_sibling_byte](Self::to_next_sibling_byte)
    /// to efficiently iterate all child branches below the focus.
    pub fn descend_first_byte(&mut self) -> bool {
        self.prepare_buffers();
        debug_assert!(self.is_regularized());
        let node_key = self.node_key();
        let node_key_len = node_key.len();
        let (cur_tok, full_key) = self.focus_node.iter_token_for_path(node_key);
        self.focus_iter_token = cur_tok;

        //Check to see if we can descend within this node
        if full_key.len() > node_key_len {
            self.prefix_buf.push(full_key[node_key_len]);
            if full_key.len() == node_key_len+1 {
                self.regularize();
            }
            return true;
        }

        let (new_tok, key_bytes, child_node, _value) = self.focus_node.next_items(self.focus_iter_token);
        self.focus_iter_token = new_tok;

        if new_tok != NODE_ITER_FINISHED {
            let byte_idx = self.node_key().len();
            self.prefix_buf.push(key_bytes[byte_idx]);

            if key_bytes.len() == byte_idx+1 {
                match child_node {
                    None => {},
                    Some(rec) => {
                        self.ancestors.push((self.focus_node.clone(), new_tok, self.prefix_buf.len()));
                        self.focus_node = rec.borrow().as_tagged();
                        self.focus_iter_token = self.focus_node.new_iter_token();
                    },
                }
            }

            true
        } else {
            false
        }
    }

    /// Moves the focus from a child branch to the next adjacent sibling, sharing a parent one byte above the focus
    ///
    /// Returns `true` if the zipper's focus has moved, or `false` if there is no adjacent sibling byte.
    pub fn to_next_sibling_byte(&mut self) -> bool {
        self.prepare_buffers();
        if self.prefix_buf.len() == 0 {
            return false
        }
        debug_assert!(self.is_regularized());
        self.deregularize();
        if self.focus_iter_token == NODE_ITER_INVALID {
            let (cur_tok, _full_key) = self.focus_node.iter_token_for_path(self.node_key());
            self.focus_iter_token = cur_tok;
        }

        let (new_tok, key_bytes, child_node, _value) = self.focus_node.next_items(self.focus_iter_token);

        if new_tok != NODE_ITER_FINISHED {

            //Check to see if the iter result has modified more than one byte
            let node_key = self.node_key();
            let fixed_len = node_key.len() - 1;
            if fixed_len > key_bytes.len() || key_bytes[..fixed_len] != node_key[..fixed_len] {
                return false;
            }

            self.focus_iter_token = new_tok;
            *self.prefix_buf.last_mut().unwrap() = key_bytes[0];

            if key_bytes.len() == 1 {
                match child_node {
                    None => {},
                    Some(rec) => {
                        self.ancestors.push((self.focus_node.clone(), new_tok, self.prefix_buf.len()));
                        self.focus_node = rec.borrow().as_tagged();
                        self.focus_iter_token = NODE_ITER_INVALID
                    },
                }
            }

            true
        } else {
            self.focus_iter_token = NODE_ITER_FINISHED;
            false
        }
    }

    /// Internal method that implements [Self::is_value], but so it can be inlined elsewhere
    #[inline]
    fn is_value_internal(&self) -> bool {
        let key = self.node_key();
        if key.len() > 0 {
            self.focus_node.node_contains_val(key)
        } else {
            if let Some((parent, _iter_tok, _prefix_offset)) = self.ancestors.last() {
                parent.node_contains_val(self.parent_key())
            } else {
                self.root_val.is_some()
            }
        }
    }

    /// Internal method implementing part of [Self::descend_until], but doesn't pay attention to to [Self::child_count]
    #[inline]
    fn descend_first(&mut self) {
        self.prepare_buffers();
        match self.focus_node.first_child_from_key(self.node_key()) {
            (Some(prefix), Some(child_node)) => {
                //Step to a new node
                self.prefix_buf.extend(prefix);
                self.ancestors.push((self.focus_node.clone(), self.focus_iter_token, self.prefix_buf.len()));
                self.focus_node = child_node.as_tagged();
                self.focus_iter_token = NODE_ITER_INVALID;

                //If we're at the root of the new node, descend to the first child
                if prefix.len() == 0 {
                    self.descend_first()
                }
            },
            (Some(prefix), None) => {
                //Stay within the same node
                self.prefix_buf.extend(prefix);
            },
            (None, _) => unreachable!()
        }
    }

    /// Internal method to ensure buffers to facilitate movement of zipper are allocated and initialized
    #[inline(always)]
    fn prepare_buffers(&mut self) {
        if self.prefix_buf.capacity() == 0 {
            self.prepare_buffers_guts()
        }
    }

    #[inline(never)]
    fn prepare_buffers_guts(&mut self) {
        self.prefix_buf = Vec::with_capacity(EXPECTED_PATH_LEN);
        self.ancestors = Vec::with_capacity(EXPECTED_DEPTH);
        self.prefix_buf.extend(self.origin_path);
    }

    /// Consumes the zipper and returns a Iterator over the downstream child bytes from the focus branch
    ///
    /// NOTE: This is mainly a convenience to allow the use of `collect` and `for` loops, as the other
    /// zipper methods can do the same thing without consuming the iterator
    pub fn into_child_iter(mut self) -> ReadZipperChildIter<'a, 'k, V> {
        self.descend_first_byte();
        ReadZipperChildIter::<'a, 'k, V>(Some(self))
    }

    /// Internal method returning the index to the key char beyond the path to the `self.focus_node`
    #[inline]
    fn node_key_start(&self) -> usize {
        self.ancestors.last().map(|(_node, _iter_tok, i)| *i)
            .unwrap_or_else(|| self.root_key_offset.unwrap_or(0))
    }
    /// Internal method returning the key within the focus node
    #[inline]
    fn node_key(&self) -> &[u8] {
        let key_start = self.node_key_start();
        if self.prefix_buf.len() > 0 {
            &self.prefix_buf[key_start..]
        } else {
            &self.origin_path[key_start..]
        }
    }
    /// Internal method returning the key that leads to `self.focus_node` within the parent
    /// NOTE: This method also returns the trailing parts of the key so it will only be useful when
    /// [self::node_key] returns `&[]`
    #[inline]
    fn parent_key(&self) -> &[u8] {
        if self.prefix_buf.len() > 0 {
            let key_start = if self.ancestors.len() > 1 {
                unsafe{ self.ancestors.get_unchecked(self.ancestors.len()-2) }.2
            } else {
                self.root_key_offset.unwrap_or(0)
            };
            &self.prefix_buf[key_start..]
        } else {
            unreachable!()
        }
    }
    /// Internal method similar to `self.node_key().len()`, but returns the number of chars that can be
    /// legally ascended within the node, taking into account the root_key
    #[inline]
    fn excess_key_len(&self) -> usize {
        self.prefix_buf.len() - self.ancestors.last().map(|(_node, _iter_tok, i)| *i).unwrap_or(self.origin_path.len())
    }
    /// Internal method which doesn't actually move the zipper, but ensures `self.node_key().len() > 0`
    /// WARNING, must never be called if `self.node_key().len() != 0`
    #[inline]
    fn ascend_across_nodes(&mut self) {
        debug_assert!(self.node_key().len() == 0);
        if let Some((focus_node, iter_tok, _prefix_offset)) = self.ancestors.pop() {
            self.focus_node = focus_node;
            self.focus_iter_token = iter_tok;
        } else {
            self.focus_iter_token = NODE_ITER_INVALID;
        }
    }
    /// Internal method used to impement `ascend_until` when ascending within a node
    #[inline]
    fn ascend_within_node(&mut self) {
        let branch_key = self.focus_node.prior_branch_key(self.node_key());
        let new_len = self.origin_path.len().max(self.node_key_start() + branch_key.len());
        self.prefix_buf.truncate(new_len);
    }
}

impl<'a, 'k, V: Clone> std::iter::IntoIterator for ReadZipper<'a, 'k, V> {
    type Item = (Vec<u8>, &'a V);
    type IntoIter = ReadZipperIter<'a, 'k, V>;

    fn into_iter(self) -> Self::IntoIter {
        ReadZipperIter {
            started: false,
            zipper: Some(self)
        }
    }
}

/// An iterator for depth-first traversal of a [ReadZipper], returned from [into_iter](std::iter::IntoIterator::into_iter)
///
/// NOTE: This is a convenience to allow access to syntactic sugar like `for` loops, [collect](std::iter::Iterator::collect),
///  etc.  It will always be faster to use the zipper itself for iteration and traversal.
pub struct ReadZipperIter<'a, 'k, V>{
    started: bool,
    zipper: Option<ReadZipper<'a, 'k, V>>,
}

impl<'a, 'k, V: Clone> Iterator for ReadZipperIter<'a, 'k, V> {
    type Item = (Vec<u8>, &'a V);

    fn next(&mut self) -> Option<(Vec<u8>, &'a V)> {
        if !self.started {
            self.started = true;
            if let Some(zipper) = &mut self.zipper {
                if let Some(val) = zipper.get_value() {
                    return Some((zipper.path().to_vec(), val))
                }
            }
        }
        if let Some(zipper) = &mut self.zipper {
            match zipper.to_next_val() {
                Some(val) => return Some((zipper.path().to_vec(), val)),
                None => self.zipper = None
            }
        }
        None
    }
}

/// An Iterator returned by [into_child_iter](ReadZipper::into_child_iter) to iterate over the children from
/// a branch of the trie
///
/// NOTE: This type is a convenience to allow access to syntactic sugar like `for` loops,
/// [collect](std::iter::Iterator::collect), etc.
///
/// NOTE: Does not descend recursively.  Use [into_iter](std::iter::IntoIterator::into_iter) for a depth-first
/// traversal, or just use the [Zipper] methods directly.
pub struct ReadZipperChildIter<'a, 'k, V>(Option<ReadZipper<'a, 'k, V>>);

impl<'a, 'k, V: Clone> Iterator for ReadZipperChildIter<'a, 'k, V> {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        if let Some(zipper) = &mut self.0 {
            let result = zipper.path().last().cloned();
            if !zipper.to_next_sibling_byte() {
                self.0 = None;
            }
            result
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::trie_map::*;
    use super::*;

    #[test]
    fn zipper_basic_test() {
        // from https://en.wikipedia.org/wiki/Radix_tree#/media/File:Patricia_trie.svg
        let mut btm = BytesTrieMap::new();
        let rs = ["romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });
    //GOAT, fix this, "at_path"
        // assert_eq!(btm.at("rom".as_bytes()).map(|m| m.items().collect::<HashSet<_>>()),
        //            Some(HashSet::from([("ane".as_bytes().to_vec(), &0), ("anus".as_bytes().to_vec(), &1), ("ulus".as_bytes().to_vec(), &2), ("'i".as_bytes().to_vec(), &7)])));

        fn assert_in_list(val: &[u8], list: &[&[u8]]) {
            for test_val in list {
                if *test_val == val {
                    return;
                }
            }
            panic!("val not found in list: {}", std::str::from_utf8(val).unwrap_or(""))
        }

        let mut rz = btm.read_zipper();
        rz.descend_to(&[b'r']); rz.descend_to(&[b'o']); rz.descend_to(&[b'm']); // focus = rom
        assert!(rz.descend_to(&[b'\''])); // focus = rom'  (' is the lowest byte)
        assert!(rz.to_sibling(true)); // focus = roma  (a is the second byte), but we can't actually guarantee whether we land on 'a' or 'u'
        assert_in_list(rz.path(), &[b"roma", b"romu"]);
        assert_eq!(rz.fork_zipper().into_child_iter().collect::<Vec<_>>(), vec![b'n']); // both follow-ups romane and romanus have n following a
        assert!(rz.to_sibling(true)); // focus = romu  (u is the third byte)
        assert_in_list(rz.path(), &[b"roma", b"romu"]);
        assert_eq!(rz.fork_zipper().into_child_iter().collect::<Vec<_>>(), vec![b'l']); // and romu is followed by lus
        assert!(!rz.to_sibling(true)); // fails because there were only 3 children ['\'', 'a', 'u']
        assert!(rz.to_sibling(false)); // focus = roma or romu (we stepped back)
        assert_in_list(rz.path(), &[b"roma", b"romu"]);
        assert!(rz.to_sibling(false)); // focus = rom' (we stepped back to where we began)
        assert_eq!(rz.path(), b"rom'");
        assert_eq!(rz.fork_zipper().into_child_iter().collect::<Vec<_>>(), vec![b'i']);
        assert!(rz.ascend(1)); // focus = rom
        assert_eq!(rz.fork_zipper().into_child_iter().collect::<Vec<_>>(), vec![b'\'', b'a', b'u']); // all three options we visited
        assert!(rz.descend_indexed_branch(0)); // focus = rom'
        assert_eq!(rz.fork_zipper().into_child_iter().collect::<Vec<_>>(), vec![b'i']);
        assert!(rz.ascend(1)); // focus = rom
        assert!(rz.descend_indexed_branch(1)); // focus = roma
        assert_eq!(rz.fork_zipper().into_child_iter().collect::<Vec<_>>(), vec![b'n']);
        assert!(rz.ascend(1));
        assert!(rz.descend_indexed_branch(2)); // focus = romu
        assert_eq!(rz.fork_zipper().into_child_iter().collect::<Vec<_>>(), vec![b'l']);
        assert!(rz.ascend(1));
        assert!(rz.descend_indexed_branch(1)); // focus = roma
        assert_eq!(rz.fork_zipper().into_child_iter().collect::<Vec<_>>(), vec![b'n']);
        assert!(rz.ascend(1));
        // ' < a < u
        // 39 105 117
    }

    #[test]
    fn zipper_with_starting_key() {
        let mut btm = BytesTrieMap::new();
        let rs = ["romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });

        //Test `descend_to` and `ascend_until`
        let root_key = b"ro";
        let mut zipper = ReadZipper::new_with_node_and_path(btm.root().borrow(), root_key, Some(root_key.len()), ZipperTracker::default());
        assert_eq!(zipper.path(), b"");
        assert_eq!(zipper.child_count(), 1);
        zipper.descend_to(b"m");
        assert_eq!(zipper.path(), b"m");
        assert_eq!(zipper.child_count(), 3);
        zipper.descend_to(b"an");
        assert_eq!(zipper.path(), b"man");
        assert_eq!(zipper.child_count(), 2);
        zipper.descend_to(b"e");
        assert_eq!(zipper.path(), b"mane");
        assert_eq!(zipper.child_count(), 0);
        assert_eq!(zipper.ascend_until(), true);
        zipper.descend_to(b"us");
        assert_eq!(zipper.path(), b"manus");
        assert_eq!(zipper.child_count(), 0);
        assert_eq!(zipper.ascend_until(), true);
        assert_eq!(zipper.path(), b"man");
        assert_eq!(zipper.child_count(), 2);
        assert_eq!(zipper.ascend_until(), true);
        assert_eq!(zipper.path(), b"m");
        assert_eq!(zipper.child_count(), 3);
        assert_eq!(zipper.ascend_until(), true);
        assert_eq!(zipper.path(), b"");
        assert_eq!(zipper.child_count(), 1);
        assert_eq!(zipper.at_root(), true);
        assert_eq!(zipper.ascend_until(), false);

        //Test `ascend`
        zipper.descend_to(b"manus");
        assert_eq!(zipper.path(), b"manus");
        assert_eq!(zipper.ascend(1), true);
        assert_eq!(zipper.path(), b"manu");
        assert_eq!(zipper.ascend(5), false);
        assert_eq!(zipper.path(), b"");
        assert_eq!(zipper.at_root(), true);
        zipper.descend_to(b"mane");
        assert_eq!(zipper.path(), b"mane");
        assert_eq!(zipper.ascend(3), true);
        assert_eq!(zipper.path(), b"m");
        assert_eq!(zipper.child_count(), 3);
    }

    #[test]
    fn indexed_zipper_movement() {
        let mut btm = BytesTrieMap::new();
        let rs = ["arrow", "bow", "cannon", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });
        let mut zipper = btm.read_zipper();

        //descends a single specific byte using `descend_indexed_branch`. Just for testing. A real user would use `descend_towards`
        fn descend_byte(zipper: &mut ReadZipper<usize>, byte: u8) {
            for i in 0..zipper.child_count() {
                assert_eq!(zipper.descend_indexed_branch(i), true);
                if *zipper.path().last().unwrap() == byte {
                    break
                } else {
                    assert_eq!(zipper.ascend(1), true);
                }
            }
        }

        assert_eq!(zipper.path(), b"");
        assert_eq!(zipper.child_count(), 4);
        descend_byte(&mut zipper, b'r');
        assert_eq!(zipper.path(), b"r");
        assert_eq!(zipper.child_count(), 2);
        assert_eq!(zipper.descend_until(), false);
        descend_byte(&mut zipper, b'o');
        assert_eq!(zipper.path(), b"ro");
        assert_eq!(zipper.child_count(), 1);
        assert_eq!(zipper.descend_until(), true);
        assert_eq!(zipper.path(), b"rom");
        assert_eq!(zipper.child_count(), 3);

        zipper.reset();
        assert_eq!(zipper.descend_until(), false);
        descend_byte(&mut zipper, b'a');
        assert_eq!(zipper.path(), b"a");
        assert_eq!(zipper.child_count(), 1);
        assert_eq!(zipper.descend_until(), true);
        assert_eq!(zipper.path(), b"arrow");
        assert_eq!(zipper.child_count(), 0);

        assert_eq!(zipper.ascend(3), true);
        assert_eq!(zipper.path(), b"ar");
        assert_eq!(zipper.child_count(), 1);

    }

    #[test]
    fn zipper_value_access() {
        let mut btm = BytesTrieMap::new();
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        rs.iter().for_each(|r| { btm.insert(r.as_bytes(), *r); });

        let root_key = b"ro";
        let mut zipper = ReadZipper::new_with_node_and_path(btm.root().borrow(), root_key, Some(root_key.len()), ZipperTracker::default());
        assert_eq!(zipper.is_value(), false);
        zipper.descend_to(b"mulus");
        assert_eq!(zipper.is_value(), true);
        assert_eq!(zipper.get_value(), Some(&"romulus"));

        let root_key = b"roman";
        let mut zipper = ReadZipper::new_with_node_and_path(btm.root().borrow(), root_key, Some(root_key.len()), ZipperTracker::default());
        assert_eq!(zipper.is_value(), true);
        assert_eq!(zipper.get_value(), Some(&"roman"));
        zipper.descend_to(b"e");
        assert_eq!(zipper.is_value(), true);
        assert_eq!(zipper.get_value(), Some(&"romane"));
        assert_eq!(zipper.ascend(1), true);
        zipper.descend_to(b"u");
        assert_eq!(zipper.is_value(), false);
        assert_eq!(zipper.get_value(), None);
        zipper.descend_until();
        assert_eq!(zipper.is_value(), true);
        assert_eq!(zipper.get_value(), Some(&"romanus"));
    }

    #[test]
    fn zipper_child_mask_test() {
        let keys = vec![
            vec![8, 194, 1, 45, 194, 1],
            vec![34, 193],
        ];
        let map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut zipper = map.read_zipper();

        assert_eq!(zipper.descend_to(&[8, 194, 1]), true);
        assert_eq!(zipper.child_count(), 1);
        assert_eq!(zipper.child_mask(), [0x200000000000, 0, 0, 0]);

        zipper.reset();
        assert_eq!(zipper.descend_to(&[8, 194, 1, 45]), true);
        assert_eq!(zipper.child_count(), 1);
        assert_eq!(zipper.child_mask(), [0, 0, 0, 0x4]);
    }

    #[test]
    fn zipper_iter() {
        let mut btm = BytesTrieMap::new();
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });
        let mut zipper = btm.read_zipper();

        //Test iteration of the whole tree
        let mut count = 0;
        assert_eq!(zipper.is_value(), false);
        while let Some(&val) = zipper.to_next_val() {
            // println!("{val}  {} = {}", std::str::from_utf8(zipper.path()).unwrap(), zipper.get_value().unwrap());
            assert_eq!(rs[val].as_bytes(), zipper.path());
            count += 1;
        }
        assert_eq!(count, rs.len());

        //Fork a sub-zipper, and test iteration of that subtree
        zipper.reset();
        zipper.descend_to(b"rub");
        let mut sub_zipper = zipper.fork_zipper();
        while let Some(&val) = sub_zipper.to_next_val() {
            // println!("{val}  {} = {}", std::str::from_utf8(sub_zipper.path()).unwrap(), std::str::from_utf8(&rs[val].as_bytes()[3..]).unwrap());
            assert_eq!(&rs[val].as_bytes()[3..], sub_zipper.path());
        }
        drop(sub_zipper);

        for (path, &val) in zipper {
            // println!("{val}  {} = {}", std::str::from_utf8(&path).unwrap(), std::str::from_utf8(rs[val].as_bytes()).unwrap());
            assert_eq!(rs[val].as_bytes(), path);
        }
    }

    #[test]
    fn path_concat_test() {
        let parent_path = "�parent";
        let exprs = [
            "�parent�Tom�Bob",
            "�parent�Pam�Bob",
            "�parent�Tom�Liz",
            "�parent�Bob�Ann",
            "�parent�Bob�Pat",
            "�parent�Pat�Jim",
            "�female�Pam",
            "�male�Tom",
            "�male�Bob",
            "�female�Liz",
            "�female�Pat",
            "�female�Ann",
            "�male�Jim",
        ];
        let family: BytesTrieMap<i32> = exprs.iter().enumerate().map(|(i, k)| (k, i as i32)).collect();

        let mut parent_zipper = family.read_zipper_at_path(parent_path.as_bytes());

        assert!(family.contains_path(parent_path));

        let mut full_parent_path = parent_path.as_bytes().to_vec();
        full_parent_path.extend(parent_zipper.path());
        assert!(family.contains_path(&full_parent_path));

        while let Some(_val) = parent_zipper.to_next_val() {
            let mut full_parent_path = parent_path.as_bytes().to_vec();
            full_parent_path.extend(parent_zipper.path());
            assert!(family.contains(&full_parent_path));
            assert_eq!(full_parent_path, parent_zipper.origin_path().unwrap());
        }
    }

    #[test]
    fn to_next_step_test() {
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        let btm: BytesTrieMap<usize> = rs.into_iter().enumerate().map(|(i, r)| (r.as_bytes(), i)).collect();
        let mut zipper = btm.read_zipper();

        let mut i = 0;
        while zipper.to_next_step() {
            match i {
                0 => assert_eq!(zipper.path(), b"a"),
                4 => assert_eq!(zipper.path(), b"arrow"),
                5 => assert_eq!(zipper.path(), b"b"),
                7 => assert_eq!(zipper.path(), b"bow"),
                8 => assert_eq!(zipper.path(), b"c"),
                13 => assert_eq!(zipper.path(), b"cannon"),
                14 => assert_eq!(zipper.path(), b"r"),
                18 => assert_eq!(zipper.path(), b"rom'i"),
                20 => assert_eq!(zipper.path(), b"roman"),
                21 => assert_eq!(zipper.path(), b"romane"),
                23 => assert_eq!(zipper.path(), b"romanus"),
                27 => assert_eq!(zipper.path(), b"romulus"),
                28 => assert_eq!(zipper.path(), b"ru"),
                32 => assert_eq!(zipper.path(), b"rubens"),
                33 => assert_eq!(zipper.path(), b"ruber"),
                37 => assert_eq!(zipper.path(), b"rubicon"),
                42 => assert_eq!(zipper.path(), b"rubicundus"),
                _ => {}
            }
            i += 1;
        }
    }

    #[test]
    fn full_path_test() {
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        let btm: BytesTrieMap<u64> = rs.into_iter().enumerate().map(|(i, k)| (k, i as u64)).collect();

        let mut zipper = btm.read_zipper_at_path(b"roma");
        assert_eq!(format!("roma{}", std::str::from_utf8(zipper.path()).unwrap()), "roma");
        assert_eq!(std::str::from_utf8(zipper.origin_path().unwrap()).unwrap(), "roma");
        zipper.descend_to(b"n");
        assert_eq!(format!("roma{}", std::str::from_utf8(zipper.path()).unwrap()), "roman");
        assert_eq!(std::str::from_utf8(zipper.origin_path().unwrap()).unwrap(), "roman");
        zipper.to_next_val();
        assert_eq!(format!("roma{}", std::str::from_utf8(zipper.path()).unwrap()), "romane");
        assert_eq!(std::str::from_utf8(zipper.origin_path().unwrap()).unwrap(), "romane");
        zipper.to_next_val();
        assert_eq!(format!("roma{}", std::str::from_utf8(zipper.path()).unwrap()), "romanus");
        assert_eq!(std::str::from_utf8(zipper.origin_path().unwrap()).unwrap(), "romanus");
        zipper.to_next_val();
        assert_eq!(zipper.path().len(), 0);
    }

    #[test]
    fn k_path_test1() {
        //This is a toy encoding where `:n:` precedes a symbol of `n` characters long
        let keys = [
            ":5:above:3:the:4:fray:",
            ":5:err:",
            ":5:erronious:6:potato:",
            ":5:error:2:is:2:my:4:name:",
            ":5:hello:5:world:",
            ":5:mucky:4:muck:",
            ":5:roger:6:rabbit:",
            ":5:zebra:",
            ":9:muckymuck:5:raker:",
        ];
        let map: BytesTrieMap<u64> = keys.into_iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut zipper = map.read_zipper_at_path(b":");

        //This is a cheesy way to encode lengths, but it's is more readable than unprintable chars
        assert!(zipper.descend_indexed_branch(0));
        let sym_len = usize::from_str_radix(std::str::from_utf8(&[zipper.path()[0]]).unwrap(), 10).unwrap();
        assert_eq!(sym_len, 5);

        //Step over the ':' character
        assert!(zipper.descend_indexed_branch(0));
        assert_eq!(zipper.child_count(), 6);

        //Start iterating over all the symbols of length=sym_len
        assert_eq!(zipper.descend_first_k_path(sym_len+1), true);

        //This should have taken us to "above:"
        assert_eq!(zipper.path(), b"5:above:");

        //Go to the next symbol.
        // Two interesting things will happen.  First, we blow past "err" because its path length is
        // shorter than `k`.  Second, we will stop in the middle of "erronious".
        // These situations would be caused by an encode bug.  Which hopefully we won't have in real
        // paths. But the parser should recognize the last digit of the path isn't ':'
        assert_eq!(zipper.to_next_k_path(sym_len+1), true);
        assert_eq!(zipper.path(), b"5:erroni");
        assert_ne!(zipper.path().last(), Some(&b':'));

        //Now we'll move on to some correctly formed symbols
        assert_eq!(zipper.to_next_k_path(sym_len+1), true);
        assert_eq!(zipper.path(), b"5:error:");
        assert_eq!(zipper.to_next_k_path(sym_len+1), true);
        assert_eq!(zipper.path(), b"5:hello:");
        assert_eq!(zipper.to_next_k_path(sym_len+1), true);
        assert_eq!(zipper.path(), b"5:mucky:");
        assert_eq!(zipper.to_next_k_path(sym_len+1), true);
        assert_eq!(zipper.path(), b"5:roger:");
        assert_eq!(zipper.to_next_k_path(sym_len+1), true);
        assert_eq!(zipper.path(), b"5:zebra:");

        //The last step should return false, and put us back to where we began
        assert_eq!(zipper.to_next_k_path(sym_len+1), false);
        assert_eq!(zipper.path(), b"5:");
        assert_eq!(zipper.child_count(), 6);
    }

    #[test]
    fn zipper_test_masks() {
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        let btm: BytesTrieMap<usize> = rs.into_iter().enumerate().map(|(i, r)| (r.as_bytes(), i)).collect();
        let mut zipper = btm.read_zipper();

        //'a' + 'b' + 'c' + 'r'
        assert_eq!(zipper.child_mask(), [0, 1<<(b'a'-64) | 1<<(b'b'-64) | 1<<(b'c'-64) | 1<<(b'r'-64), 0, 0]);

        let mut i = 0;
        while zipper.to_next_step() {
            match i {
                //'r' descending from 'a' in "arrow"
                0 => assert_eq!(zipper.child_mask(), [0, 1<<(b'r'-64), 0, 0]),
                //'r' descending from "ar" in "arrow"
                1 => assert_eq!(zipper.child_mask(), [0, 1<<(b'r'-64), 0, 0]),
                //'o' descending from "arr" in "arrow"
                2 => assert_eq!(zipper.child_mask(), [0, 1<<(b'o'-64), 0, 0]),
                //'w' descending from "arro" in "arrow"
                3 => assert_eq!(zipper.child_mask(), [0, 1<<(b'w'-64), 0, 0]),
                //leaf node, "arrow"
                4 => assert_eq!(zipper.child_mask(), [0, 0, 0, 0]),
                //'o' + 'u' descending from 'r' in "roman", "rubens", etc.
                14 => assert_eq!(zipper.child_mask(), [0, 1<<(b'o'-64) | 1<<(b'u'-64), 0, 0]),
                _ => {}
            }
            i += 1;
        }
    }

    #[test]
    fn k_path_test2() {
        const N: usize = 50;
        let keys: Vec<Vec<u8>> = (0..N).into_iter().map(|i| {
            let len = (i % 15) + 5; //length between 5 and 20 chars
            (0..len).into_iter().map(|j| ((j+i) % 255) as u8).collect()
        }).collect();
        let map: BytesTrieMap<usize> = keys.iter().enumerate().map(|(n, s)| (s, n)).collect();

        let mut zipper = map.read_zipper();
        zipper.descend_first_k_path(5);
        let mut count = 1;
        while zipper.to_next_k_path(5) {
            count += 1;
        }
        assert_eq!(count, N);
    }

    #[test]
    fn k_path_test3() {
        let keys = [":1a1A", ":1a1B", ":1a1C", ":1b1A", ":1b1B", ":1b1C", ":1c1A"];
        let map: BytesTrieMap<u64> = keys.into_iter().enumerate().map(|(i, k)| (k, i as u64)).collect();

        //Scan over the first symbols in the path (lower case letters)
        let mut zipper = map.read_zipper_at_path(b":");
        assert_eq!(zipper.descend_to(b"1"), true);
        assert_eq!(zipper.descend_first_k_path(1), true);
        assert_eq!(zipper.path(), b"1a");
        assert_eq!(zipper.to_next_k_path(1), true);
        assert_eq!(zipper.path(), b"1b");
        assert_eq!(zipper.to_next_k_path(1), true);
        assert_eq!(zipper.path(), b"1c");
        assert_eq!(zipper.to_next_k_path(1), false);
        assert_eq!(zipper.path(), b"1");

        //Scan over the nested second symbols in the path (upper case letters)
        zipper.reset();
        assert!(zipper.descend_to(b"1a1"));
        assert_eq!(zipper.descend_first_k_path(1), true);
        assert_eq!(zipper.path(), b"1a1A");
        assert_eq!(zipper.to_next_k_path(1), true);
        assert_eq!(zipper.path(), b"1a1B");
        assert_eq!(zipper.to_next_k_path(1), true);
        assert_eq!(zipper.path(), b"1a1C");
        assert_eq!(zipper.to_next_k_path(1), false);
        assert_eq!(zipper.path(), b"1a1");

        //Recursively scan nested symbols within a scan of the first outer symbols
        zipper.reset();
        assert!(zipper.descend_to(b"1"));
        assert_eq!(zipper.descend_first_k_path(1), true);
        assert_eq!(zipper.path(), b"1a");
        assert_eq!(zipper.descend_first_k_path(2), true);
        assert_eq!(zipper.path(), b"1a1A");
        assert_eq!(zipper.to_next_k_path(2), true);
        assert_eq!(zipper.path(), b"1a1B");
        assert_eq!(zipper.to_next_k_path(2), true);
        assert_eq!(zipper.path(), b"1a1C");
        assert_eq!(zipper.to_next_k_path(2), false);
        assert_eq!(zipper.path(), b"1a");
        assert_eq!(zipper.to_next_k_path(1), true);
        assert_eq!(zipper.path(), b"1b");
        assert_eq!(zipper.to_next_k_path(1), true);
        assert_eq!(zipper.path(), b"1c");
        assert_eq!(zipper.to_next_k_path(1), false);
        assert_eq!(zipper.path(), b"1");

        //Similar to above, but inter-operating with `descend_indexed_branch`
        zipper.reset();
        assert!(zipper.descend_to(b"1"));
        assert_eq!(zipper.descend_first_k_path(1), true);
        assert_eq!(zipper.path(), b"1a");
        assert_eq!(zipper.descend_indexed_branch(0), true);
        assert_eq!(zipper.path(), b"1a1");
        assert_eq!(zipper.descend_first_k_path(1), true);
        assert_eq!(zipper.path(), b"1a1A");
        assert_eq!(zipper.to_next_k_path(1), true);
        assert_eq!(zipper.path(), b"1a1B");
        assert_eq!(zipper.to_next_k_path(1), true);
        assert_eq!(zipper.path(), b"1a1C");
        assert_eq!(zipper.to_next_k_path(1), false);
        assert_eq!(zipper.path(), b"1a1");
        assert_eq!(zipper.ascend(1), true);
        assert_eq!(zipper.path(), b"1a");
        assert_eq!(zipper.to_next_k_path(1), true);
        assert_eq!(zipper.path(), b"1b");
        assert_eq!(zipper.to_next_k_path(1), true);
        assert_eq!(zipper.path(), b"1c");
        assert_eq!(zipper.to_next_k_path(1), false);
        assert_eq!(zipper.path(), b"1");
    }

    #[test]
    fn k_path_test4() {
        let keys = vec![
            vec![100, 74, 37, 218, 90, 211, 23, 84, 226, 59, 193, 236],
            vec![199, 102, 166, 28, 234, 168, 198, 13],
            vec![101, 241, 88, 163, 2, 9, 37, 110, 53, 201, 251, 164, 23, 162, 216],
            vec![237, 8, 108, 15, 63, 3, 249, 78, 200, 154, 103, 191],
            vec![106, 30, 34, 182, 157, 102, 126, 90, 200, 5, 93, 0, 163, 245, 112],
            vec![188, 177, 13, 5, 50, 66, 169, 113, 157, 202, 72, 11, 79, 73],
            vec![250, 96, 103, 31, 32, 104],
            vec![100, 152, 199, 46, 48, 252, 139, 150, 158, 8, 57, 50, 123],
            vec![65, 16, 128, 207, 27, 252, 145, 123, 105, 238, 230],
            vec![244, 34, 40, 224, 11, 125, 102],
            vec![116, 63, 105, 214, 137, 86, 202],
            vec![63, 70, 201, 21, 131, 60],
            vec![139, 209, 149, 73, 172, 12, 139, 80, 184, 105],
            vec![253, 235, 49, 156, 40, 50, 60, 73, 145, 249],
            vec![228, 81, 220, 29, 208, 234, 27],
            vec![116, 109, 134, 122, 15, 78, 126, 240, 158, 42, 221, 229, 93, 200, 194],
            vec![180, 216, 189, 14, 82, 14, 170, 195, 196, 42, 177, 144, 153, 156, 140, 109, 93, 78, 157],
            vec![190, 6, 59, 69, 208, 253, 2, 33, 86],
            vec![245, 168, 144, 122, 243, 111],
            vec![123, 150, 249, 114, 32, 140, 186, 204, 199, 8, 205, 150, 34, 104, 186, 236],
            vec![8, 29, 191, 189, 72, 101, 39, 24, 105, 44, 13, 87, 75, 187],
            vec![14, 201, 29, 151, 113, 10, 175],
            vec![83, 130, 247, 5, 250, 101, 141, 5, 42, 132, 205, 3, 118, 152, 33, 219, 1, 91, 204],
            vec![207, 215, 38, 17, 244, 96],
            vec![34, 132, 138, 222, 250, 162, 231, 68, 142, 162, 152, 172, 244, 102, 179, 111, 161, 95],
            vec![124, 120, 11, 4, 219, 210, 172, 50, 182, 160, 86, 88, 136, 122, 97, 98],
            vec![86, 74, 181, 17, 3, 173, 12],
            vec![18, 234, 66, 134, 20],
            vec![20, 24, 83, 219, 209, 20, 236, 128, 155, 15, 110, 54, 237, 105, 186, 62],
            vec![67, 11, 50, 124, 120, 33, 218],
            vec![89, 248, 169, 97, 245, 98, 230, 53, 114, 198, 227, 148, 22, 127, 198, 153, 238, 59, 223],
            vec![100, 128, 38, 54, 171, 186, 9, 133, 191, 82, 113, 86, 10, 72, 236, 124, 201, 65],
            vec![152, 115, 99, 124, 81, 254, 0, 179, 24, 87, 24, 77, 60],
            vec![107, 117, 222, 38, 162, 193, 48, 44, 140, 162, 104, 139, 90],
            vec![63, 29, 217, 85, 63, 130, 110, 121, 227, 43, 215, 223, 249, 1, 72, 134, 92, 188],
            vec![117, 3, 144, 15, 103, 113, 130, 253, 0, 102, 47, 24, 234, 0, 159],
            vec![38, 60, 197, 120, 53, 94, 202, 137, 116, 27, 12, 181],
            vec![248, 41, 252, 254, 98, 173, 42, 92, 30, 65, 72],
            vec![240, 147, 89, 110, 224, 8],
            vec![199, 86, 108, 195, 62, 169, 61],
            vec![93, 225, 21, 185, 91, 23, 19, 7, 108, 176, 191, 91],
            vec![70, 10, 122, 77, 171],
            vec![32, 161, 24, 162, 112, 152, 21, 226, 149, 253, 212, 246, 175, 182],
            vec![99, 7, 213, 87, 192, 2, 110, 242, 222, 89, 20, 83, 138, 112],
            vec![92, 64, 61, 35, 111, 41, 151, 121, 24, 157],
            vec![115, 201, 114, 124, 135, 246, 93, 230, 210, 164, 213, 254, 108, 181, 77, 19, 103, 166],
            vec![26, 231, 59, 238, 246],
            vec![52, 74, 93, 202, 140, 11, 56, 46, 211, 194, 137, 65, 36, 90, 209],
            vec![56, 245, 179, 40, 190, 168, 116, 115],
            vec![192, 215, 69, 171, 218, 187, 202, 120, 92, 33, 14, 77, 34, 46, 40, 93, 135, 117, 152],
        ];

        let map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut zipper = map.read_zipper();
        zipper.descend_first_k_path(5);
        let mut count = 1;
        while zipper.to_next_k_path(5) {
            count += 1;
        }
        assert_eq!(count, keys.len());
    }

    #[test]
    fn k_path_test5() {
        //EXPLANATION: This test triggers an edge-case because the first path is 15 bytes long, but
        // `LineListNode::KEY_BYTES_CNT` is 14.  That means the path spills over to two nodes, 1 bytes
        // before the end.  Then, we do `descend_first_k_path(2)`, meaning we end up straddling the
        // node boundary, so `to_next_k_path(2)` needs to step back across to the parent node, and
        // truncate the zipper's key, but not truncate too much
        let keys = vec![
            vec![3, 193, 4, 194, 1, 43, 3, 193, 8, 194, 1, 45, 194, 1, 46],
            vec![3, 193, 4, 194, 1, 43, 3, 193, 34, 193],
        ];

        let map: BytesTrieMap<u64> = keys.iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut zipper = map.read_zipper();
        assert!(zipper.descend_to(&[3, 193, 4, 194, 1, 43, 3, 193, 8, 194, 1, 45, 194]));
        assert_eq!(zipper.descend_first_k_path(2), true);
        assert_eq!(zipper.path(), &[3, 193, 4, 194, 1, 43, 3, 193, 8, 194, 1, 45, 194, 1, 46]);
        assert_eq!(zipper.to_next_k_path(2), false);
        assert_eq!(zipper.path(), &[3, 193, 4, 194, 1, 43, 3, 193, 8, 194, 1, 45, 194]);
    }

    #[test]
    fn zipper_byte_iter_test1() {
        let keys = [
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            "ab",
        ];
        let map: BytesTrieMap<u64> = keys.into_iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut zipper = map.read_zipper();

        assert_eq!(zipper.descend_to_byte(b'A'), true);
        assert_eq!(zipper.descend_first_byte(), true);
        assert_eq!(zipper.path(), b"AB");
        assert_eq!(zipper.to_next_sibling_byte(), false);
        assert_eq!(zipper.path(), b"AB");

        zipper.reset();
        assert_eq!(zipper.descend_to_byte(b'A'), true);
        assert_eq!(zipper.descend_first_k_path(1), true);
        assert_eq!(zipper.path(), b"AB");
        assert_eq!(zipper.to_next_k_path(1), false);
        assert_eq!(zipper.path(), b"A");
    }

    #[test]
    fn zipper_byte_iter_test2() {
        let keys = [
            vec![2, 194, 1, 1, 193, 5],
            vec![3, 194, 1, 0, 193, 6, 193, 5],
            vec![3, 193, 4, 193],
        ];
        let map: BytesTrieMap<u64> = keys.into_iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut zipper = map.read_zipper_at_path(&[2, 194]);

        assert_eq!(zipper.descend_first_byte(), true);
        assert_eq!(zipper.path(), &[1]);
        assert_eq!(zipper.to_next_sibling_byte(), false);
        assert_eq!(zipper.path(), &[1]);

        zipper.reset();
        assert_eq!(zipper.descend_first_k_path(1), true);
        assert_eq!(zipper.path(), &[1]);
        assert_eq!(zipper.to_next_k_path(1), false);
        assert_eq!(zipper.path(), &[]);
    }

    #[test]
    fn zipper_byte_iter_test3() {
        let keys = [
            vec![3, 193, 4, 193, 5, 2, 193, 6, 193, 7],
            vec![3, 193, 4, 193, 5, 2, 193, 6, 255],
        ];
        let map: BytesTrieMap<u64> = keys.into_iter().enumerate().map(|(i, k)| (k, i as u64)).collect();
        let mut zipper = map.read_zipper_at_path(&[3, 193, 4, 193, 5, 2, 193]);

        assert_eq!(zipper.path(), &[]);
        assert_eq!(zipper.descend_first_byte(), true);
        assert_eq!(zipper.path(), &[6]);
        assert_eq!(zipper.descend_first_byte(), true);
        assert_eq!(zipper.path(), &[6, 193]);
        assert_eq!(zipper.descend_first_byte(), true);
        assert_eq!(zipper.path(), &[6, 193, 7]);
    }
}

// GOAT, new zipper API.  "fork_zipper_at_path".  Cheap call to make a new zipper cheaper than descend_to
//   for the current zipper.  The idea is that there is no need to save the intervening node pointers along the path
//