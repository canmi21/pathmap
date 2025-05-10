
use core::fmt::{Debug, Formatter};
use core::{ptr, slice};
use std::collections::HashMap;

use crate::ring::*;
use crate::utils::ByteMask;

//OPTIMIZATION QUESTION 2, a note on Rc vs. rclite: Rc's payload bloat is 16 bytes, while rclite's is much smaller <8 Bytes.
// That's a big deal on a DenseByteNode because it pushes it from a single cache line onto two.
// However, rclite doesn't currently support DSTs. (like &dyn)  Therefore there are two options if we
// want to keep Rc<DenseByteNode<_>> as a type that's a single cache line. (See OPTIMIZATION QUESTION 1)
//
// 1. try and add a PR to rclite.  However it's possible not supporting DSTs was a deliberate choice, so
//  they may not even take the PR.  And it's also possible (likely) it's hard to do (given the original
//  authors didn't do it alrady)... If I had to guess it's because the rclite header is at the end of
//  the object, while the Rc header is at the beginning.
// 2. hunt for a solution that lets us do something like: Vec<dyn TrieNode<_>> where TrieNode is only
//  implementable on pointer types like Rc, Box, etc.  I don't think Rust lets you do that, but perhaps
//  it should!
//
// use rclite::Rc;
// use std::rc::Rc;


//OPTIMIZATION QUESTION 1, figure out the best compromise with regard to where to put Rc...
// Question: Is it true that it's always going to be cheaper to increment a refcount than to do a clone?
//  and to decrement a refcount instead of free?  because that seems like it would argue for an Rc at every node.
//
// However, the mutable traversal loop calls `node_get_child_mut`, which, in turn, needs to check the
// refcount imposing non-zero overhead.
//
// The bigger concern is that incrementing the refcount requires thread syncronization around the atomic
// access and that could be painful
//
//Conclusion: Need a massively multi-threaded benchmark to decide what to do with Rc / RcLite
//

use crate::utils::BitMask;
use crate::trie_node::*;
use crate::line_list_node::LineListNode;

//NOTE: This: `core::array::from_fn(|i| i as u8);` ought to work, but https://github.com/rust-lang/rust/issues/109341
const ALL_BYTES: [u8; 256] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239, 240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 254, 255];

/// A ByteNode with insides that **cannot** be shared across threads
pub type DenseByteNode<V> = ByteNode<OrdinaryCoFree<V>>;

/// A ByteNode with insides that **can** be shared across threads
pub type CellByteNode<V> = ByteNode<CellCoFree<V>>;

#[derive(Clone)]
pub struct ByteNode<Cf> {
    pub mask: ByteMask,
    values: Vec<Cf, crate::utils::Leak>,
}

impl<V: Clone + Send + Sync, Cf: CoFree<V=V>> Default for ByteNode<Cf> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V : Clone + Sync + Send, Cf: CoFree<V=V>> Debug for ByteNode<Cf> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        //Recursively printing a whole tree will get pretty unwieldy.  Should do something
        // like serialization for inspection using standard tools.
        write!(f, "ByteNode {{count={}", self.values.len())?;
        self.for_each_item(|node, c, i| {
            let cf = node.values.get(i).unwrap();
            let _ = write!(f, ", {c}:(val={} child={})", cf.has_val(), cf.has_rec());
        });
        write!(f, "}}")
    }
}

impl<V : Clone + Sync + Send, Cf: CoFree<V=V>> ByteNode<Cf> {
    #[inline]
    pub fn new() -> Self {
        Self {
            mask: ByteMask::EMPTY,
            values: Vec::new_in(crate::utils::Leak)
            // values: Vec::with_capacity_in(256, crate::utils::Leak)
        }
    }
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            mask: ByteMask::EMPTY,
            values: Vec::with_capacity_in(capacity, crate::utils::Leak),
            // values: Vec::with_capacity_in(256, crate::utils::Leak),
        }
    }
    #[inline]
    pub fn reserve_capacity(&mut self, additional: usize) {
        // self.values.reserve(additional)
    }

    /// Adds a new child at the specified key byte.  Replaces and returns an existing branch.
    /// Use [join_child_into] to join with the existing branch
    #[inline]
    pub fn set_child(&mut self, k: u8, node: TrieNodeODRc<V>) -> Option<TrieNodeODRc<V>> {
        let ix = self.mask.index_of(k) as usize;
        if self.mask.test_bit(k) {
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            cf.swap_rec(node)
        } else {
            self.mask.set_bit(k);
            let new_cf = CoFree::new(Some(node), None);
            self.values.insert(ix, new_cf);
            None
        }
    }

    /// The same as [set_child] if no child exists in the node at the key.  Otherwise joins the two nodes
    /// together
    #[inline]
    pub fn join_child_into(&mut self, k: u8, node: TrieNodeODRc<V>) -> AlgebraicStatus where V: Clone + Lattice {
        let ix = self.mask.index_of(k) as usize;
        if self.mask.test_bit(k) {
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            match cf.rec_mut() {
                Some(existing_node) => {
                    // println!("join_child_into {:?} {:?}", existing_node, node);
                    existing_node.join_into(node)
                },
                None => {
                    cf.set_rec(node);
                    AlgebraicStatus::Element
                }
            }
        } else {
            self.mask.set_bit(k);
            let new_cf = CoFree::new(Some(node), None);
            self.values.insert(ix, new_cf);
            AlgebraicStatus::Element
        }
    }

    #[inline]
    pub fn set_val(&mut self, k: u8, val: V) -> Option<V> {
        let ix = self.mask.index_of(k) as usize;
        if self.mask.test_bit(k) {
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            cf.swap_val(val)
        } else {
            self.mask.set_bit(k);
            let new_cf = CoFree::new(None, Some(val));
            self.values.insert(ix, new_cf);
            None
        }
    }

    #[inline]
    pub fn remove_val(&mut self, k: u8) -> Option<V> {
        if self.mask.test_bit(k) {
            let ix = self.mask.index_of(k) as usize;

            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            let result = cf.take_val();

            if !cf.has_rec() {
                self.mask.clear_bit(k);
                self.values.remove(ix);
            }
            result
        } else {
            None
        }
    }

    /// Similar in behavior to [set_val], but will join v with the existing value instead of replacing it
    #[inline]
    pub fn join_val_into(&mut self, k: u8, val: V) -> AlgebraicStatus where V: Lattice {
        let ix = self.mask.index_of(k) as usize;
        if self.mask.test_bit(k) {
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            match cf.val_mut() {
                Some(existing_val) => {
                    existing_val.join_into(val)
                }
                None => {
                    cf.set_val(val);
                    AlgebraicStatus::Element
                }
            }
        } else {
            self.mask.set_bit(k);
            let new_cf = CoFree::new(None, Some(val));
            self.values.insert(ix, new_cf);
            AlgebraicStatus::Element
        }
    }

    /// Sets the payload (child node or V) at the specified key.  Should not be used in situations where
    /// a the child or value may already exist at the key
    #[inline]
    pub(crate) fn set_payload_owned(&mut self, k: u8, payload: ValOrChild<V>) {
        match payload {
            ValOrChild::Child(child) => {
                let _ = self.set_child(k, child);
            },
            ValOrChild::Val(val) => {
                let result = self.set_val(k, val);
                debug_assert!(result.is_none()); //For now we don't want to replace existing nodes
            }
        }
    }

    /// Behavior is the same as [set_payload_owned], if the child or value doens't already exist, otherwise
    /// joins the existing entry with the supplied payload
    #[inline]
    pub(crate) fn join_payload_into(&mut self, k: u8, payload: ValOrChild<V>) -> AlgebraicStatus where V: Clone + Lattice {
        match payload {
            ValOrChild::Child(child) => {
                self.join_child_into(k, child)
            },
            ValOrChild::Val(val) => {
                self.join_val_into(k, val)
            }
        }
    }

    /// Internal method to remove a CoFree from the node
    #[inline]
    fn remove(&mut self, k: u8) -> Option<Cf> {
        if self.mask.test_bit(k) {
            let ix = self.mask.index_of(k) as usize;
            let v = self.values.remove(ix);
            self.mask.clear_bit(k);
            return Some(v);
        }
        None
    }

    #[inline]
    fn get(&self, k: u8) -> Option<&Cf> {
        if self.mask.test_bit(k) {
            let ix = self.mask.index_of(k) as usize;
            // println!("pos ix {} {} {:b}", pos, ix, self.mask);
            unsafe { Some(self.values.get_unchecked(ix)) }
        } else {
            None
        }
    }

    #[inline]
    fn get_mut(&mut self, k: u8) -> Option<&mut Cf> {
        if self.mask.test_bit(k) {
            let ix = self.mask.index_of(k) as usize;
            unsafe { Some(self.values.get_unchecked_mut(ix)) }
        } else {
            None
        }
    }

    #[inline]
    fn get_child_mut(&mut self, k: u8) -> Option<&mut TrieNodeODRc<V>> {
        self.get_mut(k).and_then(|cf| cf.rec_mut())
    }

    #[inline]
    pub unsafe fn get_unchecked(&self, k: u8) -> &Cf {
        let ix = self.mask.index_of(k) as usize;
        // println!("pos ix {} {} {:b}", pos, ix, self.mask);
        self.values.get_unchecked(ix)
    }

    #[inline]
    unsafe fn get_unchecked_mut(&mut self, k: u8) -> &mut Cf {
        let ix = self.mask.index_of(k) as usize;
        // println!("pos ix {} {} {:b}", pos, ix, self.mask);
        self.values.get_unchecked_mut(ix)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.mask.is_empty_mask()
    }
    /// Iterates the entries in `self`, calling `func` for each entry
    /// The arguments to `func` are: `func(self, key_byte, n)`, where `n` is the number of times
    /// `func` has been called prior.  This corresponds to index of the `CoFree` in the `values` vec
    #[inline]
    fn for_each_item<F: FnMut(&Self, u8, usize)>(&self, mut func: F) {
        let mut n = 0;
        for i in 0..4 {
            let mut lm = self.mask.0[i];
            while lm != 0 {
                let index = lm.trailing_zeros();

                let key_byte = 64*(i as u8) + (index as u8);
                func(self, key_byte, n);
                n += 1;

                lm ^= 1u64 << index;
            }
        }
    }
}

impl<V: Clone + Send + Sync, Cf: CoFree<V=V>> ByteNode<Cf> where Self: TrieNodeDowncast<V> {

