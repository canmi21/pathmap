use std::marker::PhantomData;
use std::mem;
use crate::trie_map::BytesTrieMap;
use crate::trie_node::*;
use crate::zipper::*;
use zipper_priv::*;
use crate::ring::{AlgebraicStatus, DistributiveLattice, Lattice};
use crate::TrieValue;
use crate::write_zipper::write_zipper_priv::WriteZipperPriv;

/// A [Zipper] type that moves through a Cartesian product space created by extending each value at the
/// end of a path in a primary space with the root of a secondardary space, and doing it recursively for
/// as many spaces as needed
///
/// WARNING: This zipper treats any value in an "outer" factor zipper's trie as a marker to jump to the
/// next zipper's trie.  However, the `descend_to` method will jump over values except for the leaf value.
/// This is a non-issue if the trie only contains values intended as "stitching" points, but will otherwise
/// lead to strange-seeming behavior.
///
/// WHY IS THIS API EXPERIMENTAL?  A: Because the zipper appears to create a new space for the purposes
/// of zipper movement, but the space is an ephemeral projection.  Unlike other space operations, if
/// the user tried to graft this space or materialize it into a map, they would get something that
/// would not match their expectations.
///
/// A more holistic way of performing this kind of transformation is likely desirable, but that has a
/// number of unexplored complications such as the impact to exclusivity (is this de-facto aliasing?)
/// and how the linkages could be parameterized after-the-fact, or re-parameterized en masse (without
/// visiting each node), or when the parameters could be changed and when they would be fixed.
pub struct ProductZipper<'factor_z, 'trie, V> {
    z: read_zipper_core::ReadZipperCore<'trie, 'static, V>,
    secondaries: Vec<TrieRef<'trie, V>>,
    factor_paths: Vec<usize>,
    /// We need to hang onto the zippers for the life of this object, so their trackers stay alive
    source_zippers: Vec<Box<dyn zipper_priv::ZipperReadOnlyPriv<'trie, V> + 'factor_z>>
}

