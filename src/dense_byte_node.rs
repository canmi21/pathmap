
use core::fmt::{Debug, Formatter};
use core::ptr;
use std::collections::HashMap;

use crate::ring::*;

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

use crate::trie_node::*;
use crate::line_list_node::LineListNode;

//NOTE: This: `core::array::from_fn(|i| i as u8);` ought to work, but https://github.com/rust-lang/rust/issues/109341
const ALL_BYTES: [u8; 256] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239, 240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 254, 255];

/// A ByteNode with insides that **cannot* be shared across threads
pub type DenseByteNode<V> = ByteNode<OrdinaryCoFree<V>>;

/// A ByteNode with insides that **can** be shared across threads
pub type CellByteNode<V> = ByteNode<CellCoFree<V>>;

#[derive(Clone)]
pub struct ByteNode<Cf> {
    pub mask: [u64; 4],
    #[cfg(all(feature = "all_dense_nodes", feature = "smallvec"))]
    values: SmallVec<[CoFree<V>; 2]>,
    #[cfg(all(not(feature = "all_dense_nodes"), feature = "smallvec"))]
    values: SmallVec<[CoFree<V>; 4]>,
    #[cfg(not(feature = "smallvec"))]
    values: Vec<Cf>,
}

impl<V: Clone + Send + Sync, Cf: CoFree<V=V>> Default for ByteNode<Cf> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Clone + Send + Sync, Cf: CoFree<V=V>> Debug for ByteNode<Cf> {
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

impl<V: Clone + Send + Sync, Cf: CoFree<V=V>> ByteNode<Cf> {
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

    /// Adds a new child at the specified key byte.  Replaces and returns an existing branch.
    /// Use [join_child_into] to join with the existing branch
    #[inline]
    pub fn set_child(&mut self, k: u8, node: TrieNodeODRc<V>) -> Option<TrieNodeODRc<V>> {
        let ix = self.left(k) as usize;
        if self.contains(k) {
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            cf.swap_rec(node)
        } else {
            self.set(k);
            let new_cf = CoFree::new(Some(node), None);
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
            match cf.rec_mut() {
                Some(existing_node) => {
                    let joined = existing_node.join(&node);
                    *existing_node = joined;
                },
                None => { cf.set_rec(node) }
            }
        } else {
            self.set(k);
            let new_cf = CoFree::new(Some(node), None);
            self.values.insert(ix, new_cf);
        }
    }

    #[inline]
    pub fn set_val(&mut self, k: u8, val: V) -> Option<V> {
        let ix = self.left(k) as usize;
        if self.contains(k) {
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            cf.swap_val(val)
        } else {
            self.set(k);
            let new_cf = CoFree::new(None, Some(val));
            self.values.insert(ix, new_cf);
            None
        }
    }