    /// Internal method to subtract nodes of an abstract type from the node
    fn psubtract_abstract(&self, other: &dyn TrieNode<V>) -> AlgebraicResult<TrieNodeODRc<V>> where V: Clone + DistributiveLattice {
        let mut is_identity = true;
        let mut new_node = Self::new();

        //Go over each populated entry in the node
        self.for_each_item(|self_node, key_byte, cf_idx| {
            if other.node_contains_partial_key(&[key_byte]) {
                let cf = unsafe{ self_node.values.get_unchecked(cf_idx) };
                let mut new_cf = Cf::new(None, None);

                //If there is a value at this key_byte, and the other node contains a value, subtract them
                if let Some(self_val) = cf.val() {
                    if let Some(other_val) = other.node_get_val(&[key_byte]) {
                        match self_val.psubtract(other_val) {
                            AlgebraicResult::None => { is_identity = false; },
                            AlgebraicResult::Identity(mask) => {
                                debug_assert_eq!(mask, SELF_IDENT); //subtract is not commutative
                                new_cf.set_val(self_val.clone());
                            },
                            AlgebraicResult::Element(e) => {
                                is_identity = false;
                                new_cf.set_val(e);
                            },
                        }
                    }
                }

                //If there is an onward link, see if there is a matching link in other, and subtract them
                if let Some(self_child) = cf.rec() {
                    let other_child = other.get_node_at_key(&[key_byte]);
                    match other_child.try_borrow() {
                        Some(other_child) => {
                            match self_child.borrow().psubtract_dyn(other_child) {
                                AlgebraicResult::None => { is_identity = false; }
                                AlgebraicResult::Identity(mask) => {
                                    debug_assert_eq!(mask, SELF_IDENT); //subtract is not commutative
                                    new_cf.set_rec(self_child.clone());
                                },
                                AlgebraicResult::Element(e) => {
                                    is_identity = false;
                                    new_cf.set_rec(e);
                                },
                            }
                        },
                        None => {
                            new_cf.set_rec(self_child.clone())
                        }
                    }
                }

                //If we ended up with a value or a link in the CF, insert it into a new node
                if new_cf.has_rec() || new_cf.has_val() {
                    new_node.mask.set_bit(key_byte);
                    new_node.values.push(new_cf);
                }
            } else {
                new_node.mask.set_bit(key_byte);
                let cf = unsafe{ self_node.values.get_unchecked(cf_idx) };
                new_node.values.push(cf.clone());
            }
        });
        if new_node.is_empty() {
            AlgebraicResult::None
        } else {
            if is_identity {
                //NOTE: we end up throwing away a totally formed `new_node` here, but that's a much
                // better outcome than having two copies of the same node in the trie
                AlgebraicResult::Identity(SELF_IDENT)
            } else {
                AlgebraicResult::Element(TrieNodeODRc::new(new_node))
            }
        }
    }

    /// Internal method to restrict using nodes of an abstract type
    fn prestrict_abstract(&self, other: &dyn TrieNode<V>) -> AlgebraicResult<TrieNodeODRc<V>> where V: Clone {
        let mut is_identity = true;
        let mut new_node = Self::new();

        //Go over each populated entry in the node
        self.for_each_item(|self_node, key_byte, cf_idx| {
            if other.node_contains_partial_key(&[key_byte]) {
                let cf = unsafe{ self_node.values.get_unchecked(cf_idx) };

                //If there is a comparable value in other, keep the whole cf
                if let Some(_) = other.node_get_val(&[key_byte]) {
                    new_node.mask.set_bit(key_byte);
                    new_node.values.push(cf.clone());
                } else {

                    //If there is an onward link in the CF and other node, continue the restriction recursively
                    if let Some(self_child) = cf.rec() {
                        let other_child = other.get_node_at_key(&[key_byte]);
                        match other_child.try_borrow() {
                            Some(other_child) => {
                                let mut new_cf = Cf::new(None, None);
                                match self_child.borrow().prestrict_dyn(other_child) {
                                    AlgebraicResult::None => { is_identity = false; }
                                    AlgebraicResult::Identity(mask) => {
                                        debug_assert_eq!(mask, SELF_IDENT); //restrict is not commutative
                                        new_cf.set_rec(self_child.clone());
                                    },
                                    AlgebraicResult::Element(e) => {
                                        is_identity = false;
                                        new_cf.set_rec(e);
                                    },
                                }
                                if new_cf.has_rec() {
                                    new_node.mask.set_bit(key_byte);
                                    new_node.values.push(new_cf);
                                }
                            },
                            None => {
                                is_identity = false;
                            }
                        }
                    }
                }
            } else {
                is_identity = false;
            }
        });
        if new_node.is_empty() {
            AlgebraicResult::None
        } else {
            if is_identity {
                AlgebraicResult::Identity(SELF_IDENT)
            } else {
                AlgebraicResult::Element(TrieNodeODRc::new(new_node))
            }
        }
    }

    /// Merges the entries in the ListNode into the ByteNode
    pub fn merge_from_list_node(&mut self, list_node: &LineListNode<V>) -> AlgebraicStatus where V: Clone + Lattice {
        self.reserve_capacity(2);
        // println!("merge_from_list_node {:?}", self.values.iter().map(|cf| cf.rec()).collect::<Vec<_>>());

        let slot0_status = if list_node.is_used::<0>() {
            let key = unsafe{ list_node.key_unchecked::<0>() };
            let payload = list_node.clone_payload::<0>().unwrap();
            if key.len() > 1 {
                let mut child_node = TrieNodeODRc::new(LineListNode::<V>::new());
                // println!("lln0 {:?}", child_node);
                unsafe{ (child_node.as_ptr() as *const LineListNode<V>).cast_mut().as_mut().unwrap().set_payload_owned::<0>(&key[1..], payload); }
                self.join_child_into(key[0], child_node)
            } else {
                self.join_payload_into(key[0], payload)
            }
        } else {
            AlgebraicStatus::None
        };

        let slot1_status = if list_node.is_used::<1>() {
            let key = unsafe{ list_node.key_unchecked::<1>() };
            let payload = list_node.clone_payload::<1>().unwrap();
            if key.len() > 1 {
                let mut child_node = TrieNodeODRc::new(LineListNode::<V>::new());
                // println!("lln1 {:?}", child_node);
                unsafe{ (child_node.as_ptr() as *const LineListNode<V>).cast_mut().as_mut().unwrap().set_payload_owned::<0>(&key[1..], payload); }
                self.join_child_into(key[0], child_node)
            } else {
                self.join_payload_into(key[0], payload)
            }
        } else {
            AlgebraicStatus::None
        };

        //Note: (true, true) makes sense in the context of a join implementation because when the rec_status or
        // val_status is None, it can only have gotten that way because the respective field was already None.
        // This is because Join will never convert Some into None, but this logic isn't portable to other ops
        slot0_status.merge(slot1_status, true, true)
    }
}

impl<V: Clone + Send + Sync> CellByteNode<V> {

    /// Ensures that a CoFree exists for the specified key, and returns a reference to the node and
    /// value option
    ///
    /// This enables a WriteZipper to modify a specific CoFree without touching the DenseByteNode
    /// that contains it, and therefore multiple WriteZippers can be rooted at the same parent, so
    /// long as the first byte of each path is unique
    #[inline]
    pub(crate) fn prepare_cf(&mut self, k: u8) -> (&mut TrieNodeODRc<V>, &mut Option<V>) {
        match self.mask.test_bit(k) {
            true => {},
            false => {
                let ix = self.mask.index_of(k) as usize;
                self.mask.set_bit(k);
                let new_cf = CellCoFree::new(None, None);
                self.values.insert(ix, new_cf);
            }
        }
        let cf = self.get_mut(k).unwrap();
        let (rec, val) = cf.both_mut_refs();
        let rec = match rec {
            Some(rec) => rec,
            None => {
                *rec = Some(TrieNodeODRc::new(crate::empty_node::EmptyNode));
                rec.as_mut().unwrap()
            }
        };
        (rec, val)
    }
}

// impl<V: Clone + Send + Sync> DenseByteNode<V>
impl<V: Clone + Send + Sync, Cf: CoFree<V=V>> ByteNode<Cf>
    where Self: TrieNodeDowncast<V>
{
    /// Internal method to recursively create all parents to support a value or branch at a given path
    #[cfg(feature = "all_dense_nodes")]
    #[inline]
    pub(crate) fn create_parent_path(&mut self, path: &[u8]) -> &mut DenseByteNode<V> {
        let new_node = DenseByteNode::new();
        self.set_child(path[0], TrieNodeODRc::new(new_node));
        let mut cur = self.get_child_mut(path[0]).unwrap().make_mut().as_tagged_mut().into_dense().unwrap();
        for i in 1..path.len() - 1 {
            let new_node = DenseByteNode::new();
            cur.set_child(path[i], TrieNodeODRc::new(new_node));
            cur = cur.get_child_mut(path[i]).unwrap().make_mut().as_tagged_mut().into_dense().unwrap();
        }
        cur
    }
    #[cfg(feature = "bridge_nodes")]
    /// Adds a payload (value or CF link) to a given long key, creating BridgeNodes along the way.  If the
    /// value of onward link already exists at the specified byte, it will be replaced
    pub(crate) fn add_payload(&mut self, key: &[u8], is_child: bool, payload: ValOrChildUnion<V>) {
        debug_assert!(key.len() > 0);
        //DenseByteNodes hold one byte keys, so if the key is more than 1 byte we need to
        // make an intermediate node to hold the rest of the key
        if key.len() > 1 {
            let bridge_node = crate::bridge_node::BridgeNode::new(&key[1..], is_child, payload);
            self.set_child(key[0], TrieNodeODRc::new(bridge_node));
        } else {
            if is_child {
                let child_node = unsafe{ payload.into_child() };
                self.set_child(key[0], child_node);
            } else {
                let val = unsafe{ payload.into_val() };
                self.set_val(key[0], val);
            }
        }
    }
    #[cfg(feature = "bridge_nodes")]
    pub(crate) fn merge_payload(&mut self, key: &[u8], is_child: bool, payload: ValOrChildUnion<V>) where V: Lattice {
        debug_assert!(key.len() > 0);
        //DenseByteNodes hold one byte keys, so if the key is more than 1 byte we need to
        // make an intermediate node to hold the rest of the key
        if key.len() > 1 {
            let bridge_node = crate::bridge_node::BridgeNode::new(&key[1..], is_child, payload);
            self.join_child_into(key[0], TrieNodeODRc::new(bridge_node));
        } else {
            if is_child {
                let child_node = unsafe{ payload.into_child() };
                self.join_child_into(key[0], child_node);
            } else {
                let val = unsafe{ payload.into_val() };
                self.join_val_into(key[0], val);
            }
        }
    }
}