impl<'factor_z, 'trie, V: Clone + Send + Sync + Unpin> ProductZipper<'factor_z, 'trie, V> {
    /// Creates a new `ProductZipper` from the provided zippers
    ///
    /// WARNING: passing `other_zippers` that are not at node roots may lead to a panic.  This is
    /// an implementation issue, but would be very difficult to fix and may not be worth fixing.
    pub fn new<PrimaryZ, OtherZ, ZipperList>(mut primary_z: PrimaryZ, other_zippers: ZipperList) -> Self
        where
        PrimaryZ: ZipperMoving + ZipperReadOnly<'trie, V> + 'factor_z,
        OtherZ: ZipperReadOnly<'trie, V> + 'factor_z,
        ZipperList: IntoIterator<Item=OtherZ>,
    {
        let other_z_iter = other_zippers.into_iter();
        let (mut secondaries, mut source_zippers) = match other_z_iter.size_hint() {
            (_, Some(hint)) => (Vec::with_capacity(hint), Vec::with_capacity(hint+1)),
            (_, None) => (Vec::new(), Vec::new())
        };

        //Get the core out of the primary zipper
        //This unwrap won't fail because all the types that implement `ZipperMoving` have cores
        let core_z = primary_z.take_core().unwrap();
        source_zippers.push(Box::new(primary_z) as Box<dyn zipper_priv::ZipperReadOnlyPriv<V>>);

        //Get TrieRefs for the remaining zippers
        for other_z in other_z_iter {
            let trie_ref = other_z.trie_ref_at_path("");
            secondaries.push(trie_ref);
            source_zippers.push(Box::new(other_z));
        }

        Self{z: core_z, factor_paths: Vec::with_capacity(secondaries.len()), secondaries, source_zippers}
    }
    /// Creates a new `ProductZipper` from a single zipper, with the expectation that more zippers
    /// will be added using [new_factor]
    pub fn new_with_primary<PrimaryZ>(mut primary_z: PrimaryZ) -> Self
        where PrimaryZ: ZipperMoving + ZipperReadOnly<'trie, V> + 'factor_z,
    {
        let mut source_zippers = Vec::new();

        //Get the core out of the primary zipper
        //This unwrap won't fail because all the types that implement `ZipperMoving` have cores
        let core_z = primary_z.take_core().unwrap();
        source_zippers.push(Box::new(primary_z) as Box<dyn zipper_priv::ZipperReadOnlyPriv<V>>);

        Self{z: core_z, factor_paths: Vec::new(), secondaries: vec![], source_zippers}
    }
    /// Appends additional factors to a `ProductZipper`.  This is useful when dealing with
    /// factor zippers of different types
    ///
    /// WARNING: the same warning as above applies about passing other zippers that aren't at node roots
    pub fn new_factors<OtherZ, ZipperList>(&mut self, other_zippers: ZipperList)
        where
        OtherZ: ZipperReadOnly<'trie, V> + 'factor_z,
        ZipperList: IntoIterator<Item=OtherZ>,
    {
        let other_z_iter = other_zippers.into_iter();
        for other_z in other_z_iter {
            let trie_ref = other_z.trie_ref_at_path("");
            self.secondaries.push(trie_ref);
            self.source_zippers.push(Box::new(other_z));
        }
    }
    #[inline]
    fn has_next_factor(&mut self) -> bool {
        self.factor_paths.len() < self.secondaries.len()
    }
    #[inline]
    fn enroll_next_factor(&mut self) {
        //If there is a `_secondary_root_val`, it lands at the same path as the value where the
        // paths are joined.  And the value from the earlier zipper takes precedence
        let (secondary_root, partial_path, _secondary_root_val) = self.secondaries[self.factor_paths.len()].borrow_raw_parts();

        //TODO! Dealing with hidden root path in a secondary factor is very nasty.  I'm going to punt
        // on handling this until we move this feature out of the experimental stage.
        //See "WARNING" in ProductZipper creation methods
        assert_eq!(partial_path.len(), 0);

        self.z.deregularize();
        self.z.push_node(secondary_root.as_tagged());
        self.factor_paths.push(self.path().len());
    }
    /// Internal method to descend across the boundary between two factor zippers if the focus is on a value
    ///
    /// The ProductZipper's internal representation can be a bit tricky.  See the documentation on
    /// `product_zipper_test4` for more discussion.
    #[inline]
    fn ensure_descend_next_factor(&mut self) {
        if self.z.is_value() && self.factor_paths.len() < self.secondaries.len() {

            //We don't want to push the same factor on the stack twice
            if let Some(factor_path_len) = self.factor_paths.last() {
                if *factor_path_len == self.path().len() {
                    return
                }
            }

            //If there is a `_secondary_root_val`, it lands at the same path as the value where the
            // paths are joined.  And the value from the earlier zipper takes precedence
            let (secondary_root, partial_path, _secondary_root_val) = self.secondaries[self.factor_paths.len()].borrow_raw_parts();

            //TODO! Dealing with hidden root path in a secondary factor is very nasty.  I'm going to punt
            // on handling this until we move this feature out of the experimental stage.
            //See "WARNING" in ProductZipper creation methods
            assert_eq!(partial_path.len(), 0);

            self.z.deregularize();
            self.z.push_node(secondary_root.as_tagged());
            self.factor_paths.push(self.path().len());
        }
    }
    /// Internal method to determine whether a given method should be applied to the zipper core, or to the next factor root
    #[inline]
    fn should_use_next_factor(&self) -> bool {
        if self.z.is_value() && self.factor_paths.len() < self.secondaries.len() {
            if let Some(path_len) = self.factor_paths.last() {
                if *path_len != self.z.path().len() {
                    return true
                }
            } else {
                return true
            }
        }
        false
    }
    /// Internal method to make sure `self.factor_paths` is correct after an ascend method
    #[inline]
    fn fix_after_ascend(&mut self) {
        while let Some(factor_path_start) = self.factor_paths.last() {
            if self.z.path().len() <= *factor_path_start {
                self.factor_paths.pop();
            } else {
                break
            }
        }
    }
}

impl<V: Clone + Send + Sync + Unpin> ZipperMoving for ProductZipper<'_, '_, V> {
    fn at_root(&self) -> bool {
        self.path().len() == 0
    }
    fn reset(&mut self) {
        self.factor_paths.clear();
        self.z.reset()
    }
    #[inline]
    fn path(&self) -> &[u8] {
        self.z.path()
    }
    fn val_count(&self) -> usize {
        //GOAT!!!  I think val_count properly belongs in the morphisms module
        unimplemented!()
    }
    fn descend_to_existing<K: AsRef<[u8]>>(&mut self, k: K) -> usize {
        let k = k.as_ref();
        if self.has_next_factor() {
            if self.is_value() && self.factor_paths.last().map(|l| *l).unwrap_or(0) < self.path().len() {
                self.enroll_next_factor();
            }
            let descended = self.z.descend_to_value(k);
            debug_assert!(descended <= k.len());
            if descended < k.len() && descended > 0 {
                descended + self.descend_to_existing(&k[descended..])
            } else {
                descended
            }
        } else {
            self.z.descend_to_existing(k)
        }
    }
    fn descend_to<K: AsRef<[u8]>>(&mut self, k: K) -> bool {
        let k = k.as_ref();
        let descended = self.descend_to_existing(k);
        if descended != k.len() {
            self.z.descend_to(&k[descended..]);
            return false
        }
        true
    }
    fn descend_to_byte(&mut self, k: u8) -> bool {
        self.descend_to(&[k])
    }
    fn descend_indexed_branch(&mut self, child_idx: usize) -> bool {
        self.ensure_descend_next_factor();
        self.z.descend_indexed_branch(child_idx)
    }
    fn descend_first_byte(&mut self) -> bool {
        self.ensure_descend_next_factor();
        self.z.descend_first_byte()
    }
    fn descend_until(&mut self) -> bool {
        self.ensure_descend_next_factor();
        self.z.descend_until()
    }
    fn to_sibling(&mut self, next: bool) -> bool {
        self.ensure_descend_next_factor();
        let moved = self.z.to_sibling(next);
        self.fix_after_ascend();
        moved
    }
    fn to_next_sibling_byte(&mut self) -> bool {
        self.ensure_descend_next_factor();
        let moved = self.z.to_next_sibling_byte();
        self.fix_after_ascend();
        moved
    }
    fn to_prev_sibling_byte(&mut self) -> bool {
        self.to_sibling(false)
    }
    fn ascend(&mut self, steps: usize) -> bool {
        let ascended = self.z.ascend(steps);
        self.fix_after_ascend();
        ascended
    }
    fn ascend_byte(&mut self) -> bool {
        self.ascend(1)
    }
    fn ascend_until(&mut self) -> bool {
        let ascended = self.z.ascend_until();
        self.fix_after_ascend();
        ascended
    }
    fn ascend_until_branch(&mut self) -> bool {
        let ascended = self.z.ascend_until_branch();
        self.fix_after_ascend();
        ascended
    }
}

