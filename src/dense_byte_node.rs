
use core::fmt::{Debug, Formatter};
use core::ptr;
use std::collections::HashMap;

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

#[cfg(feature = "smallvec")]
use smallvec::SmallVec;

use crate::ring::*;
use crate::trie_node::*;
use crate::line_list_node::{LineListNode, merge_into_dense_node};

//NOTE: This: `core::array::from_fn(|i| i as u8);` ought to work, but https://github.com/rust-lang/rust/issues/109341
const ALL_BYTES: [u8; 256] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239, 240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 254, 255];

#[derive(Clone)]
pub struct DenseByteNode<V> {
    pub mask: [u64; 4],
    #[cfg(all(feature = "all_dense_nodes", feature = "smallvec"))]
    values: SmallVec<[CoFree<V>; 2]>,
    #[cfg(all(not(feature = "all_dense_nodes"), feature = "smallvec"))]
    values: SmallVec<[CoFree<V>; 4]>,
    #[cfg(not(feature = "smallvec"))]
    values: Vec<CoFree<V>>,
}

impl<V> Default for DenseByteNode<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl <V> Debug for DenseByteNode<V> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        //Recursively printing a whole tree will get pretty unwieldy.  Should do something
        // like serialization for inspection using standard tools.
        write!(f, "DenseByteNode {{count={}", self.values.len())?;
        self.for_each_item(|node, c, i| {
            let cf = node.values.get(i).unwrap();
            let _ = write!(f, ", {c}:(val={} child={})", cf.value.is_some(), cf.rec.is_some());
        });
        write!(f, "}}")
    }
}