impl<V: Clone + Send + Sync, Cf: CoFree<V=V>> TrieNode<V> for ByteNode<Cf>
    where ByteNode<Cf>: TrieNodeDowncast<V>
{
    #[inline(always)]
    fn node_key_overlap(&self, key: &[u8]) -> usize {
        if self.mask.test_bit(key[0]) {
            1
        } else {
            0
        }
    }
    #[inline(always)]
    fn node_contains_partial_key(&self, key: &[u8]) -> bool {
        debug_assert!(key.len() > 0);
        if key.len() == 1 {
            self.mask.test_bit(key[0])
        } else {
            false
        }
    }
    #[inline(always)]
    fn node_get_child(&self, key: &[u8]) -> Option<(usize, &TrieNodeODRc<V>)> {
        self.get(key[0]).and_then(|cf|
            cf.rec().map(|child_node| {
                (1, child_node)
            })
        )
    }
    //GOAT, Deprecated node_get_child_and_val_mut
    // fn node_get_child_and_val_mut(&mut self, key: &[u8]) -> Option<(usize, &mut TrieNodeODRc<V>, &mut Option<V>)> {
    //     self.get_mut(key[0]).and_then(|cf|
    //         if cf.has_rec() || cf.has_val() {
    //             let (rec, val) = cf.both_mut_refs();
    //             rec.as_mut().map(|rec| (1, rec, val))
    //         } else {
    //             None
    //         }
    //     )
    // }
    fn node_get_child_mut(&mut self, key: &[u8]) -> Option<(usize, &mut TrieNodeODRc<V>)> {
        debug_assert!(key.len() > 0);
        self.get_child_mut(key[0]).map(|child_node_ptr| {
            (1, child_node_ptr)
        })
    }
    //GOAT, we probably don't need this interface, although it is fully implemented and working
    // fn node_contains_children_exclusive(&self, keys: &[&[u8]]) -> bool {
    //     let mut pos = 0;
    //     for (byte, cf) in self.mask.byte_mask_iter().zip(self.values.iter()) {
    //         if cf.rec().is_some() {
    //             match keys[pos..].binary_search(&&[byte][..]) {
    //                 Ok(new_pos) => { pos = new_pos+1; },
    //                 Err(_) => return false
    //             };
    //         }
    //     }
    //     true
    // }
    fn node_replace_child(&mut self, key: &[u8], new_node: TrieNodeODRc<V>) -> &mut dyn TrieNode<V> {
        debug_assert!(key.len() == 1);
        let cf = self.get_mut(key[0]).unwrap();
        *cf.rec_mut().unwrap() = new_node;
        cf.rec_mut().unwrap().make_mut()
    }
    fn node_get_payloads<'node, 'res>(&'node self, keys: &[(&[u8], bool)], results: &'res mut [(usize, PayloadRef<'node, V>)]) -> bool {
        //DISCUSSION: This function appears overly complicated primarily because it needs to track
        // whether or not a both the val and the rec each cofree are requested, but we don't have a bitmask
        // in advance that records vals and rec links separately.  Since we don't want nested loops, we leverage
        // the fact that a rec must be requested before a val, to stash the val for the next trip through the
        // loop.  The loop body therefore is an annoying state-machine.  But at least it's not that much code.

        //Becomes true if only half of `(Some, Some)` CoFree is requested, without requesting the other half
        // This flag never gets unset once it gets set
        let mut unrequested_cofree_half = false;
        //Temporary state that bridges across multiple requests into a `(Some, Some)` CoFree, by holding the
        // val until it's requested, leveraging the fact that values are requested after rec links
        let mut stashed_val: Option<&V> = None;
        //Tracks whether the current CoFree's val has been taken.  So, `last_byte` toggles to `Some` and stays
        // at `Some` until we move onto a different CF, while `stashed_val` toggles to `Some`, and toggles back
        // as soon as the value is requested.
        let mut last_byte: Option<u8> = None;
        //Tracks which CoFrees have yet to be requested from the node
        let mut requested_mask = ByteMask::from(self.mask);

        debug_assert!(results.len() >= keys.len());
        for ((key, expect_val), (result_key_len, payload_ref)) in keys.into_iter().zip(results.iter_mut()) {
            if key.len() > 0 {
                let byte = key[0];

                //Check to see if we had a Val from the CoFree that we aren't going to request
                match &last_byte {
                    Some(prev_byte) => {
                        if byte != *prev_byte {
                            if stashed_val.is_some() {
                                unrequested_cofree_half = true;
                            }
                            stashed_val = None;
                            last_byte = None;
                        }
                    },
                    None => {}
                }

                //Check to see if this trip through the loop is the request for the stashed val
                match stashed_val {
                    Some(val) => {
                        if key.len() == 1 && *expect_val {
                            *result_key_len = 1;
                            *payload_ref = PayloadRef::Val(val);
                            stashed_val = None;
                            continue;
                        }
                    },
                    None => {}
                }

                requested_mask.clear_bit(byte);
                match self.get(byte) {
                    Some(cf) => {
                        //A key longer than 1 byte or an explicit request for a rec link can be answered with a Child
                        if key.len() > 1 || !*expect_val {
                            match cf.rec() {
                                Some(rec) => {
                                    *result_key_len = 1;
                                    *payload_ref = PayloadRef::Child(rec);
                                },
                                None => {}
                            }
                        }
                        match cf.val() {
                            Some(val) => {
                                //Answer an explicit request for this val, or stash the val for 
                                if key.len() == 1 && *expect_val {
                                    debug_assert!(stashed_val.is_none());
                                    *result_key_len = 1;
                                    *payload_ref = PayloadRef::Val(val);
                                } else {
                                    if last_byte.is_none() {
                                        stashed_val = Some(val);
                                        last_byte = Some(byte);
                                    }
                                }
                            },
                            None => {}
                        }
                    },
                    None => {}
                }
            }
        }

        !unrequested_cofree_half && stashed_val.is_none() && requested_mask.is_empty_mask()
    }
    fn node_contains_val(&self, key: &[u8]) -> bool {
        if key.len() == 1 {
            match self.get(key[0]) {
                Some(cf) => cf.has_val(),
                None => false
            }
        } else {
            false
        }
    }
    //GOAT, we probably don't need this interface, although it is fully implemented and working
    // fn node_contains_vals_exclusive(&self, keys: &[&[u8]]) -> bool {
    //     let mut pos = 0;
    //     for (byte, cf) in self.mask.byte_mask_iter().zip(self.values.iter()) {
    //         if cf.val().is_some() {
    //             match keys[pos..].binary_search(&&[byte][..]) {
    //                 Ok(new_pos) => { pos = new_pos+1; },
    //                 Err(_) => return false
    //             };
    //         }
    //     }
    //     true
    // }
    fn node_get_val(&self, key: &[u8]) -> Option<&V> {
        if key.len() == 1 {
            self.get(key[0]).and_then(|cf| cf.val() )
        } else {
            None
        }
    }
    fn node_get_val_mut(&mut self, key: &[u8]) -> Option<&mut V> {
        if key.len() == 1 {
            self.get_mut(key[0]).and_then(|cf| cf.val_mut() )
        } else {
            None
        }
    }
    fn node_set_val(&mut self, key: &[u8], val: V) -> Result<(Option<V>, bool), TrieNodeODRc<V>> {
        #[cfg(not(feature = "all_dense_nodes"))]
        {
            //Split a new node to hold everything after the first byte of the key
            if key.len() > 1 {
                #[cfg(not(feature = "bridge_nodes"))]
                {
                    let mut child = crate::line_list_node::LineListNode::new();
                    child.node_set_val(&key[1..], val).unwrap_or_else(|_| panic!());
                    self.set_child(key[0], TrieNodeODRc::new(child));
                }
                #[cfg(feature = "bridge_nodes")]
                {
                    let child = crate::bridge_node::BridgeNode::new(&key[1..], false, val.into());
                    self.set_child(key[0], TrieNodeODRc::new(child));
                }
                Ok((None, true))
            } else {
                Ok((self.set_val(key[0], val), false))
            }
        }

        #[cfg(feature = "all_dense_nodes")]
        {
            if key.len() > 1 {
                let last_node = self.create_parent_path(key);
                Ok((last_node.set_val(key[key.len()-1], val), true))
            } else {
                Ok((self.set_val(key[key.len()-1], val), false))
            }
        }
    }
    fn node_remove_val(&mut self, key: &[u8]) -> Option<V> {
        if key.len() == 1 {
            self.remove_val(key[0])
        } else {
            None
        }
    }
    fn node_set_branch(&mut self, key: &[u8], new_node: TrieNodeODRc<V>) -> Result<bool, TrieNodeODRc<V>> {
        debug_assert!(key.len() > 0);
        #[cfg(not(feature = "all_dense_nodes"))]
        {
            //Make a new ListNode to hold everything after the first byte of the key
            if key.len() > 1 {
                #[cfg(not(feature = "bridge_nodes"))]
                {
                    let mut child = crate::line_list_node::LineListNode::new();
                    child.node_set_branch(&key[1..], new_node).unwrap_or_else(|_| panic!());
                    self.set_child(key[0], TrieNodeODRc::new(child));
                }
                #[cfg(feature = "bridge_nodes")]
                {
                    let child = crate::bridge_node::BridgeNode::new(&key[1..], true, new_node.into());
                    self.set_child(key[0], TrieNodeODRc::new(child));
                }
                Ok(true)
            } else {
                self.set_child(key[0], new_node);
                Ok(false)
            }
        }

        #[cfg(feature = "all_dense_nodes")]
        {
            if key.len() > 1 {
                let last_node = self.create_parent_path(key);
                last_node.set_child(key[key.len()-1], new_node);
                Ok(true)
            } else {
                self.set_child(key[key.len()-1], new_node);
                Ok(false)
            }
        }
    }
    fn node_remove_all_branches(&mut self, key: &[u8]) -> bool {
        if key.len() > 1 {
            return false;
        }
        debug_assert_eq!(key.len(), 1);
        let k = key[0];
        if self.mask.test_bit(k) {
            let ix = self.mask.index_of(k) as usize;
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            match (cf.has_rec(), cf.has_val()) {
                (true, true) => {
                    cf.set_rec_option(None);
                    true
                },
                (true, false) => {
                    self.values.remove(ix);
                    self.mask.clear_bit(k);
                    true
                },
                (false, _) => {
                    false
                },
            }
        } else {
            false
        }
    }
    fn node_is_empty(&self) -> bool {
        self.values.len() == 0
    }
    #[inline(always)]
    fn new_iter_token(&self) -> u128 {
        self.mask.0[0] as u128
    }
    #[inline(always)]
    fn iter_token_for_path(&self, key: &[u8]) -> u128 {
        if key.len() != 1 {
            self.new_iter_token()
        } else {
            let k = *unsafe{ key.get_unchecked(0) } as usize;
            let idx = (k & 0b11000000) >> 6;
            let bit_i = k & 0b00111111;
            debug_assert!(idx < 4);
            let mask: u64 = if bit_i+1 < 64 {
                (0xFFFFFFFFFFFFFFFF << bit_i+1) & unsafe{ self.mask.0.get_unchecked(idx) }
            } else {
                0
            };
            ((idx as u128) << 64) | (mask as u128)
        }
    }
    #[inline(always)]
    fn next_items(&self, token: u128) -> (u128, &[u8], Option<&TrieNodeODRc<V>>, Option<&V>) {
        let mut i = (token >> 64) as u8;
        let mut w = token as u64;
        loop {
            if w != 0 {
                let wi = w.trailing_zeros() as u8;
                w ^= 1u64 << wi;
                let k = i*64 + wi;

                let new_token = ((i as u128) << 64) | (w as u128);
                let cf = unsafe{ self.get_unchecked(k) };
                let k = k as usize;
                return (new_token, &ALL_BYTES[k..=k], cf.rec(), cf.val())

            } else if i < 3 {
                i += 1;

                w = unsafe { *self.mask.0.get_unchecked(i as usize) };
            } else {
                return (NODE_ITER_FINISHED, &[], None, None)
            }
        }
    }
    fn node_val_count(&self, cache: &mut HashMap<*const dyn TrieNode<V>, usize>) -> usize {
        //Discussion: These two implementations do the same thing but with a slightly different ordering of
        // the operations.  In `all_dense_nodes`, the "Branchy" impl wins.  But in a mixed-node setting, the
        // IMPL B is the winner.  My suspicion is that the ListNode's heavily branching structure leads to
        // underutilization elsewhere in the CPU so we get better instruction parallelism with IMPL B.

        //IMPL A "Branchy"
        // let mut result = 0;
        // for cf in self.values.iter() {
        //     if cf.value.is_some() {
        //         result += 1;
        //     }
        //     match &cf.rec {
        //         Some(rec) => result += rec.borrow().node_subtree_len(),
        //         None => {}
        //     }
        // }
        // result

        //IMPL B "Arithmetic"
        return self.values.iter().rfold(0, |t, cf| {
            t + cf.has_val() as usize + cf.rec().map(|r| val_count_below_node(r, cache)).unwrap_or(0)
        });
    }
    #[cfg(feature = "counters")]
    fn item_count(&self) -> usize {
        let mut cnt = 0;
        for cf in self.values.iter() {
            cnt += cf.has_rec() as usize;
            cnt += cf.has_val() as usize;
        }
        cnt
    }
    fn node_first_val_depth_along_key(&self, key: &[u8]) -> Option<usize> {
        debug_assert!(key.len() > 0);
        self.get(key[0]).and_then(|cf| {
            if cf.has_val() {
                Some(0)
            } else {
                None
            }
        })
    }
    fn nth_child_from_key(&self, key: &[u8], n: usize) -> (Option<u8>, Option<&dyn TrieNode<V>>) {
        if key.len() > 0 {
            return (None, None)
        }

        // NOTE: This code was originally written to support indexing from the front or the back of
        // the list.  However, this capability can't be exposed in the higher-level interface because
        // index stability can't be guaranteed in many (any) node implementations, and index ordering
        // guarantees without cardinality don't provide muuch use and often come with an unnecessary cost
        //
        // However, This capability may be used again in the future, so I made "FORWARD" a const instead
        // of an argument for now.
        //
        // If 'forward == true` then `n` counts forward from the first element, oterwise it counts
        // backward from the last
        const FORWARD: bool = true;

        if n >= self.values.len() {
            return (None, None)
        }

        if FORWARD {
            (self.mask.indexed_bit::<FORWARD>(n), self.values[n].rec().map(|cf| &*cf.borrow()))
        } else {
            let idx = self.values.len() - n - 1;
            (self.mask.indexed_bit::<FORWARD>(n), self.values[idx].rec().map(|cf| &*cf.borrow()))
        }
    }

    fn first_child_from_key(&self, key: &[u8]) -> (Option<&[u8]>, Option<&dyn TrieNode<V>>) {
        debug_assert_eq!(key.len(), 0);
        debug_assert!(self.values.len() > 0);

        let cf = unsafe{ self.values.get_unchecked(0) };
        let prefix = self.mask.indexed_bit::<true>(0).unwrap() as usize;
        (Some(&ALL_BYTES[prefix..=prefix]), cf.rec().map(|cf| &*cf.borrow()))
    }

    fn node_remove_unmasked_branches(&mut self, key: &[u8], mask: ByteMask) {
        debug_assert!(key.len() == 0);
        // in the future we can use `drain_filter`, but that's experimental
        let mut lead = 0;
        let mut differs = false;
        let mut c = 0;
        let mvalues = self.values.as_ptr().cast_mut();

        unsafe {
            for i in 0..4 {
                let mut lm = self.mask.0[i];
                while lm != 0 {
                    let index = lm.trailing_zeros();
                    if ((1u64 << index) & mask.0[i]) != 0 {
                        if differs { ptr::copy_nonoverlapping(mvalues.add(c), mvalues.add(lead), 1); }
                        lead += 1;
                    } else {
                        ptr::drop_in_place(mvalues.add(c));
                        differs = true;
                    }
                    lm ^= 1u64 << index;
                    c += 1;
                }
            }

            self.values.set_len(lead);
        }
        self.mask &= mask;
    }

    #[inline(always)]
    fn node_branches_mask(&self, key: &[u8]) -> ByteMask {
        match key.len() {
            0 => self.mask,
            _ => {
                //There are two ways we could get a length >= 1 key passed in. 1. The entry is a lone value (no children in the CF) or 2. The entry doesn't exist.  Either way, there are no onward child paths
                debug_assert!(self.get(key[0]).and_then(|cf| cf.rec()).is_none());
                ByteMask::EMPTY
            },
        }
    }

    #[inline(always)]
    fn count_branches(&self, key: &[u8]) -> usize {
        match key.len() {
            0 => self.values.len(),
            _ => {
                //There are two ways we could get a length >=1 key passed in. 1. The entry is a lone value (no children in the CF) or 2. The entry doesn't exist.  Either way, there are no onward child paths
                debug_assert!(self.get(key[0]).and_then(|cf| cf.rec()).is_none());
                0
            }
        }
    }

    #[inline(always)]
    fn is_leaf(&self, key: &[u8]) -> bool {
        match key.len() {
            0 => self.values.len() == 0,
            1 => self.get(key[0]).map(|cf| !cf.has_rec()).unwrap_or(true),
            _ => true
        }
    }

    fn prior_branch_key(&self, key: &[u8]) -> &[u8] {
        debug_assert!(key.len() >= 1);
        if key.len() == 1 {
            &[]
        } else {
            let k = key[0] as usize;
            &ALL_BYTES[k..=k]
        }
    }

    fn get_sibling_of_child(&self, key: &[u8], next: bool) -> (Option<u8>, Option<&dyn TrieNode<V>>) {
        if key.len() != 1 {
            return (None, None)
        }
        let k = key[0];
        let mut mask_i = ((k & 0b11000000) >> 6) as usize;
        let bit_i = k & 0b00111111;
        // println!("k {k}");

        let mut n = bit_sibling(bit_i, self.mask.0[mask_i], !next);
        // println!("{} {bit_i} {mask_i}", n == bit_i);
        if n == bit_i { // outside of word
            loop {
                if next { mask_i += 1 } else { mask_i -= 1 };
                if !(mask_i < 4) { return (None, None) }
                if self.mask.0[mask_i] == 0 { continue }
                n = self.mask.0[mask_i].trailing_zeros() as u8; break;
            }
        }

        // println!("{} {bit_i} {mask_i}", n == bit_i);
        // println!("{:?}", parent.items().map(|(k, _)| k).collect::<Vec<_>>());
        let sibling_key_char = n | ((mask_i << 6) as u8);
        // println!("candidate {}", sk);
        debug_assert!(self.mask.test_bit(sibling_key_char));
        let cf = unsafe{ self.get_unchecked(sibling_key_char) };
        (Some(sibling_key_char), cf.rec().map(|node| &*node.borrow()))
    }

    fn get_node_at_key(&self, key: &[u8]) -> AbstractNodeRef<V> {
        if key.len() < 2 {
            if key.len() == 0 {
                if !self.node_is_empty() {
                    AbstractNodeRef::BorrowedDyn(self)
                } else {
                    AbstractNodeRef::None
                }
            } else {
                match self.get(key[0]).and_then(|cf| cf.rec()) {
                    Some(link) => AbstractNodeRef::BorrowedRc(link),
                    None => AbstractNodeRef::None
                }
            }
        } else {
            AbstractNodeRef::None
        }
    }

    fn take_node_at_key(&mut self, key: &[u8]) -> Option<TrieNodeODRc<V>> {
        if key.len() < 2 {
            debug_assert!(key.len() == 1);
            match self.get_mut(key[0]) {
                Some(cf) => cf.take_rec(),
                None => None
            }
        } else {
            None
        }
    }

    fn pjoin_dyn(&self, other: &dyn TrieNode<V>) -> AlgebraicResult<TrieNodeODRc<V>> where V: Lattice {
        let other_node = other.as_tagged();
        match other_node {
            TaggedNodeRef::DenseByteNode(other_dense_node) => {
                self.pjoin(other_dense_node).map(|new_node| TrieNodeODRc::new(new_node))
            },
            TaggedNodeRef::LineListNode(other_list_node) => {
                let mut new_node = unsafe { std::ptr::read(self) };
                let status = new_node.merge_from_list_node(other_list_node);
                AlgebraicResult::from_status(status, || TrieNodeODRc::new(new_node))
            },
            #[cfg(feature = "bridge_nodes")]
            TaggedNodeRef::BridgeNode(other_bridge_node) => {
                let mut new_node = self.clone();
                debug_assert!(!other_bridge_node.is_empty());
                new_node.merge_payload(other_bridge_node.key(), other_bridge_node.is_child_ptr(), other_bridge_node.clone_payload());
                TrieNodeODRc::new(new_node)
            },
            TaggedNodeRef::TinyRefNode(tiny_node) => {
                tiny_node.pjoin_dyn(self)
            },
            TaggedNodeRef::CellByteNode(other_byte_node) => {
                self.pjoin(other_byte_node).map(|new_node| TrieNodeODRc::new(new_node))
            },
            TaggedNodeRef::EmptyNode => {
                AlgebraicResult::Identity(SELF_IDENT)
            }
        }
    }

    fn join_into_dyn(&mut self, mut other: TrieNodeODRc<V>) -> (AlgebraicStatus, Result<(), TrieNodeODRc<V>>) where V: Lattice {
        // println!("join_into_dyn Byte {:?}", other);
        let other_node = other.make_mut().as_tagged_mut();
        let status = match other_node {
            TaggedNodeRefMut::DenseByteNode(other_dense_node) => {
                self.join_into(core::mem::take(other_dense_node))
            },
            TaggedNodeRefMut::LineListNode(other_list_node) => {
                //GOAT, optimization opportunity to take the contents from the list, rather than cloning
                // them, to turn around and drop the ListNode and free them / decrement the refcounts
                self.merge_from_list_node(other_list_node)
            },
            #[cfg(feature = "bridge_nodes")]
            TaggedNodeRefMut::BridgeNode(_other_bridge_node) => {
                unimplemented!()
            },
            TaggedNodeRefMut::CellByteNode(other_byte_node) => {
                self.join_into(core::mem::take(other_byte_node))
            },
            TaggedNodeRefMut::Unsupported => unreachable!()
        };
        (status, Ok(()))
    }

    fn drop_head_dyn(&mut self, byte_cnt: usize) -> Option<TrieNodeODRc<V>> where V: Lattice {
        match self.values.len() {
            0 => { None },
            1 => {
                //WARNING: Don't be tempted to swap the node itself with its first child.  This feels like it
                // might be an optimization, but it would be a memory leak because the other node will now
                // hold an Rc to itself.
                match self.values.pop().unwrap().into_rec() {
                    Some(mut child) => {
                        if byte_cnt > 1 {
                            child.make_mut().drop_head_dyn(byte_cnt-1)
                        } else {
                            Some(child)
                        }
                    },
                    None => None
                }
            },
            _ => {
                let mut new_node = Self::new();
                while let Some(cf) = self.values.pop() {
                    let child = if byte_cnt > 1 {
                        cf.into_rec().and_then(|mut child| child.make_mut().drop_head_dyn(byte_cnt-1))
                    } else {
                        cf.into_rec()
                    };
                    match child {
                        Some(child) => {
                            let (_status, result) = new_node.join_into_dyn(child);
                            debug_assert!(result.is_ok());
                        },
                        None => {}
                    }
                }

                if !new_node.is_empty() {
                    Some(TrieNodeODRc::new(new_node))
                } else {
                    None
                }
            }
        }
    }

    fn pmeet_dyn(&self, other: &dyn TrieNode<V>) -> AlgebraicResult<TrieNodeODRc<V>> where V: Lattice {
        let other_node = other.as_tagged();
        match other_node {
            TaggedNodeRef::DenseByteNode(other_dense_node) => {
                self.pmeet(other_dense_node).map(|new_node| TrieNodeODRc::new(new_node))
            },
            TaggedNodeRef::LineListNode(other_list_node) => {
                other_list_node.pmeet_dyn(self).invert_identity()
            },
            #[cfg(feature = "bridge_nodes")]
            TaggedNodeRef::BridgeNode(other_bridge_node) => {
                other_bridge_node.pmeet_dyn(self).invert_identity()
            },
            TaggedNodeRef::TinyRefNode(tiny_node) => {
                tiny_node.pmeet_dyn(self).invert_identity()
            },
            TaggedNodeRef::CellByteNode(other_byte_node) => {
                self.pmeet(other_byte_node).map(|new_node| TrieNodeODRc::new(new_node))
            },
            TaggedNodeRef::EmptyNode => AlgebraicResult::None
        }
    }

    fn psubtract_dyn(&self, other: &dyn TrieNode<V>) -> AlgebraicResult<TrieNodeODRc<V>> where V: DistributiveLattice {
        let other_node = other.as_tagged();
        match other_node {
            TaggedNodeRef::DenseByteNode(other_dense_node) => {
                self.psubtract(other_dense_node).map(|new_node| TrieNodeODRc::new(new_node))
            },
            TaggedNodeRef::LineListNode(other_list_node) => {
                self.psubtract_abstract(other_list_node)
            },
            #[cfg(feature = "bridge_nodes")]
            TaggedNodeRef::BridgeNode(other_bridge_node) => {
                self.psubtract_abstract(other_bridge_node)
            },
            TaggedNodeRef::TinyRefNode(tiny_node) => {
                self.psubtract_abstract(tiny_node)
            },
            TaggedNodeRef::CellByteNode(other_byte_node) => {
                self.psubtract(other_byte_node).map(|new_node| TrieNodeODRc::new(new_node))
            },
            TaggedNodeRef::EmptyNode => AlgebraicResult::Identity(SELF_IDENT),
        }
    }

    fn prestrict_dyn(&self, other: &dyn TrieNode<V>) -> AlgebraicResult<TrieNodeODRc<V>> {
        let other_node = other.as_tagged();
        match other_node {
            TaggedNodeRef::DenseByteNode(other_dense_node) => {
                self.prestrict(other_dense_node).map(|node| TrieNodeODRc::new(node))
            },
            TaggedNodeRef::LineListNode(other_list_node) => {
                self.prestrict_abstract(other_list_node)
            },
            #[cfg(feature = "bridge_nodes")]
            TaggedNodeRef::BridgeNode(other_bridge_node) => {
                self.prestrict_abstract(other_bridge_node)
            },
            TaggedNodeRef::TinyRefNode(tiny_node) => {
                self.prestrict_abstract(tiny_node)
            },
            TaggedNodeRef::CellByteNode(other_byte_node) => {
                self.prestrict(other_byte_node).map(|node| TrieNodeODRc::new(node))
            },
            TaggedNodeRef::EmptyNode => AlgebraicResult::None,
        }
    }
    fn clone_self(&self) -> TrieNodeODRc<V> {
        TrieNodeODRc::new(self.clone())
    }
}

impl<V: Clone + Send + Sync> TrieNodeDowncast<V> for ByteNode<OrdinaryCoFree<V>> {
    #[inline(always)]
    fn as_tagged(&self) -> TaggedNodeRef<V> {
        TaggedNodeRef::DenseByteNode(self)
    }
    #[inline(always)]
    fn as_tagged_mut(&mut self) -> TaggedNodeRefMut<V> {
        TaggedNodeRefMut::DenseByteNode(self)
    }
    fn convert_to_cell_node(&mut self) -> TrieNodeODRc<V> {
        let mut replacement_node = CellByteNode::<V>::with_capacity(self.values.len());
        debug_assert_eq!(replacement_node.mask, [0u64; 4]);
        core::mem::swap(&mut replacement_node.mask, &mut self.mask);
        let mut values = Vec::new_in(crate::utils::Leak);
        // let mut values = Vec::with_capacity_in(256, crate::utils::Leak);
        core::mem::swap(&mut values, &mut self.values);
        for cf in values.into_iter() {
            replacement_node.values.push(cf.into())
        }
        TrieNodeODRc::new(replacement_node)
    }
}

impl<V> TrieNodeDowncast<V> for ByteNode<CellCoFree<V>> {
    fn as_tagged(&self) -> TaggedNodeRef<V> {
        TaggedNodeRef::CellByteNode(self)
    }
    fn as_tagged_mut(&mut self) -> TaggedNodeRefMut<V> {
        TaggedNodeRefMut::CellByteNode(self)
    }
    fn convert_to_cell_node(&mut self) -> TrieNodeODRc<V> {
        //Already is a cell_node, and that fact should have been detected before calling this method
        unreachable!()
    }
}

/// returns the position of the next/previous active bit in x
/// if there is no next/previous bit, returns the argument position
/// assumes that pos is active in x
pub(crate) fn bit_sibling(pos: u8, x: u64, next: bool) -> u8 {
    debug_assert_ne!((1u64 << pos) & x, 0);
    if next {
        if pos == 0 { return 0 } // resolves overflow in shift
        let succ = !0u64 >> (64 - pos);
        let m = x & succ;
        if m == 0u64 { pos }
        else { (63 - m.leading_zeros()) as u8 }
    } else {
        let prec = !(!0u64 >> (63 - pos));
        let m = x & prec;
        if m == 0u64 { pos }
        else { m.trailing_zeros() as u8 }
    }
}

pub trait CoFree: Clone + Default + Send + Sync {
    type V;
    fn new(rec: Option<TrieNodeODRc<Self::V>>, val: Option<Self::V>) -> Self;
    fn from_cf<OtherCf: CoFree<V=Self::V>>(cf: OtherCf) -> Self;
    fn rec(&self) -> Option<&TrieNodeODRc<Self::V>>;
    fn has_rec(&self) -> bool;
    fn rec_mut(&mut self) -> Option<&mut TrieNodeODRc<Self::V>>;
    fn take_rec(&mut self) -> Option<TrieNodeODRc<Self::V>>;
    fn into_rec(self) -> Option<TrieNodeODRc<Self::V>>;
    fn set_rec(&mut self, node: TrieNodeODRc<Self::V>);
    fn set_rec_option(&mut self, rec: Option<TrieNodeODRc<Self::V>>);
    fn swap_rec(&mut self, node: TrieNodeODRc<Self::V>) -> Option<TrieNodeODRc<Self::V>>;
    fn val(&self) -> Option<&Self::V>;
    fn has_val(&self) -> bool;
    fn val_mut(&mut self) -> Option<&mut Self::V>;
    fn set_val(&mut self, val: Self::V);
    fn set_val_option(&mut self, val: Option<Self::V>);
    fn swap_val(&mut self, val: Self::V) -> Option<Self::V>;
    fn take_val(&mut self) -> Option<Self::V>;
    fn both_mut(&mut self) -> (Option<&mut TrieNodeODRc<Self::V>>, Option<&mut Self::V>);
    fn into_both(self) -> (Option<TrieNodeODRc<Self::V>>, Option<Self::V>);
}

trait CfShared<OtherCf>: CoFree {
    /// Integrates the results from separate operations on the rec pointer and the value into a single result on
    /// the entire CoFree
    fn combine_algebraic_results(&self, other: &OtherCf, rec: AlgebraicResult<Option<TrieNodeODRc<Self::V>>>, val: AlgebraicResult<Option<Self::V>>) -> AlgebraicResult<Self>;
}

#[derive(Clone, Debug)]
pub struct OrdinaryCoFree<V> {
    rec: Option<TrieNodeODRc<V>>,
    value: Option<V>
}

impl<V> Default for OrdinaryCoFree<V> {
    fn default() -> Self {
        Self {rec: None, value: None}
    }
}

impl<V: Clone + Send + Sync> CoFree for OrdinaryCoFree<V> {
    type V = V;
    fn new(rec: Option<TrieNodeODRc<V>>, val: Option<V>) -> Self {
        Self { rec: rec, value: val, }
    }
    fn from_cf<OtherCf: CoFree<V=Self::V>>(cf: OtherCf) -> Self {
        let (rec, val) = cf.into_both();
        Self { rec: rec, value: val, }
    }
    fn rec(&self) -> Option<&TrieNodeODRc<V>> {
        self.rec.as_ref()
    }
    fn has_rec(&self) -> bool {
        self.rec.is_some()
    }
    fn rec_mut(&mut self) -> Option<&mut TrieNodeODRc<V>> {
        self.rec.as_mut()
    }
    fn take_rec(&mut self) -> Option<TrieNodeODRc<V>> {
        core::mem::take(&mut self.rec)
    }
    fn into_rec(self) -> Option<TrieNodeODRc<V>> {
        self.rec
    }
    fn set_rec(&mut self, node: TrieNodeODRc<V>) {
        self.rec = Some(node)
    }
    fn set_rec_option(&mut self, rec: Option<TrieNodeODRc<V>>) {
        self.rec = rec
    }
    fn swap_rec(&mut self, node: TrieNodeODRc<V>) -> Option<TrieNodeODRc<V>> {
        let mut old_child = Some(node);
        core::mem::swap(&mut old_child, &mut self.rec);
        old_child
    }
    fn val(&self) -> Option<&V> {
        self.value.as_ref()
    }
    fn has_val(&self) -> bool {
        self.value.is_some()
    }
    fn val_mut(&mut self) -> Option<&mut V> {
        self.value.as_mut()
    }
    fn set_val(&mut self, val: V) {
        self.value = Some(val)
    }
    fn set_val_option(&mut self, val: Option<V>) {
        self.value = val
    }
    fn swap_val(&mut self, val: V) -> Option<V> {
        let mut old_val = Some(val);
        core::mem::swap(&mut old_val, &mut self.value);
        old_val
    }
    fn take_val(&mut self) -> Option<V> {
        core::mem::take(&mut self.value)
    }
    fn both_mut(&mut self) -> (Option<&mut TrieNodeODRc<V>>, Option<&mut V>) {
        (self.rec.as_mut(), self.value.as_mut())
    }
    fn into_both(self) -> (Option<TrieNodeODRc<V>>, Option<V>) {
        (self.rec, self.value)
    }
}

use core::cell::UnsafeCell;
use core::pin::Pin;
use std::mem::MaybeUninit;

#[derive(Debug)]
pub struct CellCoFree<V>(Pin<Box<CellCoFreeInsides<V>>>);

impl<V: Clone + Send + Sync> Default for CellCoFree<V> {
    fn default() -> Self {
        Self(Box::pin(CellCoFreeInsides::new(None, None)))
    }
}

impl<V: Clone + Send + Sync> Clone for CellCoFree<V> {
    fn clone(&self) -> Self {
        Self(Box::pin((*self.0).clone()))
    }
}

impl<V: Clone + Send + Sync> From<OrdinaryCoFree<V>> for CellCoFree<V> {
    fn from(cf: OrdinaryCoFree<V>) -> Self {
        let (rec, val) = cf.into_both();
        Self::new(rec, val)
    }
}

impl<V> CellCoFree<V> {
    fn both_mut_refs(&mut self) -> (&mut Option<TrieNodeODRc<V>>, &mut Option<V>) {
        unsafe{ self.0.as_mut().get_unchecked_mut() }.both_mut_refs()
    }
}

impl<V: Clone + Send + Sync> CoFree for CellCoFree<V> {
    type V = V;
    fn new(rec: Option<TrieNodeODRc<V>>, val: Option<V>) -> Self {
        let insides = CellCoFreeInsides::new(rec, val);
        Self(Box::pin(insides))
    }
    fn from_cf<OtherCf: CoFree<V=Self::V>>(cf: OtherCf) -> Self {
        let (rec, val) = cf.into_both();
        Self::new(rec, val)
    }
    fn rec(&self) -> Option<&TrieNodeODRc<V>> {
        self.0.rec()
    }
    fn has_rec(&self) -> bool {
        self.0.has_rec()
    }
    fn rec_mut(&mut self) -> Option<&mut TrieNodeODRc<V>> {
        unsafe{ self.0.as_mut().get_unchecked_mut() }.rec_mut()
    }
    fn take_rec(&mut self) -> Option<TrieNodeODRc<V>> {
        unsafe{ self.0.as_mut().get_unchecked_mut() }.take_rec()
    }
    fn into_rec(self) -> Option<TrieNodeODRc<V>> {
        unsafe{ Pin::into_inner_unchecked(self.0) }.take_rec()
    }
    fn set_rec(&mut self, node: TrieNodeODRc<V>) {
        unsafe{ self.0.as_mut().get_unchecked_mut() }.set_rec(node)
    }
    fn set_rec_option(&mut self, rec: Option<TrieNodeODRc<V>>) {
        unsafe{ self.0.as_mut().get_unchecked_mut() }.set_rec_option(rec)
    }
    fn swap_rec(&mut self, node: TrieNodeODRc<V>) -> Option<TrieNodeODRc<V>> {
        unsafe{ self.0.as_mut().get_unchecked_mut() }.swap_rec(node)
    }
    fn val(&self) -> Option<&V> {
        self.0.val()
    }
    fn has_val(&self) -> bool {
        self.0.has_val()
    }
    fn val_mut(&mut self) -> Option<&mut V> {
        unsafe{ self.0.as_mut().get_unchecked_mut() }.val_mut()
    }
    fn take_val(&mut self) -> Option<V> {
        unsafe{ self.0.as_mut().get_unchecked_mut() }.take_val()
    }
    fn set_val(&mut self, val: V) {
        unsafe{ self.0.as_mut().get_unchecked_mut() }.set_val(val)
    }
    fn set_val_option(&mut self, val: Option<V>) {
        unsafe{ self.0.as_mut().get_unchecked_mut() }.set_val_option(val)
    }
    fn swap_val(&mut self, val: V) -> Option<V> {
        unsafe{ self.0.as_mut().get_unchecked_mut() }.swap_val(val)
    }
    fn both_mut(&mut self) -> (Option<&mut TrieNodeODRc<V>>, Option<&mut V>) {
        unsafe{ self.0.as_mut().get_unchecked_mut() }.both_mut()
    }
    fn into_both(self) -> (Option<TrieNodeODRc<V>>, Option<V>) {
        unsafe{ Pin::into_inner_unchecked(self.0) }.into_both()
    }
}

#[derive(Debug)]
struct CellCoFreeInsides<V> {
    rec: UnsafeCell<Option<TrieNodeODRc<V>>>,
    value: UnsafeCell<Option<V>>
}

impl<V> Default for CellCoFreeInsides<V> {
    fn default() -> Self {
        Self {rec: UnsafeCell::new(None), value: UnsafeCell::new(None)}
    }
}

unsafe impl<V: Send + Sync> Send for CellCoFreeInsides<V> {}
unsafe impl<V: Send + Sync> Sync for CellCoFreeInsides<V> {}

impl<V: Clone + Send + Sync> Clone for CellCoFreeInsides<V> {
    fn clone(&self) -> Self {
        Self {
            rec: UnsafeCell::new(self.rec().cloned()),
            value: UnsafeCell::new(self.val().cloned()),
        }
    }
}

impl<V> CellCoFreeInsides<V> {
    fn both_mut_refs(&mut self) -> (&mut Option<TrieNodeODRc<V>>, &mut Option<V>) {
        let rec = unsafe{ &mut *self.rec.get() };
        let val = unsafe{ &mut *self.value.get() };
        (rec, val)
    }
}

impl<V: Clone + Send + Sync> CoFree for CellCoFreeInsides<V> {
    type V = V;
    fn new(rec: Option<TrieNodeODRc<V>>, val: Option<V>) -> Self {
        Self {
            rec: UnsafeCell::new(rec),
            value: UnsafeCell::new(val)
        }
    }
    fn from_cf<OtherCf: CoFree<V=Self::V>>(cf: OtherCf) -> Self {
        let (rec, val) = cf.into_both();
        Self::new(rec, val)
    }
    fn rec(&self) -> Option<&TrieNodeODRc<V>> {
        unsafe{ &*self.rec.get() }.as_ref()
    }
    fn has_rec(&self) -> bool {
        unsafe{ &*self.rec.get() }.is_some()
    }
    fn rec_mut(&mut self) -> Option<&mut TrieNodeODRc<V>> {
        unsafe{ &mut *self.rec.get() }.as_mut()
    }
    fn take_rec(&mut self) -> Option<TrieNodeODRc<V>> {
        core::mem::take(&mut self.rec).into_inner()
    }
    fn into_rec(self) -> Option<TrieNodeODRc<V>> {
        self.rec.into_inner()
    }
    fn set_rec(&mut self, node: TrieNodeODRc<V>) {
        self.rec = UnsafeCell::new(Some(node))
    }
    fn set_rec_option(&mut self, rec: Option<TrieNodeODRc<V>>) {
        self.rec = UnsafeCell::new(rec)
    }
    fn swap_rec(&mut self, node: TrieNodeODRc<V>) -> Option<TrieNodeODRc<V>> {
        let mut old_child = UnsafeCell::new(Some(node));
        core::mem::swap(&mut old_child, &mut self.rec);
        old_child.into_inner()
    }
    fn val(&self) -> Option<&V> {
        unsafe{ &*self.value.get() }.as_ref()
    }
    fn has_val(&self) -> bool {
        unsafe{ &*self.value.get() }.is_some()
    }
    fn val_mut(&mut self) -> Option<&mut V> {
        unsafe{ &mut *self.value.get() }.as_mut()
    }
    fn take_val(&mut self) -> Option<V> {
        core::mem::take(&mut self.value).into_inner()
    }
    fn set_val(&mut self, val: V) {
        self.value = UnsafeCell::new(Some(val))
    }
    fn set_val_option(&mut self, val: Option<V>) {
        self.value = UnsafeCell::new(val)
    }
    fn swap_val(&mut self, val: V) -> Option<V> {
        let mut old_val = UnsafeCell::new(Some(val));
        core::mem::swap(&mut old_val, &mut self.value);
        old_val.into_inner()
    }
    fn both_mut(&mut self) -> (Option<&mut TrieNodeODRc<V>>, Option<&mut V>) {
        let rec = unsafe{ &mut *self.rec.get() }.as_mut();
        let val = unsafe{ &mut *self.value.get() }.as_mut();
        (rec, val)
    }
    fn into_both(self) -> (Option<TrieNodeODRc<V>>, Option<V>) {
        let rec = self.rec.into_inner();
        let val = self.value.into_inner();
        (rec, val)
    }
}

//GOAT this is unneeded
// impl<V: Clone + Send + Sync + Lattice> From<CellCoFree<V>> for OrdinaryCoFree<V> {
//     fn from(other: CellCoFree<V>) -> Self {
//         Self::from_cf(other)
//     }
// }

impl<V: Clone + Send + Sync + Lattice, Cf: CoFree<V=V>, OtherCf: CoFree<V=V>> HeteroLattice<OtherCf> for Cf {
    fn pjoin(&self, other: &OtherCf) -> AlgebraicResult<Self> {
        let rec_result = self.rec().pjoin(&other.rec()).flatten();
        let val_result = self.val().pjoin(&other.val()).flatten();
        rec_result.merge(val_result, |which_arg| {
            match which_arg {
                0 => self.rec().cloned(),
                1 => other.rec().cloned(),
                _ => unreachable!()
            }
        }, |which_arg| {
            match which_arg {
                0 => self.val().cloned(),
                1 => other.val().cloned(),
                _ => unreachable!()
            }
        }, |rec, val| AlgebraicResult::Element(Self::new(rec, val)))
    }
    fn join_into(&mut self, other: OtherCf) -> AlgebraicStatus {
        let (other_rec, other_val) = other.into_both();
        let rec_status = match self.rec_mut() {
            Some(self_rec) => match other_rec {
                Some(other_rec) => {
                    let (status, result) = self_rec.make_mut().join_into_dyn(other_rec);
                    match result {
                        Ok(()) => {},
                        Err(replacement_node) => {*self_rec = replacement_node},
                    }
                    status
                },
                None => AlgebraicStatus::Identity,
            },
            None => match other_rec {
                Some(_) => {
                    self.set_rec_option(other_rec);
                    AlgebraicStatus::Element
                },
                None => AlgebraicStatus::None
            }
        };
        let val_status = match self.val_mut() {
            Some(self_val) => match other_val {
                Some(other_val) => self_val.join_into(other_val),
                None => AlgebraicStatus::Identity,
            },
            None => match other_val {
                Some(_) => {
                    self.set_val_option(other_val);
                    AlgebraicStatus::Element
                },
                None => AlgebraicStatus::None
            }
        };
        //Note: (true, true) makes sense in the context of a join implementation because when the rec_status or
        // val_status is None, it can only have gotten that way because the respective CF field was already None.
        // This is because Join will never convert Some into None, but this logic isn't portable to other ops
        rec_status.merge(val_status, true, true)
    }
    fn pmeet(&self, other: &OtherCf) -> AlgebraicResult<Self> {
        let rec = self.rec().pmeet(&other.rec());
        let val = self.val().pmeet(&other.val());
        self.combine_algebraic_results(other, rec, val)
    }
    fn join_all(_xs: &[&Self]) -> Self where Self: Sized {
        unreachable!() //Currently not used
    }
    fn convert(other: OtherCf) -> Self {
        Self::from_cf(other)
    }
    fn bottom() -> Self {
        Self::default()
    }
}

impl<V: Clone + Sync + Send + DistributiveLattice, Cf: CoFree<V=V>, OtherCf: CoFree<V=V>> HeteroDistributiveLattice<OtherCf> for Cf {
    fn psubtract(&self, other: &OtherCf) -> AlgebraicResult<Self> where Self: Sized {
        let rec = self.rec().psubtract(&other.rec());
        let val = self.val().psubtract(&other.val());
        self.combine_algebraic_results(other, rec, val)
    }
}

impl<V: Clone + Sync + Send, Cf: CoFree<V=V>, OtherCf: CoFree<V=V>> HeteroQuantale<OtherCf> for Cf {
    fn prestrict(&self, other: &OtherCf) -> AlgebraicResult<Self> {
        debug_assert!(self.has_rec() || self.has_val());
        if other.has_val() { AlgebraicResult::Identity(SELF_IDENT) } // assumes self can never be CoFree{None, None}
        else {
            match (self.rec(), other.rec()) {
                (Some(l), Some(r)) => {
                    match l.prestrict(r) {
                        AlgebraicResult::Identity(mask) => {
                            debug_assert_eq!(mask, SELF_IDENT); //restrict is not commutative
                            if self.has_val() {
                                //We need to strip off the value of a recursive branch in the lmap,
                                // without a corresponding value in the rmap
                                AlgebraicResult::Element(CoFree::new(Some(l.clone()), None))
                            } else {
                                AlgebraicResult::Identity(SELF_IDENT)
                            }
                        },
                        AlgebraicResult::None => AlgebraicResult::None,
                        AlgebraicResult::Element(node) => AlgebraicResult::Element(CoFree::new(Some(node), None)),
                    }
                }
                _ => { AlgebraicResult::None }
            }
        }
    }
}

impl<V: Clone, Cf: CoFree<V=V>, OtherCf: CoFree<V=V>> CfShared<OtherCf> for Cf {
    #[inline]
    fn combine_algebraic_results(&self, other: &OtherCf, rec: AlgebraicResult<Option<TrieNodeODRc<Self::V>>>, val: AlgebraicResult<Option<Self::V>>) -> AlgebraicResult<Self> {
        match (rec, val) {
            (AlgebraicResult::None, AlgebraicResult::None) => AlgebraicResult::None,
            (AlgebraicResult::Identity(rec_mask), AlgebraicResult::Identity(val_mask)) => {
                debug_assert!(rec_mask & (SELF_IDENT | COUNTER_IDENT) > 0);
                debug_assert!(val_mask & (SELF_IDENT | COUNTER_IDENT) > 0);
                let new_mask = rec_mask & val_mask;
                if new_mask > 0 {
                    AlgebraicResult::Identity(new_mask)
                } else {
                    let rec = if rec_mask & SELF_IDENT > 0 {
                        self.rec().cloned()
                    } else {
                        other.rec().cloned()
                    };
                    let val = if val_mask & SELF_IDENT > 0 {
                        self.val().cloned()
                    } else {
                        other.val().cloned()
                    };
                    AlgebraicResult::Element(Self::new(rec, val))
                }
            },
            (AlgebraicResult::None, AlgebraicResult::Identity(val_mask)) => {
                let mut new_mask = val_mask;
                if !self.rec().is_none() {
                    new_mask &= !SELF_IDENT;
                }
                if !other.rec().is_none() {
                    new_mask &= !COUNTER_IDENT;
                }
                if new_mask > 0 {
                    AlgebraicResult::Identity(new_mask)
                } else {
                    AlgebraicResult::Element(Self::new(None, self.val().cloned()))
                }
            },
            (AlgebraicResult::Identity(rec_mask), AlgebraicResult::None) => {
                let mut new_mask = rec_mask;
                if !self.val().is_none() {
                    new_mask &= !SELF_IDENT;
                }
                if !other.val().is_none() {
                    new_mask &= !COUNTER_IDENT;
                }
                if new_mask > 0 {
                    AlgebraicResult::Identity(new_mask)
                } else {
                    AlgebraicResult::Element(Self::new(self.rec().cloned(), None))
                }
            },
            (rec_el, val_el) => {
                let rec = rec_el.flatten().map_into_option(|arg_idx| {
                    match arg_idx {
                        0 => self.rec().cloned(),
                        1 => other.rec().cloned(),
                        _ => unreachable!()
                    }
                });
                let val = val_el.flatten().map_into_option(|arg_idx| {
                    match arg_idx {
                        0 => self.val().cloned(),
                        1 => other.val().cloned(),
                        _ => unreachable!()
                    }
                });
                debug_assert!(rec.is_some() || val.is_some());
                AlgebraicResult::Element(Self::new(rec, val))
            }
        }
    }
}

impl<V: Clone + Send + Sync + Lattice, Cf: CoFree<V=V>, OtherCf: CoFree<V=V>> HeteroLattice<ByteNode<OtherCf>> for ByteNode<Cf> {
    fn pjoin(&self, other: &ByteNode<OtherCf>) -> AlgebraicResult<Self> {
        let jm: ByteMask = self.mask | other.mask; //joined mask
        let mm: ByteMask = self.mask & other.mask; //meet mask

        let mut is_identity = self.mask == jm;
        let mut is_counter_identity = other.mask == jm;

        let jmc = [jm.0[0].count_ones(), jm.0[1].count_ones(), jm.0[2].count_ones(), jm.0[3].count_ones()];

        let len = (jmc[0] + jmc[1] + jmc[2] + jmc[3]) as usize;
        let mut v: Vec<Cf, crate::utils::Leak> = Vec::with_capacity_in(len, crate::utils::Leak);
        // let mut v: Vec<Cf, crate::utils::Leak> = Vec::with_capacity_in(256, crate::utils::Leak);
        let new_v = v.spare_capacity_mut();

        let mut l = 0;
        let mut r = 0;
        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm.0[i];
            while lm != 0 {
                // this body runs at most 256 times, in the case there is 100% overlap between full nodes
                let index = lm.trailing_zeros();
                // println!("{}", index);
                if ((1u64 << index) & mm.0[i]) != 0 {
                    //This runs for cofrees that exist in both nodes
                    let lv = unsafe { self.values.get_unchecked(l) };
                    let rv = unsafe { other.values.get_unchecked(r) };
                    match lv.pjoin(rv) {
                        AlgebraicResult::None => unreachable!(), //Some joined to Some should never be None
                        AlgebraicResult::Identity(mask) => {
                            debug_assert!((mask & SELF_IDENT > 0) || (mask & COUNTER_IDENT > 0));
                            if mask & SELF_IDENT == 0 {
                                is_identity = false;
                            }
                            if mask & COUNTER_IDENT == 0 {
                                is_counter_identity = false;
                            }
                            if mask & SELF_IDENT > 0 {
                                unsafe { new_v.get_unchecked_mut(c).write(lv.clone()) };
                            } else {
                                let new_cf = Cf::from_cf(rv.clone());
                                unsafe { new_v.get_unchecked_mut(c).write(new_cf) };
                            }
                        },
                        AlgebraicResult::Element(jv) => {
                            is_identity = false;
                            is_counter_identity = false;
                            debug_assert!(jv.has_rec() || jv.has_val());
                            unsafe { new_v.get_unchecked_mut(c).write(jv) };
                        }
                    }
                    // println!("pushing lv rv j {:?} {:?} {:?}", lv, rv, jv);
                    l += 1;
                    r += 1;
                } else if ((1u64 << index) & self.mask.0[i]) != 0 {
                    // This runs for CoFrees that exist in only the left node
                    is_counter_identity = false;
                    let lv = unsafe { self.values.get_unchecked(l) };
                    // println!("pushing lv {:?}", lv);
                    unsafe { new_v.get_unchecked_mut(c).write(lv.clone()) };
                    l += 1;
                } else {
                    // This runs for CoFrees that exist in only the right node
                    is_identity = false;
                    let rv = unsafe { other.values.get_unchecked(r) };
                    // println!("pushing rv {:?}", rv);
                    unsafe { new_v.get_unchecked_mut(c).write(<_>::from_cf(rv.clone())) };
                    r += 1;
                }
                lm ^= 1u64 << index;
                c += 1;
            }
        }

        unsafe{ v.set_len(c); }
        if c == 0 {
            AlgebraicResult::None
        } else {
            if is_identity || is_counter_identity {
                let mut mask = 0;
                if is_identity { mask |= SELF_IDENT; }
                if is_counter_identity { mask |= COUNTER_IDENT; }
                AlgebraicResult::Identity(mask)
            } else {
                AlgebraicResult::Element(Self{ mask: jm, values: v })
            }
        }
    }

    fn join_into(&mut self, mut other: ByteNode<OtherCf>) -> AlgebraicStatus {
        let jm: ByteMask = self.mask | other.mask;
        let mm: ByteMask = self.mask & other.mask;

        let mut is_identity = self.mask == jm;

        let jmc = [jm.0[0].count_ones(), jm.0[1].count_ones(), jm.0[2].count_ones(), jm.0[3].count_ones()];

        let l = (jmc[0] + jmc[1] + jmc[2] + jmc[3]) as usize;
        let mut v = Vec::with_capacity_in(l, crate::utils::Leak);
        // let mut v = Vec::with_capacity_in(256, crate::utils::Leak);
        let new_v = v.spare_capacity_mut();

        let mut l = 0;
        let mut r = 0;
        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm.0[i];
            while lm != 0 {
                // this body runs at most 256 times, in the case there is 100% overlap between full nodes
                let index = lm.trailing_zeros();
                // println!("{}", index);
                if ((1u64 << index) & mm.0[i]) != 0 {
                    let mut lv = unsafe { std::ptr::read(self.values.get_unchecked_mut(l)) };
                    let rv = unsafe { std::ptr::read(other.values.get_unchecked_mut(r)) };
                    match lv.join_into(rv) {
                        AlgebraicStatus::Identity => { },
                        AlgebraicStatus::Element => { is_identity = false; },
                        AlgebraicStatus::None => unreachable!(), //Some.join(Some) shouldn't create None
                    }
                    unsafe { new_v.get_unchecked_mut(c).write(lv) };
                    l += 1;
                    r += 1;
                } else if ((1u64 << index) & self.mask.0[i]) != 0 {
                    let lv = unsafe { std::ptr::read(self.values.get_unchecked_mut(l)) };
                    unsafe { new_v.get_unchecked_mut(c).write(lv) };
                    l += 1;
                } else {
                    is_identity = false;
                    let rv = unsafe { std::ptr::read(other.values.get_unchecked_mut(r)) };
                    unsafe { new_v.get_unchecked_mut(c).write(<_>::from_cf(rv)) };
                    r += 1;
                }
                lm ^= 1u64 << index;
                c += 1;
            }
        }

        unsafe { self.values.set_len(0) }
        unsafe { other.values.set_len(0) }
        unsafe { v.set_len(c) }
        self.mask = jm;
        self.values = v;

        if c == 0 {
            AlgebraicStatus::None
        } else if is_identity {
            AlgebraicStatus::Identity
        } else {
            AlgebraicStatus::Element
        }
    }

    fn pmeet(&self, other: &ByteNode<OtherCf>) -> AlgebraicResult<Self> {
        // TODO this technically doesn't need to calculate and iterate over jm
        // iterating over mm and calculating m such that the following suffices
        // c_{self,other} += popcnt(m & {self,other})
        let jm: ByteMask = self.mask | other.mask;
        let mut mm: ByteMask = self.mask & other.mask;

        let mut is_identity = self.mask == mm;
        let mut is_counter_identity = other.mask == mm;

        let mmc = [mm.0[0].count_ones(), mm.0[1].count_ones(), mm.0[2].count_ones(), mm.0[3].count_ones()];

        let len = (mmc[0] + mmc[1] + mmc[2] + mmc[3]) as usize;
        let mut v = Vec::with_capacity_in(len, crate::utils::Leak);
        // let mut v = Vec::with_capacity_in(256, crate::utils::Leak);
        let new_v = v.spare_capacity_mut();

        let mut l = 0;
        let mut r = 0;
        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm.0[i];
            while lm != 0 {
                let index = lm.trailing_zeros();

                if ((1u64 << index) & mm.0[i]) != 0 {
                    //This runs for cofrees that exist in both nodes

                    let lv = unsafe { self.values.get_unchecked(l) };
                    let rv = unsafe { other.values.get_unchecked(r) };
                    match lv.pmeet(rv) {
                        AlgebraicResult::None => {
                            is_counter_identity = false;
                            is_identity = false;
                            mm.0[i] ^= 1u64 << index;
                        },
                        AlgebraicResult::Identity(mask) => {
                            debug_assert!((mask & SELF_IDENT > 0) || (mask & COUNTER_IDENT > 0));
                            if mask & SELF_IDENT == 0 {
                                is_identity = false;
                            }
                            if mask & COUNTER_IDENT == 0 {
                                is_counter_identity = false;
                            }
                            if mask & SELF_IDENT > 0 {
                                unsafe { new_v.get_unchecked_mut(c).write(lv.clone()) };
                            } else {
                                let new_cf = Cf::from_cf(rv.clone());
                                unsafe { new_v.get_unchecked_mut(c).write(new_cf) };
                            }
                            c += 1;
                        },
                        AlgebraicResult::Element(jv) => {
                            is_identity = false;
                            is_counter_identity = false;
                            unsafe { new_v.get_unchecked_mut(c).write(jv) };
                            c += 1;
                        },
                    }
                    l += 1;
                    r += 1;
                } else if ((1u64 << index) & self.mask.0[i]) != 0 {
                    l += 1;
                } else {
                    r += 1;
                }
                lm ^= 1u64 << index;
            }
        }

        unsafe{ v.set_len(c); }
        if c == 0 {
            AlgebraicResult::None
        } else {
            if is_identity || is_counter_identity {
                let mut mask = 0;
                if is_identity { mask |= SELF_IDENT; }
                if is_counter_identity { mask |= COUNTER_IDENT; }
                AlgebraicResult::Identity(mask)
            } else {
                AlgebraicResult::Element(Self{ mask: mm, values: v })
            }
        }
    }

    fn join_all(xs: &[&Self]) -> Self {
        let mut jm: ByteMask = ByteMask::EMPTY;
        for x in xs.iter() {
            jm |= x.mask;
        }

        let jmc = [jm.0[0].count_ones(), jm.0[1].count_ones(), jm.0[2].count_ones(), jm.0[3].count_ones()];

        let len = (jmc[0] + jmc[1] + jmc[2] + jmc[3]) as usize;
        let mut v = Vec::with_capacity_in(len, crate::utils::Leak);
        // let mut v = Vec::with_capacity_in(256, crate::utils::Leak);
        let new_v = v.spare_capacity_mut();

        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm.0[i];
            while lm != 0 {
                // this body runs at most 256 times, in the case there is 100% overlap between full nodes
                let index = lm.trailing_zeros();

                //GOAT, allocating a temp buffer likely undoes the gains from join_all
                let to_join: Vec<&Cf> = xs.iter().enumerate().filter_map(|(i, x)| x.get(i as u8)).collect();
                let joined = HeteroLattice::<Cf>::join_all(&to_join[..]);
                unsafe { new_v.get_unchecked_mut(c).write(joined) };

                lm ^= 1u64 << index;
                c += 1;
            }
        }

        unsafe{ v.set_len(c); }
        return Self{ mask: jm, values: v };
    }
    fn convert(other: ByteNode<OtherCf>) -> Self {
        let mut values = Vec::with_capacity_in(other.values.len(), crate::utils::Leak);
        // let mut values = Vec::with_capacity_in(256, crate::utils::Leak);
        other.values.into_iter().map(|other_cf| Cf::convert(other_cf)).collect_into(&mut values);
        Self {
            mask: other.mask,
            values
        }
    }
    fn bottom() -> Self {
        Self::default()
    }
}