impl<'factor_z, 'trie, V: Clone + Send + Sync + Unpin> ZipperReadOnly<'trie, V> for ProductZipper<'factor_z, 'trie, V>{
    fn get_value(&self) -> Option<&'trie V> { self.z.get_value() }
    fn trie_ref_at_path<K: AsRef<[u8]>>(&self, path: K) -> TrieRef<'trie, V> { self.z.trie_ref_at_path(path) }
}

impl<'factor_z, 'trie, V: Clone + Send + Sync + Unpin> zipper_priv::ZipperReadOnlyPriv<'trie, V> for ProductZipper<'factor_z, 'trie, V> {
    fn borrow_raw_parts<'z>(&'z self) -> (&'trie dyn TrieNode<V>, &'z [u8], Option<&'trie V>) {
        panic!("Making a product zipper from another product zipper no supported.  Use `new_factors` instead.");
    }
    fn take_core(&mut self) -> Option<read_zipper_core::ReadZipperCore<'trie, 'static, V>> {
        panic!("Making a product zipper from another product zipper no supported.  Use `new_factors` instead.");
    }
}

impl<V: Clone + Send + Sync + Unpin> Zipper for ProductZipper<'_, '_, V> {
    fn path_exists(&self) -> bool {
        self.z.path_exists()
    }
    fn is_shared(&self) -> bool {
        self.z.is_shared()
    }
    fn is_value(&self) -> bool {
        self.z.is_value()
    }
    fn child_count(&self) -> usize {
        if self.should_use_next_factor() {
            self.secondaries[self.factor_paths.len()].child_count()
        } else {
            self.z.child_count()
        }
    }
    fn child_mask(&self) -> [u64; 4] {
        if self.should_use_next_factor() {
            self.secondaries[self.factor_paths.len()].child_mask()
        } else {
            self.z.child_mask()
        }
    }
}

impl<V: Clone + Send + Sync + Unpin> ZipperAccess<V> for ProductZipper<'_, '_, V> {
    type ReadZipperT<'a> = () where Self: 'a;
    fn value(&self) -> Option<&V> {
        self.get_value()
    }
    fn fork_read_zipper<'a>(&'a self) -> Self::ReadZipperT<'a> {
        panic!("This won't do what you want it to do.");
    }
    fn make_map(&self) -> Option<BytesTrieMap<Self::V>> {
        panic!("This won't do what you want it to do.");
    }
}

impl<V: Clone + Send + Sync + Unpin> zipper_priv::ZipperPriv for ProductZipper<'_, '_, V> {
    type V = V;

    fn get_focus(&self) -> AbstractNodeRef<Self::V> { self.z.get_focus() }
    fn try_borrow_focus(&self) -> Option<&dyn TrieNode<Self::V>> { self.z.try_borrow_focus() }
}

impl<V: Clone + Send + Sync + Unpin> zipper_priv::ZipperMovingPriv for ProductZipper<'_, '_, V> {
    unsafe fn origin_path_assert_len(&self, len: usize) -> &[u8] { unsafe{ self.z.origin_path_assert_len(len) } }
    fn prepare_buffers(&mut self) { self.z.prepare_buffers() }
}

impl<V: Clone + Send + Sync + Unpin> ZipperAbsolutePath for ProductZipper<'_, '_, V> {
    fn origin_path(&self) -> Option<&[u8]> { self.z.origin_path() }
    fn root_prefix_path(&self) -> Option<&[u8]> { self.z.root_prefix_path() }
}

pub(crate) struct FullZipper {
    path: Vec<u8>
}