impl<V> DenseByteNode<V> {
    #[inline]
    pub fn new() -> Self {
        Self {
            mask: [0u64; 4],
            values: <_>::default()
        }
    }
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            mask: [0u64; 4],
            #[cfg(feature = "smallvec")]
            values: SmallVec::with_capacity(capacity),
            #[cfg(not(feature = "smallvec"))]
            values: Vec::with_capacity(capacity),
        }
    }
    #[inline]
    pub fn reserve_capacity(&mut self, additional: usize) {
        self.values.reserve(additional)
    }
    #[inline]
    fn left(&self, pos: u8) -> u8 {
        if pos == 0 { return 0 }
        let mut c = 0u8;
        let m = !0u64 >> (63 - ((pos - 1) & 0b00111111));
        if pos > 0b01000000 { c += self.mask[0].count_ones() as u8; }
        else { return c + (self.mask[0] & m).count_ones() as u8 }
        if pos > 0b10000000 { c += self.mask[1].count_ones() as u8; }
        else { return c + (self.mask[1] & m).count_ones() as u8 }
        if pos > 0b11000000 { c += self.mask[2].count_ones() as u8; }
        else { return c + (self.mask[2] & m).count_ones() as u8 }
        // println!("{} {:b} {} {}", pos, self.mask[3], m.count_ones(), c);
        return c + (self.mask[3] & m).count_ones() as u8;
    }

    #[inline]
    fn contains(&self, k: u8) -> bool {
        0 != (self.mask[((k & 0b11000000) >> 6) as usize] & (1u64 << (k & 0b00111111)))
    }

    #[inline]
    fn set(&mut self, k: u8) -> () {
        // println!("setting k {} : {} {:b}", k, ((k & 0b11000000) >> 6) as usize, 1u64 << (k & 0b00111111));
        self.mask[((k & 0b11000000) >> 6) as usize] |= 1u64 << (k & 0b00111111);
    }

    /// Internal method to clear the bit associated with a given key.  This should be accompanied by removing the
    /// cofree entry from the values Vec
    #[inline]
    fn clear(&mut self, k: u8) -> () {
        // println!("setting k {} : {} {:b}", k, ((k & 0b11000000) >> 6) as usize, 1u64 << (k & 0b00111111));
        self.mask[((k & 0b11000000) >> 6) as usize] &= !(1u64 << (k & 0b00111111));
    }

    /// Ensures that a CoFree exists for the specified key
    ///
    /// Returns `true` if a new CoFree was created, and `false` if one already existed
    ///
    /// This enables a WriteZipper to modify a specific CoFree without touching the DenseByteNode
    /// that contains it, and therefore multiple WriteZippers can be rooted at the same parent, so
    /// long as the first byte of each path is unique
    #[inline]
    pub(crate) fn prepare_cf(&mut self, k: u8) -> bool {
        if self.get(k).is_some() {
            false
        } else {
            let ix = self.left(k) as usize;
            self.set(k);
            let new_cf = CoFree {rec: None, value: None };
            self.values.insert(ix, new_cf);
            true
        }
    }

    /// Adds a new child at the specified key byte.  Replaces and returns an existing branch.
    /// Use [join_child_into] to join with the existing branch
    #[inline]
    pub fn set_child(&mut self, k: u8, node: TrieNodeODRc<V>) -> Option<TrieNodeODRc<V>> {
        let ix = self.left(k) as usize;
        if self.contains(k) {
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            let mut old_child = Some(node);
            core::mem::swap(&mut old_child, &mut cf.rec);
            old_child
        } else {
            self.set(k);
            let new_cf = CoFree {rec: Some(node), value: None };
            self.values.insert(ix, new_cf);
            None
        }
    }

    /// The same as [set_child] if no child exists in the node at the key.  Otherwise joins the two nodes
    /// together
    #[inline]
    pub fn join_child_into(&mut self, k: u8, node: TrieNodeODRc<V>) where V: Clone + Lattice {
        let ix = self.left(k) as usize;
        if self.contains(k) {
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            match &mut cf.rec {
                Some(existing_node) => {
                    let joined = existing_node.join(&node);
                    *existing_node = joined;
                },
                None => { cf.rec = Some(node); }
            }
        } else {
            self.set(k);
            let new_cf = CoFree {rec: Some(node), value: None };
            self.values.insert(ix, new_cf);
        }
    }

    #[inline]
    pub fn set_val(&mut self, k: u8, val: V) -> Option<V> {
        let ix = self.left(k) as usize;
        if self.contains(k) {
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            let result = core::mem::take(&mut cf.value);
            cf.value = Some(val);
            result
        } else {
            self.set(k);
            let new_cf = CoFree {rec: None, value: Some(val) };
            self.values.insert(ix, new_cf);
            None
        }
    }

    #[inline]
    pub fn remove_val(&mut self, k: u8) -> Option<V> {
        let ix = self.left(k) as usize;
        debug_assert!(self.contains(k));

        let cf = unsafe { self.values.get_unchecked_mut(ix) };
        let result = core::mem::take(&mut cf.value);

        if cf.rec.is_none() {
            self.clear(k);
            self.values.remove(ix);
        }
        result
    }

    /// Similar in behavior to [set_val], but will join v with the existing value instead of replacing it
    #[inline]
    pub fn join_val_into(&mut self, k: u8, val: V) where V: Lattice {
        let ix = self.left(k) as usize;
        if self.contains(k) {
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            match &mut cf.value {
                Some(existing_val) => {
                    let joined = existing_val.join(&val);
                    *existing_val = joined;
                }
                None => { cf.value = Some(val); }
            }
        } else {
            self.set(k);
            let new_cf = CoFree {rec: None, value: Some(val) };
            self.values.insert(ix, new_cf);
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
    pub(crate) fn join_payload_into(&mut self, k: u8, payload: ValOrChild<V>) where V: Clone + Lattice {
        match payload {
            ValOrChild::Child(child) => {
                self.join_child_into(k, child);
            },
            ValOrChild::Val(val) => {
                self.join_val_into(k, val);
            }
        }
    }

    // //GOAT-Deprecated-Update
    // #[inline]
    // fn update_val(&mut self, k: u8, default_f: Box<dyn FnOnce()->V + '_>) -> &mut V {
    //     let ix = self.left(k) as usize;
    //     if self.contains(k) {
    //         let cf = unsafe { self.values.get_unchecked_mut(ix) };
    //         if cf.value.is_none() {
    //             cf.value = Some(default_f());
    //         }
    //         cf.value.as_mut().unwrap()
    //     } else {
    //         self.set(k);
    //         let new_cf = CoFree {rec: None, value: Some(default_f()) };
    //         self.values.insert(ix, new_cf);
    //         let cf = unsafe { self.values.get_unchecked_mut(ix) };
    //         cf.value.as_mut().unwrap()
    //     }
    // }

    //GOAT-Deprecated-Update
    // #[inline]
    // fn update<F : FnOnce() -> CoFree<V>>(&mut self, k: u8, default: F) -> &mut CoFree<V> {
    //     let ix = self.left(k) as usize;
    //     if !self.contains(k) {
    //         self.set(k);
    //         self.values.insert(ix, default());
    //     }
    //     unsafe { self.values.get_unchecked_mut(ix) }
    // }

    /// Internal method to remove a CoFree from the node
    #[inline]
    fn remove(&mut self, k: u8) -> Option<CoFree<V>> {
        if self.contains(k) {
            let ix = self.left(k) as usize;
            let v = self.values.remove(ix);
            self.clear(k);
            return Some(v);
        }
        None
    }

    #[inline]
    pub(crate) fn get(&self, k: u8) -> Option<&CoFree<V>> { //GOAT, we don't want this to be public for real!!
        if self.contains(k) {
            let ix = self.left(k) as usize;
            // println!("pos ix {} {} {:b}", pos, ix, self.mask);
            unsafe { Some(self.values.get_unchecked(ix)) }
        } else {
            None
        }
    }

    #[inline]
    fn get_mut(&mut self, k: u8) -> Option<&mut CoFree<V>> {
        if self.contains(k) {
            let ix = self.left(k) as usize;
            unsafe { Some(self.values.get_unchecked_mut(ix)) }
        } else {
            None
        }
    }

    #[inline]
    fn get_child_mut(&mut self, k: u8) -> Option<&mut TrieNodeODRc<V>> {
        self.get_mut(k).and_then(|cf| cf.rec.as_mut())
    }

    #[inline]
    pub unsafe fn get_unchecked(&self, k: u8) -> &CoFree<V> {
        let ix = self.left(k) as usize;
        // println!("pos ix {} {} {:b}", pos, ix, self.mask);
        self.values.get_unchecked(ix)
    }

    #[inline]
    unsafe fn get_unchecked_mut(&mut self, k: u8) -> &mut CoFree<V> {
        let ix = self.left(k) as usize;
        // println!("pos ix {} {} {:b}", pos, ix, self.mask);
        self.values.get_unchecked_mut(ix)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.mask[0] == 0 && self.mask[1] == 0 && self.mask[2] == 0 && self.mask[3] == 0
    }

    #[inline]
    fn len(&self) -> usize {
        return (self.mask[0].count_ones() + self.mask[1].count_ones() + self.mask[2].count_ones() + self.mask[3].count_ones()) as usize;
    }

    /// Determines the nth prefix in the node, counting forwards or backwards
    #[inline]
    pub (crate) fn item_idx_to_prefix<const FORWARD: bool>(&self, idx: usize) -> Option<u8> {
        let mut i = if FORWARD { 0 } else { 3 };
        let mut m = self.mask[i];
        let mut c = 0;
        let mut c_ahead = m.count_ones() as usize;
        loop {
            if idx < c_ahead { break; }
            if FORWARD { i += 1} else { i -= 1 };
            if i > 3 { return None }
            m = self.mask[i];
            c = c_ahead;
            c_ahead += m.count_ones() as usize;
        }

        let mut loc;
        if !FORWARD {
            loc = 63 - m.leading_zeros();
            while c < idx {
                m ^= 1u64 << loc;
                loc = 63 - m.leading_zeros();
                c += 1;
            }
        } else {
            loc = m.trailing_zeros();
            while c < idx {
                m ^= 1u64 << loc;
                loc = m.trailing_zeros();
                c += 1;
            }
        }

        let prefix = i << 6 | (loc as usize);
        // println!("{:#066b}", self.focus.mask[i]);
        // println!("{i} {loc} {prefix}");
        debug_assert!(self.contains(prefix as u8));

        Some(prefix as u8)
    }

    /// Iterates the entries in `self`, calling `func` for each entry
    /// The arguments to `func` are: `func(self, key_byte, n)`, where `n` is the number of times
    /// `func` has been called prior.  This corresponds to index of the `CoFree` in the `values` vec
    #[inline]
    fn for_each_item<F: FnMut(&Self, u8, usize)>(&self, mut func: F) {
        let mut n = 0;
        for i in 0..4 {
            let mut lm = self.mask[i];
            while lm != 0 {
                let index = lm.trailing_zeros();

                let key_byte = 64*(i as u8) + (index as u8);
                func(self, key_byte, n);
                n += 1;

                lm ^= 1u64 << index;
            }
        }
    }

    /// Internal method to subtract nodes of an abstract type from the node
    fn psubtract_abstract(&self, other: &dyn TrieNode<V>) -> (bool, Option<TrieNodeODRc<V>>) where V: Clone + PartialDistributiveLattice {
        let mut new_node = Self::new();

        //Go over each populated entry in the node
        self.for_each_item(|self_node, key_byte, cf_idx| {
            if other.node_contains_partial_key(&[key_byte]) {
                let cf = unsafe{ self_node.values.get_unchecked(cf_idx) };
                let mut new_cf = CoFree::new();

                //If there is a value at this key_byte, and the other node contains a value, subtract them
                if let Some(self_val) = &cf.value {
                    if let Some(other_val) = other.node_get_val(&[key_byte]) {
                        new_cf.value = self_val.psubtract(other_val);
                    }
                }

                //If there is an onward link, see if there is a matching link in other, and subtract them
                if let Some(self_child) = &cf.rec {
                    let other_child = other.get_node_at_key(&[key_byte]);
                    match other_child.try_borrow() {
                        Some(other_child) => {
                            let difference = self_child.borrow().psubtract_dyn(other_child);
                            if difference.0 {
                                new_cf.rec = Some(self_child.clone());
                            } else {
                                new_cf.rec = difference.1;
                            }
                        },
                        None => {
                            new_cf.rec = Some(self_child.clone());
                        }
                    }
                }

                //If we ended up with a value or a link in the CF, insert it into a new node
                if new_cf.rec.is_some() || new_cf.value.is_some() {
                    new_node.set(key_byte);
                    new_node.values.push(new_cf);
                }
            } else {
                new_node.set(key_byte);
                let cf = unsafe{ self_node.values.get_unchecked(cf_idx) };
                new_node.values.push(cf.clone());
            }
        });
        if new_node.is_empty() {
            (false, None)
        } else {
            //GOAT, OPTIMIZATION OPPORTUNITY. track whether any unique new nodes were created, or
            // whether we can just past back the "unmodified" flag for self
            (false, Some(TrieNodeODRc::new(new_node)))
        }
    }

    /// Internal method to restrict using nodes of an abstract type
    fn prestrict_abstract(&self, other: &dyn TrieNode<V>) -> (bool, Option<TrieNodeODRc<V>>) where V: Clone {
        let mut new_node = Self::new();

        //Go over each populated entry in the node
        self.for_each_item(|self_node, key_byte, cf_idx| {
            if other.node_contains_partial_key(&[key_byte]) {
                let cf = unsafe{ self_node.values.get_unchecked(cf_idx) };

                //If there is a comparable value in other, keep the whole cf
                if let Some(_) = other.node_get_val(&[key_byte]) {
                    new_node.set(key_byte);
                    new_node.values.push(cf.clone());
                } else {

                    //If there is an onward link in the CF and other node, continue the restriction recursively
                    if let Some(self_child) = &cf.rec {
                        let other_child = other.get_node_at_key(&[key_byte]);
                        match other_child.try_borrow() {
                            Some(other_child) => {
                                let restricted = self_child.borrow().prestrict_dyn(other_child);
                                let mut new_cf = CoFree::new();
                                // if restricted.0 {
                                //     new_cf.rec = Some(self_child.clone());
                                // } else {
                                //     new_cf.rec = restricted.1;
                                // }
                                new_cf.rec = restricted; //GOAT, optimization opportunity to track when we can reuse a whole node unmodified. See commented out code above.
                                if new_cf.rec.is_some() {
                                    new_node.set(key_byte);
                                    new_node.values.push(new_cf);
                                }
                            },
                            None => { }
                        }
                    }
                }
            }
        });
        if new_node.is_empty() {
            (false, None)
        } else {
            //GOAT, OPTIMIZATION OPPORTUNITY. track whether any unique new nodes were created, or
            // whether we can just past back the "unmodified" flag for self
            (false, Some(TrieNodeODRc::new(new_node)))
        }
    }
}

impl <V: Clone> DenseByteNode<V> {
    /// Internal method to recursively create all parents to support a value or branch at a given path
    #[cfg(feature = "all_dense_nodes")]
    #[inline]
    pub(crate) fn create_parent_path(&mut self, path: &[u8]) -> &mut Self {
        let mut cur = self;
        for i in 0..path.len() - 1 {
            let new_node = Self::new();
            cur.set_child(path[i], TrieNodeODRc::new(new_node));
            cur = cur.get_child_mut(path[i]).unwrap().make_mut().as_dense_mut().unwrap();
        }
        cur
    }
}

impl<V: Clone> TrieNode<V> for DenseByteNode<V> {
    fn node_contains_partial_key(&self, key: &[u8]) -> bool {
        debug_assert!(key.len() > 0);
        if key.len() == 1 {
            self.contains(key[0])
        } else {
            false
        }
    }
    #[inline(always)]
    fn node_get_child(&self, key: &[u8]) -> Option<(usize, &dyn TrieNode<V>)> {
        self.get(key[0]).and_then(|cf|
            cf.rec.as_ref().map(|child_node| {
                (1, &*child_node.borrow())
            })
        )
    }
    fn node_get_child_and_val_mut(&mut self, key: &[u8]) -> Option<(usize, Option<&mut V>, Option<&mut TrieNodeODRc<V>>)> {
        self.get_mut(key[0]).and_then(|cf|
            if cf.rec.is_some() || cf.value.is_some() {
                Some((1, cf.value.as_mut(), cf.rec.as_mut()))
            } else {
                None
            }
        )
    }
    fn node_get_child_mut(&mut self, key: &[u8]) -> Option<(usize, &mut TrieNodeODRc<V>)> {
        debug_assert!(key.len() > 0);
        self.get_child_mut(key[0]).map(|child_node_ptr| {
            (1, child_node_ptr)
        })
    }
    fn node_replace_child(&mut self, key: &[u8], new_node: TrieNodeODRc<V>) -> &mut dyn TrieNode<V> {
        debug_assert!(key.len() == 1);
        let cf = self.get_mut(key[0]).unwrap();
        *cf.rec.as_mut().unwrap() = new_node;
        cf.rec.as_mut().unwrap().make_mut()
    }
    fn node_contains_val(&self, key: &[u8]) -> bool {
        if key.len() == 1 {
            match self.get(key[0]) {
                Some(cf) => cf.value.is_some(),
                None => false
            }
        } else {
            false
        }
    }
    fn node_get_val(&self, key: &[u8]) -> Option<&V> {
        if key.len() == 1 {
            self.get(key[0]).and_then(|cf| cf.value.as_ref() )
        } else {
            None
        }
    }
    fn node_get_val_mut(&mut self, key: &[u8]) -> Option<&mut V> {
        if key.len() == 1 {
            self.get_mut(key[0]).and_then(|cf| cf.value.as_mut() )
        } else {
            None
        }
    }
    fn node_set_val(&mut self, key: &[u8], val: V) -> Result<(Option<V>, bool), TrieNodeODRc<V>> {
        #[cfg(not(feature = "all_dense_nodes"))]
        {
            //Split a ListNode to hold everything after the first byte of the key
            if key.len() > 1 {
                let mut child = LineListNode::new();
                child.node_set_val(&key[1..], val).unwrap_or_else(|_| panic!());
                self.set_child(key[0], TrieNodeODRc::new(child));
                Ok((None, true))
            } else {
                Ok((self.set_val(key[0], val), false))
            }
        }

        #[cfg(feature = "all_dense_nodes")]
        {
            let (last_node, sub_branch_added) = if key.len() > 1 {
                (self.create_parent_path(key), true)
            } else {
                (self, false)
            };
            Ok((last_node.set_val(key[key.len()-1], val), sub_branch_added))
        }
    }
    fn node_remove_val(&mut self, key: &[u8]) -> Option<V> {
        if key.len() == 1 {
            self.remove_val(key[0])
        } else {
            None
        }
    }
    //GOAT-Deprecated-Update, delete this once we have the WriteZipper doing everything `Update` did
    // fn node_update_val<'v>(&mut self, key: &[u8], default_f: Box<dyn FnOnce()->V + 'v>) -> Result<&mut V, TrieNodeODRc<V>> {

    //     //GOAT, I am recursively creating DenseByteNodes to the end, temporarily until I add a better
    //     // tail node type
    //     let mut cur = self;
    //     for i in 0..key.len() - 1 {
    //         let new_node = Self::new();
    //         cur = cur.add_child(key[i], TrieNodeODRc::new(new_node)).as_dense_mut().unwrap();
    //     }

    //     //This implementation will never return Err, because the DenseByteNode can hold any possible value
    //     Ok(cur.update_val(key[key.len()-1], default_f))
    // }
    fn node_set_branch(&mut self, key: &[u8], new_node: TrieNodeODRc<V>) -> Result<bool, TrieNodeODRc<V>> {
        debug_assert!(key.len() > 0);
        #[cfg(not(feature = "all_dense_nodes"))]
        {
            //Make a new ListNode to hold everything after the first byte of the key
            if key.len() > 1 {
                let mut child = LineListNode::new();
                child.node_set_branch(&key[1..], new_node).unwrap_or_else(|_| panic!());
                self.set_child(key[0], TrieNodeODRc::new(child));
                Ok(true)
            } else {
                self.set_child(key[0], new_node);
                Ok(false)
            }
        }

        #[cfg(feature = "all_dense_nodes")]
        {
            let (last_node, sub_branch_added) = if key.len() > 1 {
                (self.create_parent_path(key), true)
            } else {
                (self, false)
            };
            last_node.set_child(key[key.len()-1], new_node);
            Ok(sub_branch_added)
        }
    }
    fn node_remove_all_branches(&mut self, key: &[u8]) -> bool {
        if key.len() > 1 {
            return false;
        }
        debug_assert_eq!(key.len(), 1);
        let k = key[0];
        if self.contains(k) {
            let ix = self.left(k) as usize;
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            match (cf.rec.is_some(), cf.value.is_some()) {
                (true, true) => {
                    cf.rec = None;
                    true
                },
                (true, false) => {
                    self.values.remove(ix);
                    self.clear(k);
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
        self.mask[0] as u128
    }
    #[inline(always)]
    fn iter_token_for_path(&self, key: &[u8]) -> (u128, &[u8]) {
        if key.len() != 1 {
            (self.new_iter_token(), &[])
        } else {
            let k = *unsafe{ key.get_unchecked(0) } as usize;
            let idx = (k & 0b11000000) >> 6;
            let bit_i = k & 0b00111111;
            debug_assert!(idx < 4);
            let mask: u64 = if bit_i+1 < 64 {
                (0xFFFFFFFFFFFFFFFF << bit_i+1) & unsafe{ self.mask.get_unchecked(idx) }
            } else {
                0
            };
            (((idx as u128) << 64) | (mask as u128), &ALL_BYTES[k..=k])
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
                return (new_token, &ALL_BYTES[k..=k], cf.rec.as_ref(), cf.value.as_ref())

            } else if i < 3 {
                i += 1;

                w = unsafe { *self.mask.get_unchecked(i as usize) };
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
            t + cf.value.is_some() as usize + cf.rec.as_ref().map(|r| val_count_below_node(r, cache)).unwrap_or(0)
        });
    }
    #[cfg(feature = "counters")]
    fn item_count(&self) -> usize {
        let mut cnt = 0;
        for cf in self.values.iter() {
            cnt += cf.rec.is_some() as usize;
            cnt += cf.value.is_some() as usize;
        }
        cnt
    }
    fn node_first_val_depth_along_key(&self, key: &[u8]) -> Option<usize> {
        debug_assert!(key.len() > 0);
        self.get(key[0]).and_then(|cf| {
            if cf.value.is_some() {
                Some(0)
            } else {
                None
            }
        })
    }
    fn nth_child_from_key(&self, key: &[u8], n: usize) -> (Option<u8>, Option<&dyn TrieNode<V>>) {
        debug_assert_eq!(key.len(), 0);
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

        // #iterations can be reduced by popcount(mask[i] & prefix)

        //Find the nth entry
        let mut child = -1;
        // this is not DRY but I lost the fight to the Rust compiler
        let pair = if FORWARD { self.values.iter().enumerate().find(|_| {
            child += 1; child == (n as i32)
        }) } else { self.values.iter().rev().enumerate().find(|_| {
            child += 1; child == (n as i32)
        }) };

        //Figure out which prefix corresponds to that entry
        match pair {
            None => { return (None, None) }
            Some((item, cf)) => {
                (self.item_idx_to_prefix::<FORWARD>(item), cf.rec.as_ref().map(|cf| &*cf.borrow()))
            }
        }
    }

    fn first_child_from_key(&self, key: &[u8]) -> (Option<&[u8]>, Option<&dyn TrieNode<V>>) {
        debug_assert_eq!(key.len(), 0);
        debug_assert!(self.values.len() > 0);

        let cf = unsafe{ self.values.get_unchecked(0) };
        let prefix = self.item_idx_to_prefix::<true>(0).unwrap() as usize;
        (Some(&ALL_BYTES[prefix..=prefix]), cf.rec.as_ref().map(|cf| &*cf.borrow()))
    }

    fn node_remove_unmasked_branches(&mut self, key: &[u8], mask: [u64; 4]) {
        debug_assert!(key.len() == 0);
        // in the future we can use `drain_filter`, but that's experimental
        let mut lead = 0;
        let mut differs = false;
        let mut c = 0;
        let mvalues = self.values.as_ptr().cast_mut();

        unsafe {
            for i in 0..4 {
                let mut lm = self.mask[i];
                while lm != 0 {
                    let index = lm.trailing_zeros();
                    if ((1u64 << index) & mask[i]) != 0 {
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
        self.mask = [self.mask[0] & mask[0], self.mask[1] & mask[1], self.mask[2] & mask[2], self.mask[3] & mask[3]];
    }

    #[inline(always)]
    fn node_branches_mask(&self, key: &[u8]) -> [u64; 4] {
        match key.len() {
            0 => self.mask,
            _ => {
                //There are two ways we could get a length >= 1 key passed in. 1. The entry is a lone value (no children in the CF) or 2. The entry doesn't exist.  Either way, there are no onward child paths
                debug_assert!(self.get(key[0]).and_then(|cf| cf.rec.as_ref()).is_none());
                [0; 4]
            },
        }
    }

    #[inline(always)]
    fn count_branches(&self, key: &[u8]) -> usize {
        match key.len() {
            0 => self.values.len(),
            _ => {
                //There are two ways we could get a length >=1 key passed in. 1. The entry is a lone value (no children in the CF) or 2. The entry doesn't exist.  Either way, there are no onward child paths
                debug_assert!(self.get(key[0]).and_then(|cf| cf.rec.as_ref()).is_none());
                0
            }
        }
    }

    #[inline(always)]
    fn is_leaf(&self, key: &[u8]) -> bool {
        match key.len() {
            0 => self.values.len() == 0,
            1 => self.get(key[0]).map(|cf| !cf.rec.is_some()).unwrap_or(true),
            _ => true
        }
    }

    fn prior_branch_key(&self, key: &[u8]) -> &[u8] {
        debug_assert!(key.len() == 1);
        &[]
    }

    fn get_sibling_of_child(&self, key: &[u8], next: bool) -> (Option<u8>, Option<&dyn TrieNode<V>>) {
        if key.len() != 1 {
            return (None, None)
        }
        let k = key[0];
        let mut mask_i = ((k & 0b11000000) >> 6) as usize;
        let bit_i = k & 0b00111111;
        // println!("k {k}");

        let mut n = bit_sibling(bit_i, self.mask[mask_i], !next);
        // println!("{} {bit_i} {mask_i}", n == bit_i);
        if n == bit_i { // outside of word
            loop {
                if next { mask_i += 1 } else { mask_i -= 1 };
                if !(mask_i < 4) { return (None, None) }
                if self.mask[mask_i] == 0 { continue }
                n = self.mask[mask_i].trailing_zeros() as u8; break;
            }
        }

        // println!("{} {bit_i} {mask_i}", n == bit_i);
        // println!("{:?}", parent.items().map(|(k, _)| k).collect::<Vec<_>>());
        let sibling_key_char = n | ((mask_i << 6) as u8);
        // println!("candidate {}", sk);
        debug_assert!(self.contains(sibling_key_char));
        let cf = unsafe{ self.get_unchecked(sibling_key_char) };
        (Some(sibling_key_char), cf.rec.as_ref().map(|node| &*node.borrow()))
    }

    fn get_node_at_key(&self, key: &[u8]) -> AbstractNodeRef<V> {
        if key.len() < 2 {
            if key.len() == 0 {
                AbstractNodeRef::BorrowedDyn(self)
            } else {
                match self.get(key[0]).and_then(|cf| cf.rec.as_ref()) {
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
                Some(cf) => {
                    let mut result = None;
                    core::mem::swap(&mut result, &mut cf.rec);
                    result
                },
                None => None
            }
        } else {
            None
        }
    }

    fn join_dyn(&self, other: &dyn TrieNode<V>) -> TrieNodeODRc<V> where V: Lattice {
        if let Some(other_dense_node) = other.as_dense() {
            let new_node = self.join(other_dense_node);
            TrieNodeODRc::new(new_node)
        } else {
            if let Some(other_list_node) = other.as_list() {
                let mut new_node = self.clone();
                merge_into_dense_node(&mut new_node, other_list_node);
                TrieNodeODRc::new(new_node)
            } else {
                unreachable!()
            }
        }
    }

    fn join_into_dyn(&mut self, mut other: TrieNodeODRc<V>) where V: Lattice {
        let other_node = other.make_mut();
        if let Some(other_dense_node) = other_node.as_dense_mut() {
            self.join_into(core::mem::take(other_dense_node));
        } else {
            if let Some(other_list_node) = other.borrow().as_list() {
                //GOAT, optimization opportunity to take the contents from the list, rather than cloning
                // them, to turn around and drop the ListNode and free them / decrement the refcounts
                merge_into_dense_node(self, other_list_node);
            } else {
                unreachable!()
            }
        }
    }

    fn drop_head_dyn(&mut self, byte_cnt: usize) -> Option<TrieNodeODRc<V>> where V: Lattice {
        match self.values.len() {
            0 => { None },
            1 => {
                //WARNING: Don't be tempted to swap the node itself with its first child.  This feels like it
                // might be an optimization, but it would be a memory leak because the other node will now
                // hold an Rc to itself.
                match self.values.pop().unwrap().rec {
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
                        cf.rec.and_then(|mut child| child.make_mut().drop_head_dyn(byte_cnt-1))
                    } else {
                        cf.rec
                    };
                    match child {
                        Some(child) => {
                            new_node.join_into_dyn(child);
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

    fn meet_dyn(&self, other: &dyn TrieNode<V>) -> Option<TrieNodeODRc<V>> where V: Lattice {
        if let Some(other_dense_node) = other.as_dense() {
            let new_node = self.meet(other_dense_node);
            if !new_node.is_empty() {
                Some(TrieNodeODRc::new(new_node))
            } else {
                None
            }
        } else {
            other.meet_dyn(self)
        }
    }

    fn psubtract_dyn(&self, other: &dyn TrieNode<V>) -> (bool, Option<TrieNodeODRc<V>>) where V: PartialDistributiveLattice {
        if let Some(other_dense_node) = other.as_dense() {
            let new_node = self.subtract(other_dense_node);
            if new_node.is_empty() {
                (false, None)
            } else {
                //GOAT!!!! Optimization opportunity.  We want to carry a dirty flag out of `self.subtract`
                // and return if nothing was subtracted, rather than `false` !!!!!!!!!!!
                (false, Some(TrieNodeODRc::new(new_node)))
            }
        } else {
            self.psubtract_abstract(other)
        }
    }

    fn prestrict_dyn(&self, other: &dyn TrieNode<V>) -> Option<TrieNodeODRc<V>> {
        if let Some(other_dense_node) = other.as_dense() {
            self.prestrict(other_dense_node).map(|node| TrieNodeODRc::new(node))
        } else {
            let (_, restricted) = self.prestrict_abstract(other);
            // GOAT, Optimization opportunity to return a "reuse node unmodified" flag
            restricted
        }
    }

    fn as_dense(&self) -> Option<&DenseByteNode<V>> {
        Some(self)
    }
    fn as_dense_mut(&mut self) -> Option<&mut DenseByteNode<V>> {
        Some(self)
    }
    fn as_list(&self) -> Option<&LineListNode<V>> {
        None
    }
    fn as_list_mut(&mut self) -> Option<&mut LineListNode<V>> {
        None
    }
    fn clone_self(&self) -> TrieNodeODRc<V> {
        TrieNodeODRc::new(self.clone())
    }
    #[inline(always)]
    fn as_tagged(&self) -> TaggedNodeRef<V> {
        TaggedNodeRef::DenseByteNode(self)
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

#[derive(Default, Clone, Debug)]
pub struct CoFree<V> {
    pub(crate) rec: Option<TrieNodeODRc<V>>,
    pub(crate) value: Option<V>
}

impl<V> CoFree<V> {
    fn new() -> Self {
        Self {rec: None, value: None}
    }
}

impl<V : Clone + Lattice> Lattice for CoFree<V> {
    fn join(&self, other: &Self) -> Self {
        CoFree {
            rec: self.rec.join(&other.rec),
            value: self.value.join(&other.value)
        }
    }

    fn join_into(&mut self, other: Self) {
        self.rec.join_into(other.rec);
        self.value.join_into(other.value);
    }

    fn meet(&self, other: &Self) -> Self {
        CoFree {
            rec: self.rec.meet(&other.rec),
            value: self.value.meet(&other.value)
        }
    }

    fn bottom() -> Self {
        CoFree::new()
    }
}

impl<V: Clone + PartialDistributiveLattice> PartialDistributiveLattice for CoFree<V> {
    fn psubtract(&self, other: &Self) -> Option<Self> where Self: Sized {
        let r = self.rec.psubtract(&other.rec);
        let v = self.value.subtract(&other.value);
        match r {
            None => { if v.is_none() { None } else { Some(CoFree{ rec: None, value: v }) } }
            Some(sr) => { Some(CoFree{ rec: sr, value: v }) }
        }
    }
}

impl <V: Clone> PartialQuantale for CoFree<V> {
    fn prestrict(&self, other: &Self) -> Option<Self> where Self: Sized {
        // unsafe { println!("prestrict cofree {:?} {:?}", std::mem::transmute::<&CoFree<V>, &CoFree<u64>>(self), std::mem::transmute::<&CoFree<V>, &CoFree<u64>>(other)); }
        if other.value.is_some() { Some(self.clone()) } // assumes self can not be CoFree{None, None}
        else {
            match (&self.rec, &other.rec) {
                (Some(l), Some(r)) => {
                    l.prestrict(r).map(|n| CoFree { rec: Some(n), value: None })
                }
                _ => { None }
            }
        }
    }
}

impl<V: Lattice + Clone> Lattice for DenseByteNode<V> {
    // #[inline(never)]
    fn join(&self, other: &Self) -> Self {
        let jm: [u64; 4] = [self.mask[0] | other.mask[0],
            self.mask[1] | other.mask[1],
            self.mask[2] | other.mask[2],
            self.mask[3] | other.mask[3]];
        let mm: [u64; 4] = [self.mask[0] & other.mask[0],
            self.mask[1] & other.mask[1],
            self.mask[2] & other.mask[2],
            self.mask[3] & other.mask[3]];

        let jmc = [jm[0].count_ones(), jm[1].count_ones(), jm[2].count_ones(), jm[3].count_ones()];

        let len = (jmc[0] + jmc[1] + jmc[2] + jmc[3]) as usize;
        let mut v: Vec<CoFree<V>> = Vec::with_capacity(len);
        let new_v = v.spare_capacity_mut();

        let mut l = 0;
        let mut r = 0;
        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm[i];
            while lm != 0 {
                // this body runs at most 256 times, in the case there is 100% overlap between full nodes
                let index = lm.trailing_zeros();
                // println!("{}", index);
                if ((1u64 << index) & mm[i]) != 0 {
                    let lv = unsafe { self.values.get_unchecked(l) };
                    let rv = unsafe { other.values.get_unchecked(r) };
                    let jv = lv.join(rv);
                    debug_assert!(jv.rec.is_some() || jv.value.is_some());
                    // println!("pushing lv rv j {:?} {:?} {:?}", lv, rv, jv);
                    unsafe { new_v.get_unchecked_mut(c).write(jv) };
                    l += 1;
                    r += 1;
                } else if ((1u64 << index) & self.mask[i]) != 0 {
                    let lv = unsafe { self.values.get_unchecked(l) };
                    // println!("pushing lv {:?}", lv);
                    unsafe { new_v.get_unchecked_mut(c).write(lv.clone()) };
                    l += 1;
                } else {
                    let rv = unsafe { other.values.get_unchecked(r) };
                    // println!("pushing rv {:?}", rv);
                    unsafe { new_v.get_unchecked_mut(c).write(rv.clone()) };
                    r += 1;
                }
                lm ^= 1u64 << index;
                c += 1;
            }
        }

        unsafe{ v.set_len(c); }
        return DenseByteNode::<V>{ mask: jm, values: <_>::from(v) };
    }

    fn join_into(&mut self, mut other: Self) {
        let jm: [u64; 4] = [self.mask[0] | other.mask[0],
            self.mask[1] | other.mask[1],
            self.mask[2] | other.mask[2],
            self.mask[3] | other.mask[3]];
        let mm: [u64; 4] = [self.mask[0] & other.mask[0],
            self.mask[1] & other.mask[1],
            self.mask[2] & other.mask[2],
            self.mask[3] & other.mask[3]];

        let jmc = [jm[0].count_ones(), jm[1].count_ones(), jm[2].count_ones(), jm[3].count_ones()];

        let l = (jmc[0] + jmc[1] + jmc[2] + jmc[3]) as usize;
        let mut v = Vec::with_capacity(l);
        let new_v = v.spare_capacity_mut();

        let mut l = 0;
        let mut r = 0;
        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm[i];
            while lm != 0 {
                // this body runs at most 256 times, in the case there is 100% overlap between full nodes
                let index = lm.trailing_zeros();
                // println!("{}", index);
                if ((1u64 << index) & mm[i]) != 0 {
                    let mut lv = unsafe { std::ptr::read(self.values.get_unchecked_mut(l)) };
                    let rv = unsafe { std::ptr::read(other.values.get_unchecked_mut(r)) };
                    lv.join_into(rv);
                    unsafe { new_v.get_unchecked_mut(c).write(lv) };
                    l += 1;
                    r += 1;
                } else if ((1u64 << index) & self.mask[i]) != 0 {
                    let lv = unsafe { std::ptr::read(self.values.get_unchecked_mut(l)) };
                    unsafe { new_v.get_unchecked_mut(c).write(lv) };
                    l += 1;
                } else {
                    let rv = unsafe { std::ptr::read(other.values.get_unchecked_mut(r)) };
                    unsafe { new_v.get_unchecked_mut(c).write(rv) };
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
        self.values = <_>::from(v);
    }

    fn meet(&self, other: &Self) -> Self {
        // TODO this technically doesn't need to calculate and iterate over jm
        // iterating over mm and calculating m such that the following suffices
        // c_{self,other} += popcnt(m & {self,other})
        let jm: [u64; 4] = [self.mask[0] | other.mask[0],
            self.mask[1] | other.mask[1],
            self.mask[2] | other.mask[2],
            self.mask[3] | other.mask[3]];
        let mut mm: [u64; 4] = [self.mask[0] & other.mask[0],
            self.mask[1] & other.mask[1],
            self.mask[2] & other.mask[2],
            self.mask[3] & other.mask[3]];

        let mmc = [mm[0].count_ones(), mm[1].count_ones(), mm[2].count_ones(), mm[3].count_ones()];

        let len = (mmc[0] + mmc[1] + mmc[2] + mmc[3]) as usize;
        let mut v = Vec::with_capacity(len);
        let new_v = v.spare_capacity_mut();

        let mut l = 0;
        let mut r = 0;
        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm[i];
            while lm != 0 {
                let index = lm.trailing_zeros();

                if ((1u64 << index) & mm[i]) != 0 {
                    let lv = unsafe { self.values.get_unchecked(l) };
                    let rv = unsafe { other.values.get_unchecked(r) };
                    let jv = lv.meet(rv);
                    if jv.rec.is_some() || jv.value.is_some() {
                        unsafe { new_v.get_unchecked_mut(c).write(jv) };
                        c += 1;
                    } else {
                        mm[i] ^= 1u64 << index;
                    }
                    l += 1;
                    r += 1;
                } else if ((1u64 << index) & self.mask[i]) != 0 {
                    l += 1;
                } else {
                    r += 1;
                }
                lm ^= 1u64 << index;
            }
        }

        unsafe{ v.set_len(c); }
        return DenseByteNode::<V>{ mask: mm, values: <_>::from(v) };
    }

    fn bottom() -> Self {
        DenseByteNode::new()
    }

    fn join_all(xs: &[&Self]) -> Self {
        let mut jm: [u64; 4] = [0, 0, 0, 0];
        for x in xs.iter() {
            jm[0] |= x.mask[0];
            jm[1] |= x.mask[1];
            jm[2] |= x.mask[2];
            jm[3] |= x.mask[3];
        }

        let jmc = [jm[0].count_ones(), jm[1].count_ones(), jm[2].count_ones(), jm[3].count_ones()];

        let len = (jmc[0] + jmc[1] + jmc[2] + jmc[3]) as usize;
        let mut v = Vec::with_capacity(len);
        let new_v = v.spare_capacity_mut();

        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm[i];
            while lm != 0 {
                // this body runs at most 256 times, in the case there is 100% overlap between full nodes
                let index = lm.trailing_zeros();

                let to_join: Vec<&CoFree<V>> = xs.iter().enumerate().filter_map(|(i, x)| x.get(i as u8)).collect();
                let joined = Lattice::join_all(&to_join[..]);
                unsafe { new_v.get_unchecked_mut(c).write(joined) };

                lm ^= 1u64 << index;
                c += 1;
            }
        }

        unsafe{ v.set_len(c); }
        return DenseByteNode::<V>{ mask: jm, values: <_>::from(v) };
    }
}

impl <V : PartialDistributiveLattice + Clone> DistributiveLattice for DenseByteNode<V> {
    fn subtract(&self, other: &Self) -> Self {
        let mut btn = self.clone();

        for i in 0..4 {
            let mut lm = self.mask[i];
            while lm != 0 {
                let index = lm.trailing_zeros();

                if ((1u64 << index) & other.mask[i]) != 0 {
                    let lv = unsafe { self.get_unchecked(64*(i as u8) + (index as u8)) };
                    let rv = unsafe { other.get_unchecked(64*(i as u8) + (index as u8)) };
                    match lv.psubtract(rv) {
                        None => {
                            btn.remove(64*(i as u8) + (index as u8));
                        },
                        Some(jv) => {
                            let dst = unsafe { btn.get_unchecked_mut(64*(i as u8) + (index as u8)) };
                            *dst = jv;
                        }
                    }
                }

                lm ^= 1u64 << index;
            }
        }

        return btn;
    }
}

impl <V : PartialDistributiveLattice + Clone> PartialDistributiveLattice for DenseByteNode<V> {
    fn psubtract(&self, other: &Self) -> Option<Self> where Self: Sized {
        let r = self.subtract(other);
        if r.len() == 0 { return None }
        else { return Some(r) }
    }
}

impl <V: Clone> PartialQuantale for DenseByteNode<V> {
    fn prestrict(&self, other: &Self) -> Option<Self> where Self: Sized {
        // TODO this technically doesn't need to calculate and iterate over jm
        // iterating over mm and calculating m such that the following suffices
        // c_{self,other} += popcnt(m & {self,other})
        let jm: [u64; 4] = [self.mask[0] | other.mask[0],
            self.mask[1] | other.mask[1],
            self.mask[2] | other.mask[2],
            self.mask[3] | other.mask[3]];
        let mut mm: [u64; 4] = [self.mask[0] & other.mask[0],
            self.mask[1] & other.mask[1],
            self.mask[2] & other.mask[2],
            self.mask[3] & other.mask[3]];

        let mmc = [mm[0].count_ones(), mm[1].count_ones(), mm[2].count_ones(), mm[3].count_ones()];

        let len = (mmc[0] + mmc[1] + mmc[2] + mmc[3]) as usize;
        let mut v = Vec::with_capacity(len);
        let new_v = v.spare_capacity_mut();

        let mut l = 0;
        let mut r = 0;
        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm[i];
            while lm != 0 {
                let index = lm.trailing_zeros();

                if ((1u64 << index) & mm[i]) != 0 {
                    let lv = unsafe { self.values.get_unchecked(l) };
                    let rv = unsafe { other.values.get_unchecked(r) };
                    // println!("dense prestrict {}", index as usize + i*64);
                    if let Some(jv) = lv.prestrict(rv) {
                        unsafe { new_v.get_unchecked_mut(c).write(jv) };
                        c += 1;
                    } else {
                        mm[i] ^= 1u64 << index;
                    }
                    l += 1;
                    r += 1;
                } else if ((1u64 << index) & self.mask[i]) != 0 {
                    l += 1;
                } else {
                    r += 1;
                }
                lm ^= 1u64 << index;
            }
        }

        if c == 0 { None }
        else {
            unsafe{ v.set_len(c); }
            return Some(DenseByteNode::<V>{ mask: mm, values: <_>::from(v) });
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