//NOTE: This *looks* like an impl of DistributiveLattice, but it isn't, so we can have `self` and
// `other` be differently parameterized types
impl<V: DistributiveLattice + Clone + Send + Sync, Cf: CoFree<V=V>> ByteNode<Cf> {
    fn psubtract<OtherCf: CoFree<V=V>>(&self, other: &ByteNode<OtherCf>) -> AlgebraicResult<Self> where Self: Sized {
        let mut is_identity = true;
        let mut btn = self.clone();

        for i in 0..4 {
            let mut lm = self.mask.0[i];
            while lm != 0 {
                let index = lm.trailing_zeros();

                if ((1u64 << index) & other.mask.0[i]) != 0 {
                    let lv = unsafe { self.get_unchecked(64*(i as u8) + (index as u8)) };
                    let rv = unsafe { other.get_unchecked(64*(i as u8) + (index as u8)) };
                    match HeteroDistributiveLattice::psubtract(lv, rv) {
                        AlgebraicResult::None => {
                            is_identity = false;
                            btn.remove(64*(i as u8) + (index as u8));
                        },
                        AlgebraicResult::Identity(mask) => {
                            debug_assert_eq!(mask, SELF_IDENT); //subtract is non-commutative
                        },
                        AlgebraicResult::Element(jv) => {
                            is_identity = false;
                            let dst = unsafe { btn.get_unchecked_mut(64*(i as u8) + (index as u8)) };
                            *dst = jv;
                        },
                    }
                }

                lm ^= 1u64 << index;
            }
        }

        if btn.is_empty() {
            AlgebraicResult::None
        } else {
            if is_identity {
                AlgebraicResult::Identity(SELF_IDENT)
            } else {
                AlgebraicResult::Element(btn)
            }
        }
    }
}