    #[inline]
    pub fn remove_val(&mut self, k: u8) -> Option<V> {
        let ix = self.left(k) as usize;
        debug_assert!(self.contains(k));

        let cf = unsafe { self.values.get_unchecked_mut(ix) };
        let result = cf.take_val();

        if !cf.has_rec() {
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
            match cf.val_mut() {
                Some(existing_val) => {
                    let joined = existing_val.join(&val);
                    *existing_val = joined;
                }
                None => { cf.set_val(val) }
            }
        } else {
            self.set(k);
            let new_cf = CoFree::new(None, Some(val));
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
    fn remove(&mut self, k: u8) -> Option<Cf> {
        if self.contains(k) {
            let ix = self.left(k) as usize;
            let v = self.values.remove(ix);
            self.clear(k);
            return Some(v);
        }
        None
    }

    #[inline]
    fn get(&self, k: u8) -> Option<&Cf> {
        if self.contains(k) {
            let ix = self.left(k) as usize;
            // println!("pos ix {} {} {:b}", pos, ix, self.mask);
            unsafe { Some(self.values.get_unchecked(ix)) }
        } else {
            None
        }
    }

    #[inline]
    fn get_mut(&mut self, k: u8) -> Option<&mut Cf> {
        if self.contains(k) {
            let ix = self.left(k) as usize;
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
        let ix = self.left(k) as usize;
        // println!("pos ix {} {} {:b}", pos, ix, self.mask);
        self.values.get_unchecked(ix)
    }

    #[inline]
    unsafe fn get_unchecked_mut(&mut self, k: u8) -> &mut Cf {
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
    fn item_idx_to_prefix<const FORWARD: bool>(&self, idx: usize) -> Option<u8> {
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

}

impl<V: Clone + Send + Sync, Cf: CoFree<V=V>> ByteNode<Cf> where Self: TrieNodeDowncast<V> {

    /// Internal method to subtract nodes of an abstract type from the node
    fn psubtract_abstract(&self, other: &dyn TrieNode<V>) -> (bool, Option<TrieNodeODRc<V>>) where V: Clone + PartialDistributiveLattice {
        let mut new_node = Self::new();

        //Go over each populated entry in the node
        self.for_each_item(|self_node, key_byte, cf_idx| {
            if other.node_contains_partial_key(&[key_byte]) {
                let cf = unsafe{ self_node.values.get_unchecked(cf_idx) };
                let mut new_cf = Cf::new(None, None);

                //If there is a value at this key_byte, and the other node contains a value, subtract them
                if let Some(self_val) = cf.val() {
                    if let Some(other_val) = other.node_get_val(&[key_byte]) {
                        new_cf.set_val_option(self_val.psubtract(other_val));
                    }
                }

                //If there is an onward link, see if there is a matching link in other, and subtract them
                if let Some(self_child) = cf.rec() {
                    let other_child = other.get_node_at_key(&[key_byte]);
                    match other_child.try_borrow() {
                        Some(other_child) => {
                            let difference = self_child.borrow().psubtract_dyn(other_child);
                            if difference.0 {
                                new_cf.set_rec(self_child.clone());
                            } else {
                                new_cf.set_rec_option(difference.1);
                            }
                        },
                        None => {
                            new_cf.set_rec(self_child.clone())
                        }
                    }
                }

                //If we ended up with a value or a link in the CF, insert it into a new node
                if new_cf.has_rec() || new_cf.has_val() {
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
                    if let Some(self_child) = cf.rec() {
                        let other_child = other.get_node_at_key(&[key_byte]);
                        match other_child.try_borrow() {
                            Some(other_child) => {
                                let restricted = self_child.borrow().prestrict_dyn(other_child);
                                let mut new_cf = Cf::new(None, None);
                                // if restricted.0 {
                                //     new_cf.rec = Some(self_child.clone());
                                // } else {
                                //     new_cf.rec = restricted.1;
                                // }
                                new_cf.set_rec_option(restricted); //GOAT, optimization opportunity to track when we can reuse a whole node unmodified. See commented out code above.
                                if new_cf.has_rec() {
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

    /// Merges the entries in the ListNode into the ByteNode
    pub fn merge_from_list_node(&mut self, list_node: &LineListNode<V>) where V: Clone + Lattice {
        self.reserve_capacity(2);

        if list_node.is_used::<0>() {
            let key = unsafe{ list_node.key_unchecked::<0>() };
            let payload = list_node.clone_payload::<0>().unwrap();
            if key.len() > 1 {
                let mut child_node = LineListNode::<V>::new();
                unsafe{ child_node.set_payload_owned::<0>(&key[1..], payload); }
                self.join_child_into(key[0], TrieNodeODRc::new(child_node));
            } else {
                self.join_payload_into(key[0], payload);
            }
        }

        if list_node.is_used::<1>() {
            let key = unsafe{ list_node.key_unchecked::<1>() };
            let payload = list_node.clone_payload::<1>().unwrap();
            if key.len() > 1 {
                let mut child_node = LineListNode::<V>::new();
                unsafe{ child_node.set_payload_owned::<0>(&key[1..], payload); }
                self.join_child_into(key[0], TrieNodeODRc::new(child_node));
            } else {
                self.join_payload_into(key[0], payload);
            }
        }
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
        match self.contains(k) {
            true => {},
            false => {
                let ix = self.left(k) as usize;
                self.set(k);
                let new_cf = CellCoFree::new(None, None);
                self.values.insert(ix, new_cf);
            }
        }
        let cf = self.get_mut(k).unwrap();
        let (rec, val) = cf.both_mut_refs();
        let rec = match rec {
            Some(rec) => rec,
            None => {
                *rec = Some(TrieNodeODRc::new(crate::empty_node::EmptyNode::new()));
                rec.as_mut().unwrap()
            }
        };
        (rec, val)
    }
}

/*
const BASE: i32 = 256;

fn pattern(n: i32) -> Vec<i32> {
    (0..BASE * n).step_by(n as usize).map(|k| k % BASE).collect()
}

fn repetition(n: i32) -> Vec<i32> {
    let mut result = Vec::new();
    let mut last = 0;
    let mut count = 1;

    for x in (n..(BASE * n)).step_by(n as usize) {
        let xb = x/BASE;
        if xb == last { count += 1; }
        else {
            last = xb;
            result.push(count);
            count = 1;
        }
    }
    result.push(count);
    result
}

// fn expand<'a, I>(it: I, m: &[i32]) -> impl Iterator<Item = I::Item> + 'a
//     where
//         I: Iterator + Clone + 'a,
//         I::Item: 'a,
// {
//     it.zip(m.iter()).flat_map(|(e, &i)| repeat(e).take(i as usize))
// }

fn nth<I>(mut it: I, n: usize) -> I::Item
    where
        I: Iterator,
{
    for _ in 0..n-1 {
        it.next();
    }
    it.next().unwrap()
}

fn one_up(pat: &[i32], n: i32) -> Vec<i32> {
    let mut seq = Vec::new();
    let mut c = 0;
    let mut it = pat.iter().scan(0, |state, &x| {
        *state += x;
        Some(*state)
    }).cycle();

    for _ in 0..n {
        let i = nth(&mut it, BASE as usize);
        seq.push(i - c);
        c = i;
    }

    seq
}

pub(crate) fn _so_range(step: u8, order: u8) -> TrieNodeODRc<()> {
    assert!(order == 4);
    let mut lv1 = Vec::new();
    let mut pat = pattern(step as i32);
    // println!("pat {}", pat.len());
    let mut pat_it = pat.into_iter().cycle();
    let r1 = repetition(step as i32);
    // println!("rep({}) {:?}", step as i32, r1);

    for &tk in &r1 {
        let mut n = DenseByteNode::new();
        for k in pat_it.by_ref().take(tk as usize) {
            n.set_val(k as u8, ());
        }
        lv1.push(n);
    }
    // println!("fst level {}", lv1.len());
    let mut lv2 = Vec::new();
    let mut lv1_it = lv1.into_iter().cycle();
    let r2 = one_up(&r1, step as i32);
    // println!("one_up({:?})", r2);
    for &tk in &r2 {
        let mut n = DenseByteNode::new();
        for (k, c) in lv1_it.by_ref().take(tk as usize).enumerate() {
            n.set_child(k as u8, TrieNodeODRc::new(c));
        }
        lv2.push(n);
    }
    // println!("snd level {}", lv2.len());
    // Third Level
    let mut lv3 = Vec::new();
    let mut lv2_it = lv2.into_iter().cycle();
    let r3 = one_up(&r2, step as i32);
    // println!("two_up({:?})", r3);
    for &tk in &r3 {
        let mut n = DenseByteNode::new();
        for (k, c) in lv2_it.by_ref().take(tk as usize).enumerate() {
            n.set_child(k as u8, TrieNodeODRc::new(c));
        }
        lv3.push(n);
        break
    }

    let mut n = DenseByteNode::new();
    n.set_child(0, TrieNodeODRc::new(std::mem::take(&mut lv3[0])));

    return TrieNodeODRc::<()>::new(n);



    // println!("trd level {}", lv3.len());
    // // Fourth Level (Stops after first iteration)
    // let mut lv4 = Vec::new();
    // let mut lv3_it = lv3.into_iter().cycle();
    // let r4 = one_up(&r3, step as i32);
    //
    // for &tk in &r4 {
    //     let mut n = DenseByteNode::new();
    //     for (k, c) in lv3_it.by_ref().take(tk as usize).enumerate() {
    //         n.set_child(k as u8, TrieNodeODRc::new(c));
    //     }
    //     lv4.push(n);
    //
    //     break;  // Stop after the first iteration
    // }
    //
    // return TrieNodeODRc::<()>::new(std::mem::take(&mut lv4[0]));
}
*/

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
}

impl<V: Clone + Send + Sync, Cf: CoFree<V=V>> TrieNode<V> for ByteNode<Cf>
    where ByteNode<Cf>: TrieNodeDowncast<V>
{
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
            cf.rec().map(|child_node| {
                (1, &*child_node.borrow())
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
    fn node_replace_child(&mut self, key: &[u8], new_node: TrieNodeODRc<V>) -> &mut dyn TrieNode<V> {
        debug_assert!(key.len() == 1);
        let cf = self.get_mut(key[0]).unwrap();
        *cf.rec_mut().unwrap() = new_node;
        cf.rec_mut().unwrap().make_mut()
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
        if self.contains(k) {
            let ix = self.left(k) as usize;
            let cf = unsafe { self.values.get_unchecked_mut(ix) };
            match (cf.has_rec(), cf.has_val()) {
                (true, true) => {
                    cf.set_rec_option(None);
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
                return (new_token, &ALL_BYTES[k..=k], cf.rec(), cf.val())

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
            t + cf.has_val() as usize + cf.rec().map(|r| val_count_below_node(r, cache)).unwrap_or(0)
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
            if cf.has_val() {
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
                (self.item_idx_to_prefix::<FORWARD>(item), cf.rec().map(|cf| &*cf.borrow()))
            }
        }
    }

    fn first_child_from_key(&self, key: &[u8]) -> (Option<&[u8]>, Option<&dyn TrieNode<V>>) {
        debug_assert_eq!(key.len(), 0);
        debug_assert!(self.values.len() > 0);

        let cf = unsafe{ self.values.get_unchecked(0) };
        let prefix = self.item_idx_to_prefix::<true>(0).unwrap() as usize;
        (Some(&ALL_BYTES[prefix..=prefix]), cf.rec().map(|cf| &*cf.borrow()))
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
                debug_assert!(self.get(key[0]).and_then(|cf| cf.rec()).is_none());
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
        (Some(sibling_key_char), cf.rec().map(|node| &*node.borrow()))
    }

    fn get_node_at_key(&self, key: &[u8]) -> AbstractNodeRef<V> {
        if key.len() < 2 {
            if key.len() == 0 {
                AbstractNodeRef::BorrowedDyn(self)
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

    fn join_dyn(&self, other: &dyn TrieNode<V>) -> TrieNodeODRc<V> where V: Lattice {
        let other_node = other.as_tagged();
        match other_node {
            TaggedNodeRef::DenseByteNode(other_dense_node) => {
                let new_node = self.join(other_dense_node);
                TrieNodeODRc::new(new_node)
            },
            TaggedNodeRef::LineListNode(other_list_node) => {
                let mut new_node = self.clone();
                new_node.merge_from_list_node(other_list_node);
                TrieNodeODRc::new(new_node)
            },
            TaggedNodeRef::TinyRefNode(tiny_node) => {
                tiny_node.join_dyn(self)
            },
            TaggedNodeRef::CellByteNode(other_byte_node) => {
                let new_node = self.join(other_byte_node);
                TrieNodeODRc::new(new_node)
            },
            TaggedNodeRef::EmptyNode(_) => {
                //GOAT, optimization opportunity.  Could communicate here that the resultant node is a clone
                // so we could just bump the refcount rather than make a new TrieNodeODRc pointer
                TrieNodeODRc::new(self.clone())
            }
        }
    }

    fn join_into_dyn(&mut self, mut other: TrieNodeODRc<V>) where V: Lattice {
        let other_node = other.make_mut().as_tagged_mut();
        match other_node {
            TaggedNodeRefMut::DenseByteNode(other_dense_node) => {
                self.join_into(core::mem::take(other_dense_node));
            },
            TaggedNodeRefMut::LineListNode(other_list_node) => {
                //GOAT, optimization opportunity to take the contents from the list, rather than cloning
                // them, to turn around and drop the ListNode and free them / decrement the refcounts
                self.merge_from_list_node(other_list_node);
            },
            TaggedNodeRefMut::CellByteNode(other_byte_node) => {
                self.join_into(core::mem::take(other_byte_node));
            },
            TaggedNodeRefMut::Unsupported => { }
        } 
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
        let other_node = other.as_tagged();
        match other_node {
            TaggedNodeRef::DenseByteNode(other_dense_node) => {
                let new_node = self.meet(other_dense_node);
                if !new_node.is_empty() {
                    Some(TrieNodeODRc::new(new_node))
                } else {
                    None
                }
            },
            TaggedNodeRef::LineListNode(other_list_node) => {
                other_list_node.meet_dyn(self)
            },
            TaggedNodeRef::TinyRefNode(tiny_node) => {
                tiny_node.meet_dyn(self)
            },
            TaggedNodeRef::CellByteNode(other_byte_node) => {
                let new_node = self.meet(other_byte_node);
                if !new_node.is_empty() {
                    Some(TrieNodeODRc::new(new_node))
                } else {
                    None
                }
            },
            TaggedNodeRef::EmptyNode(_) => None
        }
    }

    fn psubtract_dyn(&self, other: &dyn TrieNode<V>) -> (bool, Option<TrieNodeODRc<V>>) where V: PartialDistributiveLattice {
        let other_node = other.as_tagged();
        match other_node {
            TaggedNodeRef::DenseByteNode(other_dense_node) => {
                let new_node = self.subtract(other_dense_node);
                if new_node.is_empty() {
                    (false, None)
                } else {
                    //GOAT!!!! Optimization opportunity.  We want to carry a dirty flag out of `self.subtract`
                    // and return if nothing was subtracted, rather than `false` !!!!!!!!!!!
                    (false, Some(TrieNodeODRc::new(new_node)))
                }
            },
            TaggedNodeRef::LineListNode(other_list_node) => {
                self.psubtract_abstract(other_list_node)
            },
            TaggedNodeRef::TinyRefNode(tiny_node) => {
                self.psubtract_abstract(tiny_node)
            },
            TaggedNodeRef::CellByteNode(other_byte_node) => {
                let new_node = self.subtract(other_byte_node);
                if new_node.is_empty() {
                    (false, None)
                } else {
                    //GOAT!!!! Optimization opportunity.  We want to carry a dirty flag out of `self.subtract`
                    // and return if nothing was subtracted, rather than `false` !!!!!!!!!!!
                    (false, Some(TrieNodeODRc::new(new_node)))
                }
            },
            TaggedNodeRef::EmptyNode(_) => (true, None), //Preserve original value unmodified
        }
    }

    fn prestrict_dyn(&self, other: &dyn TrieNode<V>) -> Option<TrieNodeODRc<V>> {
        let other_node = other.as_tagged();
        match other_node {
            TaggedNodeRef::DenseByteNode(other_dense_node) => {
                self.prestrict(other_dense_node).map(|node| TrieNodeODRc::new(node))
            },
            TaggedNodeRef::LineListNode(other_list_node) => {
                let (_, restricted) = self.prestrict_abstract(other_list_node);
                // GOAT, Optimization opportunity to return a "reuse node unmodified" flag
                restricted
            },
            TaggedNodeRef::TinyRefNode(tiny_node) => {
                let (_, restricted) = self.prestrict_abstract(tiny_node);
                // GOAT, Optimization opportunity to return a "reuse node unmodified" flag
                restricted
            },
            TaggedNodeRef::CellByteNode(other_byte_node) => {
                self.prestrict(other_byte_node).map(|node| TrieNodeODRc::new(node))
            },
            TaggedNodeRef::EmptyNode(_) => None,
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
        let mut values = vec![];
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

pub trait CoFree: Clone + Send + Sync {
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

#[derive(Clone, Default, Debug)]
pub struct OrdinaryCoFree<V> {
    rec: Option<TrieNodeODRc<V>>,
    value: Option<V>
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

impl<V: Clone + Send + Sync + Lattice, Cf: CoFree<V=V>, OtherCf: CoFree<V=V>> HeteroLattice<OtherCf> for Cf {
    fn join(&self, other: &OtherCf) -> Self {
        let rec = self.rec().join(&other.rec());
        let val = self.val().join(&other.val());
        CoFree::new(rec, val)
    }
    fn join_into(&mut self, other: OtherCf) {
        let (other_rec, other_val) = other.into_both();

        self.set_rec_option(self.rec().join(&other_rec.as_ref()));
        self.set_val_option(self.val().join(&other_val.as_ref()));

        //GOAT, the below implementation **Should** be more efficient, but there are some unimplemented code paths
        // match self.rec_mut() {
        //     Some(self_rec) => { other_rec.map(|other_rec| self_rec.join_into(other_rec)); },
        //     None => self.set_rec_option(other_rec)
        // }
        // match self.val_mut() {
        //     Some(self_val) => { other_val.map(|other_val| self_val.join_into(other_val)); },
        //     None => self.set_val_option(other_val)
        // }
    }

    fn meet(&self, other: &OtherCf) -> Self {
        let rec = self.rec().meet(&other.rec());
        let val = self.val().meet(&other.val());
        CoFree::new(rec, val)
    }
    fn bottom() -> Self {
        CoFree::new(None, None)
    }
    fn join_all(_xs: &[&Self]) -> Self where Self: Sized {
        panic!() //Currently not used
    }
}

impl<V: Clone + PartialDistributiveLattice, Cf: CoFree<V=V>, OtherCf: CoFree<V=V>> HeteroPartialDistributiveLattice<OtherCf> for Cf {
    fn psubtract(&self, other: &OtherCf) -> Option<Self> where Self: Sized {
        let r = self.rec().psubtract(&other.rec());
        let v = self.val().psubtract(&other.val()).unwrap_or(None);
        match r {
            None => { if v.is_none() { None } else { Some(CoFree::new(None, v)) } }
            Some(sr) => { Some(CoFree::new(sr, v)) }
        }
    }
}

impl<V: Clone + PartialDistributiveLattice, Cf: CoFree<V=V>> PartialDistributiveLattice for Cf {
    fn psubtract(&self, other: &Self) -> Option<Self> where Self: Sized {
        let r = self.rec().psubtract(&other.rec());
        let v = self.val().psubtract(&other.val()).unwrap_or(None);
        match r {
            None => { if v.is_none() { None } else { Some(CoFree::new(None, v)) } }
            Some(sr) => { Some(CoFree::new(sr, v)) }
        }
    }
}

impl<V: Clone, Cf: CoFree<V=V>, OtherCf: CoFree<V=V>> HeteroPartialQuantale<OtherCf> for Cf {
    fn prestrict(&self, other: &OtherCf) -> Option<Self> where Self: Sized {
        debug_assert!(self.has_rec() || self.has_val());
        if other.has_val() { Some(self.clone()) } // assumes self can not be CoFree{None, None}
        else {
            match (self.rec(), other.rec()) {
                (Some(l), Some(r)) => {
                    l.prestrict(r).map(|n| CoFree::new(Some(n), None) )
                }
                _ => { None }
            }
        }
    }
}

impl<V: Clone + Send + Sync + Lattice, Cf: CoFree<V=V>, OtherCf: CoFree<V=V>> HeteroLattice<ByteNode<OtherCf>> for ByteNode<Cf> {
    fn join(&self, other: &ByteNode<OtherCf>) -> Self {
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
        let mut v: Vec<Cf> = Vec::with_capacity(len);
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
                    debug_assert!(jv.has_rec() || jv.has_val());
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
                    unsafe { new_v.get_unchecked_mut(c).write(<_>::from_cf(rv.clone())) };
                    r += 1;
                }
                lm ^= 1u64 << index;
                c += 1;
            }
        }

        unsafe{ v.set_len(c); }
        return Self{ mask: jm, values: <_>::from(v) };
    }

    fn join_into(&mut self, mut other: ByteNode<OtherCf>) {
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
        self.values = <_>::from(v);
    }

    fn meet(&self, other: &ByteNode<OtherCf>) -> Self {
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
                    if jv.has_rec() || jv.has_val() {
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
        return Self{ mask: mm, values: <_>::from(v) };
    }

    fn bottom() -> Self {
        Self::new()
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

                //GOAT, allocating a temp buffer likely undoes the gains from join_all
                let to_join: Vec<&Cf> = xs.iter().enumerate().filter_map(|(i, x)| x.get(i as u8)).collect();
                let joined = HeteroLattice::<Cf>::join_all(&to_join[..]);
                unsafe { new_v.get_unchecked_mut(c).write(joined) };

                lm ^= 1u64 << index;
                c += 1;
            }
        }

        unsafe{ v.set_len(c); }
        return Self{ mask: jm, values: <_>::from(v) };
    }
}


//NOTE: This *looks* like an impl of DistributiveLattice, but it isn't, so we can have `self` and
// `other` be differently parameterized types
// impl<V: PartialDistributiveLattice + Clone + Send + Sync, Cf: CoFree<V=V>> ByteNode<Cf> {
impl<V: PartialDistributiveLattice + Clone + Send + Sync, Cf: CoFree<V=V>> ByteNode<Cf> {
    fn subtract<OtherCf: CoFree<V=V>>(&self, other: &ByteNode<OtherCf>) -> Self {
        let mut btn = self.clone();

        for i in 0..4 {
            let mut lm = self.mask[i];
            while lm != 0 {
                let index = lm.trailing_zeros();

                if ((1u64 << index) & other.mask[i]) != 0 {
                    let lv = unsafe { self.get_unchecked(64*(i as u8) + (index as u8)) };
                    let rv = unsafe { other.get_unchecked(64*(i as u8) + (index as u8)) };
                    match HeteroPartialDistributiveLattice::psubtract(lv, rv) {
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

//NOTE: This *looks* like an impl of PartialDistributiveLattice, but it isn't, so we can have `self` and
// `other` be differently parameterized types
impl<V: PartialDistributiveLattice + Clone + Send + Sync, Cf: CoFree<V=V>> ByteNode<Cf> {
    fn psubtract<OtherCf: CoFree<V=V>>(&self, other: &ByteNode<OtherCf>) -> Option<Self> where Self: Sized {
        let r = self.subtract(other);
        if r.len() == 0 { return None }
        else { return Some(r) }
    }
}

//NOTE: This *looks* like an impl of PartialQuantale, but it isn't, so we can have `self` and
// `other` be differently parameterized types
impl<V:Clone, Cf: CoFree<V=V>> ByteNode<Cf> {
    fn prestrict<OtherCf: CoFree<V=V>>(&self, other: &ByteNode<OtherCf>) -> Option<Self> where Self: Sized {
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
            return Some(Self{ mask: mm, values: <_>::from(v) });
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