impl Zipper for FullZipper {
    fn path_exists(&self) -> bool { true }
    fn is_shared(&self) -> bool { true }
    fn is_value(&self) -> bool { true }
    fn child_count(&self) -> usize { 256 }
    fn child_mask(&self) -> [u64; 4] { [!0u64, !0u64, !0u64, !0u64] }
}

impl ZipperMovingPriv for FullZipper {
    unsafe fn origin_path_assert_len(&self, len: usize) -> &[u8] {
        assert!(len <= self.path.capacity());
        unsafe{ core::slice::from_raw_parts(self.path.as_ptr(), len) }
    }

    fn prepare_buffers(&mut self) {
        self.path.reserve(EXPECTED_PATH_LEN)
    }
}

impl ZipperMoving for FullZipper {
    fn at_root(&self) -> bool { self.path.len() == 0 }
    fn reset(&mut self) { self.path.clear() }
    fn path(&self) -> &[u8] { &self.path[..] }
    fn val_count(&self) -> usize { usize::MAX/2 } // usize::MAX is a dangerous default for overflow
    fn descend_to<K: AsRef<[u8]>>(&mut self, k: K) -> bool {
        self.path.extend_from_slice(k.as_ref());
        true
    }
    fn descend_to_byte(&mut self, k: u8) -> bool {
        self.path.push(k);
        true
    }
    fn descend_indexed_branch(&mut self, idx: usize) -> bool {
        assert!(idx < 256);
        self.path.push(idx as u8);
        true
    }
    fn descend_first_byte(&mut self) -> bool {
        self.path.push(0);
        true
    }
    fn descend_until(&mut self) -> bool {
        self.path.push(0); // not sure?
        true
    }
    fn ascend(&mut self, steps: usize) -> bool {
        if steps > self.path.len() {
            self.path.clear();
            false
        } else {
            self.path.truncate(self.path.len() - steps);
            true
        }
    }
    fn ascend_byte(&mut self) -> bool {
        self.path.pop().is_some()
    }
    fn ascend_until(&mut self) -> bool {
        self.path.pop().is_some() // not sure?
    }
    fn ascend_until_branch(&mut self) -> bool {
        self.path.pop().is_some() // not sure? What's the difference with the previous?
    }
    fn to_sibling(&mut self, next: bool) -> bool {
        if self.path.is_empty() { return false } // right?
        if next {
            let last = self.path.last_mut().unwrap();
            if *last != 255 { *last = *last + 1; true }
            else { false }
        } else {
            let first = self.path.first_mut().unwrap();
            if *first != 0 { *first = *first - 1; true }
            else { false }
        }
    }
    fn to_next_sibling_byte(&mut self) -> bool { self.to_sibling(true) }
    fn to_prev_sibling_byte(&mut self) -> bool { self.to_sibling(false) }
}

pub(crate) struct MappedZipper<A, B, RZ, F> {
    pub rz: RZ,
    pub f: F,
    pub pd: PhantomData<(A, B)>
}

impl <A : TrieValue, B : TrieValue, RZ : ZipperAccess<A>, F : Fn(&A) -> B + Copy> ZipperAccess<B> for MappedZipper<A, B, RZ, F> {
    type ReadZipperT<'a> = MappedZipper<A, B, RZ::ReadZipperT<'a>, F> where Self: 'a;

    fn value(&self) -> Option<&B> {
        // this returning a reference is actually quite problematic conceptually for this RZ operator
        // for now we just leak, a slightly more sensible design is using the cursor-like API where your value only lives between calls
        self.rz.value().map(|v| &*Box::leak(Box::new((self.f)(v))))
    }

    fn fork_read_zipper<'a>(&'a self) -> Self::ReadZipperT<'a> {
        MappedZipper{ rz: self.rz.fork_read_zipper(), f: self.f, pd: Default::default() }
    }

    fn make_map(&self) -> Option<BytesTrieMap<Self::V>> {
        self.rz.make_map().map(|btm| btm.map_values(|a| (self.f)(a)))
    }
}

impl<A : TrieValue, B : TrieValue, RZ: ZipperAccess<A>, F: Fn(&A) -> B + Copy> ZipperPriv for MappedZipper<A, B, RZ, F> {
    type V = B;

    fn get_focus(&self) -> AbstractNodeRef<Self::V> { todo!() }
    fn try_borrow_focus(&self) -> Option<&dyn TrieNode<Self::V>> { todo!() }
}


// Doesn't seem as lawful as the above, still maybe useful for testing
pub(crate) struct NullZipper {}

impl<V: TrieValue> WriteZipperPriv<V> for NullZipper {
    fn take_focus(&mut self) -> Option<TrieNodeODRc<V>> {
        None
    }
}

impl <V : TrieValue> ZipperWriting<V> for NullZipper {
    type ZipperHead<'z> = ZipperHead<'z, 'static, V> where Self: 'z;