//NOTE: This *looks* like an impl of Quantale, but it isn't, so we can have `self` and
// `other` be differently parameterized types
impl<V:Clone + Sync + Send, Cf: CoFree<V=V>> ByteNode<Cf> {
    fn prestrict<OtherCf: CoFree<V=V>>(&self, other: &ByteNode<OtherCf>) -> AlgebraicResult<Self> where Self: Sized {
        let mut is_identity = true;

        // TODO this technically doesn't need to calculate and iterate over jm
        // iterating over mm and calculating m such that the following suffices
        // c_{self,other} += popcnt(m & {self,other})
        let jm: ByteMask = self.mask | other.mask;
        let mut mm: ByteMask = self.mask & other.mask;

        let mmc = [mm.0[0].count_ones(), mm.0[1].count_ones(), mm.0[2].count_ones(), mm.0[3].count_ones()];

        let len = (mmc[0] + mmc[1] + mmc[2] + mmc[3]) as usize;
        let mut v = Vec::with_capacity_in(len, crate::utils::Leak);
        // let mut v = Vec::with_capacity_in(256, crate::utils::Leak);
        let new_v = v.spare_capacity_mut();

        let mut l = 0;
        let mut r = 0;
        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm.0[i];
            while lm != 0 {
                let index = lm.trailing_zeros();

                if ((1u64 << index) & mm.0[i]) != 0 {
                    let lv = unsafe { self.values.get_unchecked(l) };
                    let rv = unsafe { other.values.get_unchecked(r) };
                    // println!("dense prestrict {}", index as usize + i*64);

                    match lv.prestrict(rv) {
                        AlgebraicResult::None => {
                            is_identity = false;
                            mm.0[i] ^= 1u64 << index;
                        }
                        AlgebraicResult::Identity(mask) => {
                            debug_assert_eq!(mask, SELF_IDENT); //restrict is non-commutative
                            unsafe { new_v.get_unchecked_mut(c).write(lv.clone()) };
                            c += 1;
                        },
                        AlgebraicResult::Element(jv) => {
                            is_identity = false;
                            unsafe { new_v.get_unchecked_mut(c).write(jv) };
                            c += 1;
                        },
                    }
                    l += 1;
                    r += 1;
                } else {
                    is_identity = false;
                    if ((1u64 << index) & self.mask.0[i]) != 0 {
                        l += 1;
                    } else {
                        r += 1;
                    }
                }
                lm ^= 1u64 << index;
            }
        }

        unsafe{ v.set_len(c); }
        if c == 0 {
            AlgebraicResult::None
        } else {
            if is_identity {
                AlgebraicResult::Identity(SELF_IDENT)
            } else {
                AlgebraicResult::Element(Self{ mask: mm, values: v })
            }
        }
    }
}

#[test]
fn bit_siblings() {
    let x = 0b0000000000000000000000000000000000000100001001100000000000000010u64;
    let i = 0b0000000000000000000000000000000000000000000001000000000000000000u64;
    let p = 0b0000000000000000000000000000000000000000001000000000000000000000u64;
    let n = 0b0000000000000000000000000000000000000000000000100000000000000000u64;
    let f = 0b0000000000000000000000000000000000000100000000000000000000000000u64;
    let l = 0b0000000000000000000000000000000000000000000000000000000000000010u64;
    let bit_i = 18;
    let bit_i_onehot = 1u64 << bit_i;
    assert_eq!(i, bit_i_onehot);
    assert_ne!(bit_i_onehot & x, 0);
    assert_eq!(p, 1u64 << bit_sibling(bit_i, x, false));
    assert_eq!(n, 1u64 << bit_sibling(bit_i, x, true));
    assert_eq!(f, 1u64 << bit_sibling(f.trailing_zeros() as u8, x, false));
    assert_eq!(l, 1u64 << bit_sibling(l.trailing_zeros() as u8, x, true));
    assert_eq!(0, bit_sibling(0, 1, false));
    assert_eq!(0, bit_sibling(0, 1, true));
    assert_eq!(63, bit_sibling(63, 1u64 << 63, false));
    assert_eq!(63, bit_sibling(63, 1u64 << 63, true));
}