    fn get_value_mut(&mut self) -> Option<&mut V> { None }
    fn get_value_or_insert(&mut self, default: V) -> &mut V { Box::leak(Box::new(default)) }
    fn get_value_or_insert_with<F>(&mut self, func: F) -> &mut V where F: FnOnce() -> V { Box::leak(Box::new(func())) }
    fn set_value(&mut self, _val: V) -> Option<V> { None }
    fn remove_value(&mut self) -> Option<V> { None }
    fn zipper_head<'z>(&'z mut self) -> Self::ZipperHead<'z> { todo!() }
    fn graft<Z: ZipperAccess<V>>(&mut self, _read_zipper: &Z) {}
    fn graft_map(&mut self, _map: BytesTrieMap<V>) {}
    fn join<Z: ZipperAccess<V>>(&mut self, _read_zipper: &Z) -> AlgebraicStatus where V: Lattice { AlgebraicStatus::Element }
    fn join_map(&mut self, _map: BytesTrieMap<V>) -> AlgebraicStatus where V: Lattice { AlgebraicStatus::Element }
    fn join_into<Z: ZipperAccess<V> + ZipperWriting<V>>(&mut self, _src_zipper: &mut Z) -> AlgebraicStatus where V: Lattice { AlgebraicStatus::Element }
    fn drop_head(&mut self, _byte_cnt: usize) -> bool where V: Lattice { false }
    fn insert_prefix<K: AsRef<[u8]>>(&mut self, _prefix: K) -> bool { false }
    fn remove_prefix(&mut self, _n: usize) -> bool { false }
    fn meet<Z: ZipperAccess<V>>(&mut self, _read_zipper: &Z) -> AlgebraicStatus where V: Lattice { AlgebraicStatus::Element }
    fn meet_2<'z, ZA: ZipperAccess<V>, ZB: ZipperAccess<V>>(&mut self, _rz_a: &ZA, _rz_b: &ZB) -> AlgebraicStatus where V: Lattice { AlgebraicStatus::Element }
    fn subtract<Z: ZipperAccess<V>>(&mut self, _read_zipper: &Z) -> AlgebraicStatus where V: DistributiveLattice { AlgebraicStatus::Element }
    fn restrict<Z: ZipperAccess<V>>(&mut self, _read_zipper: &Z) -> AlgebraicStatus { AlgebraicStatus::Element }
    fn restricting<Z: ZipperAccess<V>>(&mut self, _read_zipper: &Z) -> bool { false }
    fn remove_branches(&mut self) -> bool { false }
    fn take_map(&mut self) -> Option<BytesTrieMap<V>> { None }
    fn remove_unmasked_branches(&mut self, _mask: [u64; 4]) {}
}

#[cfg(test)]
mod tests {
    use crate::utils::ByteMask;
    use crate::zipper::*;
    use crate::trie_map::BytesTrieMap;
    use crate::experimental::ProductZipper;
    use crate::morphisms::{Catamorphism, SplitCata};

    /// Tests a very simple two-level product zipper
    #[test]
    fn product_zipper_test1() {
        let keys = [b"AAa", b"AAb", b"AAc"];
        let keys2 = [b"DDd", b"EEe", b"FFf"];
        let map: BytesTrieMap<u64> = keys.into_iter().enumerate().map(|(i, v)| (v, i as u64)).collect();
        let map2: BytesTrieMap<u64> = keys2.into_iter().enumerate().map(|(i, v)| (v, (i + 1000) as u64)).collect();

        let rz = map.read_zipper();
        let mut pz = ProductZipper::new(rz, [map2.read_zipper()]);

        //Descend within the first factor
        assert!(pz.descend_to(b"AA"));
        assert_eq!(pz.path(), b"AA");
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.child_count(), 3);
        assert!(pz.descend_to(b"a"));
        assert_eq!(pz.path(), b"AAa");
        assert_eq!(pz.get_value(), Some(&0));
        assert_eq!(pz.child_count(), 3);

        //Step to the next factor
        assert!(pz.descend_to(b"DD"));
        assert_eq!(pz.path(), b"AAaDD");
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.child_count(), 1);
        assert!(pz.descend_to(b"d"));
        assert_eq!(pz.path(), b"AAaDDd");
        assert_eq!(pz.get_value(), Some(&1000));
        assert_eq!(pz.child_count(), 0);
        assert!(!pz.descend_to(b"GGg"));
        assert_eq!(pz.path(), b"AAaDDdGGg");
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.child_count(), 0);

        //Test Reset, if the zipper was in another factor
        pz.reset();
        assert_eq!(pz.path(), b"");
        assert!(pz.descend_to(b"AA"));
        assert_eq!(pz.path(), b"AA");
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.child_count(), 3);

        //Try to descend to a non-existent path that would be within the first factor
        assert!(!pz.descend_to(b"aBBb"));
        assert_eq!(pz.path(), b"AAaBBb");
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.child_count(), 0);

        //Now descend to the second factor in one jump
        pz.reset();
        assert!(pz.descend_to(b"AAaDD"));
        assert_eq!(pz.path(), b"AAaDD");
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.child_count(), 1);
        pz.reset();
        assert!(pz.descend_to(b"AAaDDd"));
        assert_eq!(pz.path(), b"AAaDDd");
        assert_eq!(pz.get_value(), Some(&1000));
        assert_eq!(pz.child_count(), 0);
        assert!(!pz.descend_to(b"GG"));
        assert_eq!(pz.path(), b"AAaDDdGG");
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.child_count(), 0);

        //Make sure we can ascend out of a secondary factor; in this sub-test we'll hit the path middles
        assert!(pz.ascend(1));
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.path(), b"AAaDDdG");
        assert_eq!(pz.child_count(), 0);
        assert!(pz.ascend(3));
        assert_eq!(pz.path(), b"AAaD");
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.child_count(), 1);
        assert!(pz.ascend(2));
        assert_eq!(pz.path(), b"AA");
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.child_count(), 3);
        assert!(!pz.ascend(3));
        assert_eq!(pz.path(), b"");
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.child_count(), 1);
        assert!(pz.at_root());

        assert!(!pz.descend_to(b"AAaDDdGG"));
        assert_eq!(pz.path(), b"AAaDDdGG");
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.child_count(), 0);

        //Now try to hit the path transition points
        assert!(pz.ascend(2));
        assert_eq!(pz.path(), b"AAaDDd");
        assert_eq!(pz.get_value(), Some(&1000));
        assert_eq!(pz.child_count(), 0);
        assert!(pz.ascend(3));
        assert_eq!(pz.path(), b"AAa");
        assert_eq!(pz.get_value(), Some(&0));
        assert_eq!(pz.child_count(), 3);
        assert!(pz.ascend(3));
        assert_eq!(pz.path(), b"");
        assert_eq!(pz.get_value(), None);
        assert_eq!(pz.child_count(), 1);
        assert!(pz.at_root());
    }

    /// Tests a 3-level product zipper, with a catamorphism, and no funny-business in the tries
    #[test]
    fn product_zipper_test2() {
        let lpaths = ["abcdefghijklmnopqrstuvwxyz".as_bytes(), "arrow".as_bytes(), "x".as_bytes()];
        let rpaths = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes(), "a".as_bytes(), "bow".as_bytes()];
        let epaths = ["foo".as_bytes(), "pho".as_bytes()];
        let l = BytesTrieMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = BytesTrieMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = BytesTrieMap::from_iter(epaths.iter().map(|x| (x, ())));
        let p = ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);

        let mut map_cnt = 0;
        let mut collapse_cnt = 0;
        p.into_cata_side_effect(SplitCata::new(
            |_, _p| {
                // println!("Map  {}", String::from_utf8_lossy(_p));
                map_cnt += 1;
            },
            |_, _, _p| {
                // println!("Col *{}", String::from_utf8_lossy(_p));
                collapse_cnt += 1
            },
            |_, _, _| ()));

        // println!("{map_cnt} {collapse_cnt}");
        assert_eq!(map_cnt, 18);
        assert_eq!(collapse_cnt, 12);
    }

    /// Same as `product_zipper_test2` but with tries that contain values along the paths
    #[test]
    fn product_zipper_test3() {
        let lpaths = ["abcdefghijklmnopqrstuvwxyz".as_bytes(), "arrow".as_bytes(), "x".as_bytes(), "arr".as_bytes()];
        let rpaths = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes(), "a".as_bytes(), "bow".as_bytes(), "bo".as_bytes()];
        let epaths = ["foo".as_bytes(), "pho".as_bytes(), "f".as_bytes()];
        let l = BytesTrieMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = BytesTrieMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = BytesTrieMap::from_iter(epaths.iter().map(|x| (x, ())));
        let p = ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);

        let mut map_cnt = 0;
        let mut collapse_cnt = 0;
        p.into_cata_side_effect(SplitCata::new(
            |_, _p| {
                // println!("Map  {}", String::from_utf8_lossy(_p));
                map_cnt += 1;
            },
            |_, _, _p| {
                // println!("Col *{}", String::from_utf8_lossy(_p));
                collapse_cnt += 1
            },
            |_, _, _| ()));

        // println!("{map_cnt} {collapse_cnt}");
        assert_eq!(map_cnt, 18);
        assert_eq!(collapse_cnt, 21);
    }

    /// Narrows in on some tricky behavior surrounding values at factor transitions.  The issue is that the
    /// same path can be represented with more than one regularized form.  In the test below, the path:
    /// `abcdefghijklmnopqrstuvwxyzbo` falls on the transition point (value) in the second factor, signaling
    /// a step to the third factor.
    ///
    /// However, the regularization behavior means that the zipper's `focus_node` will be regularized to point
    /// to the 'w' in "bow".  This doesn't actually represent the 'w', but rather represents "the node that
    /// follows 'o', which just happens to be 'w'".  On ascent, however, the `focus_node` will be the base
    /// of the third factor, e.g. the {'f', 'p'} node.
    ///
    /// These are both valid configurations for the zipper and the user-facing methods should behave the same
    /// regardless of the config.
    ///
    /// NOTE: This logic is the same regardless of node type, but using `all_dense_nodes` will shake out any
    /// problems more aggressively.
    #[test]
    fn product_zipper_test4() {
        let lpaths = ["abcdefghijklmnopqrstuvwxyz".as_bytes(), "arrow".as_bytes(), "x".as_bytes(), "arr".as_bytes()];
        let rpaths = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes(), "a".as_bytes(), "bow".as_bytes(), "bo".as_bytes()];
        let epaths = ["foo".as_bytes(), "pho".as_bytes(), "f".as_bytes()];
        let l = BytesTrieMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = BytesTrieMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = BytesTrieMap::from_iter(epaths.iter().map(|x| (x, ())));
        let mut p = ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);

        p.descend_to("abcdefghijklmnopqrstuvwxyzbo");
        assert_eq!(p.value(), Some(&()));
        assert_eq!(p.child_count(), 2);
        assert_eq!(p.child_mask(), ByteMask::from_iter([b'p', b'f']));

        p.descend_first_byte();
        p.ascend_byte();
        assert_eq!(p.value(), Some(&()));
        assert_eq!(p.child_count(), 2);
        assert_eq!(p.child_mask(), ByteMask::from_iter([b'p', b'f']));
    }

    #[test]
    fn product_zipper_test5() {
        let lpaths = ["abcdefghijklmnopqrstuvwxyz".as_bytes(), "arrow".as_bytes(), "x".as_bytes(), "arr".as_bytes()];
        let rpaths = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes(), "a".as_bytes(), "bow".as_bytes(), "bo".as_bytes()];
        let epaths = ["foo".as_bytes(), "pho".as_bytes(), "f".as_bytes()];
        let l = BytesTrieMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = BytesTrieMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = BytesTrieMap::from_iter(epaths.iter().map(|x| (x, ())));

        {
            let mut p = ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);
            assert!(p.descend_to("abcdefghijklmnopqrstuvwxyzbofo"));
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbofo");
            assert!(p.descend_first_byte());
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbofoo");
        }
        {
            let mut p = ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);
            p.descend_to("abcdefghijklmnopqrstuvwxyzbof");
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbof");
            assert!(p.is_value());
            assert!(p.descend_to("oo"));
            assert!(p.is_value());
        }
        {
            let mut p = ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);
            p.descend_to("abcdefghijklmnopqrstuvwxyzbofo");
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbofo");
            assert!(p.ascend_byte());
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbof");
            assert!(p.ascend_byte());
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbo");
            assert!(p.descend_to_byte(b'p'));
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbop");
            assert!(p.descend_to_byte(b'h'));
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzboph");
            assert!(p.descend_to_byte(b'o'));
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbopho");
            assert!(p.is_value());
            assert!(p.ascend_until());
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbo");
            assert!(p.ascend(2));
            assert_eq!(vec![b'A', b'a', b'b'], crate::utils::ByteMaskIter::new(p.child_mask()).collect::<Vec<_>>());
            assert!(p.descend_to("ABCDEFGHIJKLMNOPQRSTUVWXYZ"));
            assert_eq!(vec![b'f', b'p'], crate::utils::ByteMaskIter::new(p.child_mask()).collect::<Vec<_>>())
        }
    }

    #[test]
    fn product_zipper_test6() {
        let lpaths = ["abcdefghijklmnopqrstuvwxyz".as_bytes(), "arrow".as_bytes(), "x".as_bytes(), "arr".as_bytes()];
        let rpaths = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes(), "a".as_bytes(), "bow".as_bytes(), "bo".as_bytes()];
        let epaths = ["foo".as_bytes(), "pho".as_bytes(), "f".as_bytes()];
        let l = BytesTrieMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = BytesTrieMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = BytesTrieMap::from_iter(epaths.iter().map(|x| (x, ())));

        {
            let mut p = ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);
            assert!(!p.descend_to("ABCDEFGHIJKLMNOPQRSTUVWXYZ"));
            // println!("p {}", std::str::from_utf8(p.path()).unwrap());
            assert!(!p.ascend(27));
        }
    }

    /// Hits an additional bug where an intermediate value might be stepped over by one `descend_to`
    /// but used as a marker to move to the product zipper by the other `descend...` methods 
    #[test]
    fn product_zipper_test7() {
        let apaths = ["arr".as_bytes(), "arrow".as_bytes(), "arrowhead".as_bytes()];
        let bpaths = ["bo".as_bytes(), "bow".as_bytes(), "bowie".as_bytes()];
        let cpaths = ["cl".as_bytes(), "club".as_bytes(), "clubhouse".as_bytes()];
        let a = BytesTrieMap::from_iter(apaths.iter().map(|x| (x, ())));
        let b = BytesTrieMap::from_iter(bpaths.iter().map(|x| (x, ())));
        let c = BytesTrieMap::from_iter(cpaths.iter().map(|x| (x, ())));
        let mut p1 = ProductZipper::new(a.read_zipper(), [b.read_zipper(), c.read_zipper()]);
        let mut p2 = ProductZipper::new(a.read_zipper(), [b.read_zipper(), c.read_zipper()]);

        // Reference
        for _ in 0..14 {
            p1.descend_first_byte();
        }
        assert_eq!(p1.path_exists(), true);
        assert_eq!(p1.path(), b"arrboclubhouse");
        assert_eq!(p1.value(), Some(&()));

        // Validate that I can do the same thing with descend_to()
        p2.descend_to(b"arrboclubhouse");
        assert_eq!(p2.path_exists(), true);
        assert_eq!(p2.path(), b"arrboclubhouse");
        assert_eq!(p2.value(), Some(&()));

        // Validate that I can back up, and re-descend
        {
            p2.ascend(11);
            assert_eq!(p2.path(), b"arr");
            assert_eq!(p2.path_exists(), true);
            assert_eq!(p2.value(), Some(&()));

            p2.descend_to(b"boclub");
            assert_eq!(p2.path(), b"arrboclub");
            assert_eq!(p2.path_exists(), true);
            assert_eq!(p2.value(), Some(&()));
        }

        //Now descend to a non-existent path off of the first factor, and re-ascend to
        // an existing path
        {
            p2.reset();
            // "arowbow" should't exist because the whole subtrie below "arr" should be the
            // second factor
            p2.descend_to(b"arrowbow");
            assert_eq!(p2.path(), b"arrowbow");
            assert_eq!(p2.path_exists(), false);

            // "arowbowclub" should't exist because we started in a trie that doesn't exist
            p2.descend_to(b"club");
            assert_eq!(p2.path(), b"arrowbowclub");
            assert_eq!(p2.path_exists(), false);

            p2.ascend(9);
            assert_eq!(p2.path(), b"arr");
            assert_eq!(p2.path_exists(), true);
            assert_eq!(p2.value(), Some(&()));

            p2.descend_to(b"boclub");
            assert_eq!(p2.path(), b"arrboclub");
            assert_eq!(p2.path_exists(), true);
            assert_eq!(p2.value(), Some(&()));
        }

        //Now descend to a non-existent path off of the second factor, and re-ascend to
        // get back to an existing path
        {
            p2.reset();
            // "arrbowclub" should't exist because the whole subtrie below "arrbo" should be the
            // third factor
            p2.descend_to(b"arrbowclub");
            assert_eq!(p2.path(), b"arrbowclub");
            assert_eq!(p2.path_exists(), false);

            p2.ascend(5);
            assert_eq!(p2.path(), b"arrbo");
            assert_eq!(p2.path_exists(), true);
            assert_eq!(p2.value(), Some(&()));

            p2.descend_to(b"club");
            assert_eq!(p2.path(), b"arrboclub");
            assert_eq!(p2.path_exists(), true);
            assert_eq!(p2.value(), Some(&()));
        }
    }

    #[test]
    fn product_zipper_test8() {
        let lpaths = ["abcdefghijklmnopqrstuvwxyz".as_bytes(), "arr".as_bytes(), "arrow".as_bytes(), "x".as_bytes()];
        let rpaths = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes(), "a".as_bytes(), "bo".as_bytes(), "bow".as_bytes(), "bat".as_bytes(), "bit".as_bytes()];
        let epaths = ["foo".as_bytes(), "pho".as_bytes(), "f".as_bytes()];
        let l = BytesTrieMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = BytesTrieMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = BytesTrieMap::from_iter(epaths.iter().map(|x| (x, ())));

        let new_pz = || ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);

        let mut moving_pz = new_pz();
        let cata_pz = new_pz();
        cata_pz.into_cata_side_effect(|_, _, _, path| {
            // println!("{}", String::from_utf8_lossy(path));
            let overlap = crate::line_list_node::find_prefix_overlap(path, moving_pz.path());
            if overlap < moving_pz.path().len() {
                moving_pz.ascend(moving_pz.path().len() - overlap);
            }
            if moving_pz.path().len() < path.len() {
                assert!(moving_pz.descend_to(&path[moving_pz.path().len()..]));
            }
            assert_eq!(moving_pz.path(), path);

            let mut fresh_pz = new_pz();
            fresh_pz.descend_to(path);

            assert_eq!(moving_pz.path(), fresh_pz.path());
            assert_eq!(moving_pz.path_exists(), fresh_pz.path_exists());
            assert_eq!(moving_pz.value(), fresh_pz.value());
            assert_eq!(moving_pz.child_count(), fresh_pz.child_count());
            assert_eq!(moving_pz.child_mask(), fresh_pz.child_mask());
        })
    }
}
