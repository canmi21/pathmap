#![allow(warnings)]

use std::ptr::null_mut;
use crate::alloc::Allocator;
use crate::utils::ByteMask;
use crate::PathMap;
use crate::trie_node::*;
use crate::zipper::*;
use crate::ring::{AlgebraicStatus, DistributiveLattice, Lattice};
use crate::TrieValue;
use crate::write_zipper::write_zipper_priv::WriteZipperPriv;

struct FullZipper {
    path: Vec<u8>
}

impl Zipper for FullZipper {
    fn path_exists(&self) -> bool { true }
    fn is_val(&self) -> bool { true }
    fn child_count(&self) -> usize { 256 }
    fn child_mask(&self) -> ByteMask { [!0u64, !0u64, !0u64, !0u64].into() }
}

impl ZipperPathBuffer for FullZipper {
    unsafe fn origin_path_assert_len(&self, len: usize) -> &[u8] {
        assert!(len <= self.path.capacity());
        unsafe{ core::slice::from_raw_parts(self.path.as_ptr(), len) }
    }

    fn prepare_buffers(&mut self) {
        self.reserve_buffers(EXPECTED_PATH_LEN, 0)
    }
    fn reserve_buffers(&mut self, path_len: usize, _stack_depth: usize) {
        self.path.reserve(path_len)
    }
}

impl ZipperMoving for FullZipper {
    fn at_root(&self) -> bool { self.path.len() == 0 }
    fn reset(&mut self) { self.path.clear() }
    fn val_count(&self) -> usize { usize::MAX/2 } // usize::MAX is a dangerous default for overflow
    fn descend_to<K: AsRef<[u8]>>(&mut self, k: K) -> bool {
        self.path.extend_from_slice(k.as_ref());
        true
    }
    fn descend_to_byte(&mut self, k: u8) -> bool {
        self.path.push(k);
        true
    }
    fn descend_indexed_byte(&mut self, idx: usize) -> Option<u8> {
        assert!(idx < 256);
        self.path.push(idx as u8);
        Some(idx as u8)
    }
    fn descend_first_byte(&mut self) -> Option<u8> {
        self.path.push(0);
        Some(0)
    }
    fn descend_until(&mut self, mut dst_path: *mut u8) -> usize {
        self.path.push(0); // not sure?
        if dst_path != null_mut() { unsafe { *dst_path = 0 } }
        1
    }
    fn ascend(&mut self, steps: usize) -> usize {
        if steps > self.path.len() {
            let old_depth = self.path.len();
            self.path.clear();
            old_depth
        } else {
            self.path.truncate(self.path.len() - steps);
            steps
        }
    }
    fn ascend_byte(&mut self) -> bool {
        self.path.pop().is_some()
    }
    fn ascend_until(&mut self) -> usize {
        self.ascend(1)
    }
    fn ascend_until_branch(&mut self) -> usize {
        self.ascend(1)
    }
    fn to_next_sibling_byte(&mut self) -> Option<u8> { self.to_sibling(true) }
    fn to_prev_sibling_byte(&mut self) -> Option<u8> { self.to_sibling(false) }
}

impl ZipperPath for FullZipper {
    fn path(&self) -> &[u8] { &self.path[..] }
}

impl FullZipper {
    fn to_sibling(&mut self, next: bool) -> Option<u8> {
        if self.path.is_empty() { return None } // right?
        if next {
            let last = self.path.last_mut().unwrap();
            if *last != 255 { *last = *last + 1; Some(*last) }
            else { None }
        } else {
            let first = self.path.first_mut().unwrap();
            if *first != 0 { *first = *first - 1; Some(*first) }
            else { None }
        }
    }
}

// Doesn't seem as lawful as the above, still maybe useful for testing
struct NullZipper {}

impl<V: TrieValue, A: Allocator> WriteZipperPriv<V, A> for NullZipper {
    fn take_focus(&mut self, prune: bool) -> Option<TrieNodeODRc<V, A>> {
        None
    }
    fn take_root_prefix_path(&mut self) -> Vec<u8> {
        unimplemented!()
    }
    fn alloc(&self) -> A {
        unimplemented!()
    }
}

impl <V: TrieValue, A: Allocator> ZipperWriting<V, A> for NullZipper {
    type ZipperHead<'z> = ZipperHead<'z, 'static, V> where Self: 'z;

    fn get_val_mut(&mut self) -> Option<&mut V> { None }
    fn get_val_or_set_mut(&mut self, default: V) -> &mut V { Box::leak(Box::new(default)) }
    fn get_val_or_set_mut_with<F>(&mut self, func: F) -> &mut V where F: FnOnce() -> V { Box::leak(Box::new(func())) }
    fn set_val(&mut self, _val: V) -> Option<V> { None }
    fn remove_val(&mut self, _prune: bool) -> Option<V> { None }
    fn zipper_head<'z>(&'z mut self) -> Self::ZipperHead<'z> { todo!() }
    fn graft<Z: ZipperSubtries<V, A>>(&mut self, _read_zipper: &Z) {}
    fn graft_map(&mut self, _map: PathMap<V, A>) {}
    fn join_into<Z: ZipperSubtries<V, A>>(&mut self, _read_zipper: &Z) -> AlgebraicStatus where V: Lattice { AlgebraicStatus::Element }
    fn join_map_into(&mut self, _map: PathMap<V, A>) -> AlgebraicStatus where V: Lattice { AlgebraicStatus::Element }
    fn join_into_take<Z: ZipperSubtries<V, A> + ZipperWriting<V, A>>(&mut self, _src_zipper: &mut Z, prune: bool) -> AlgebraicStatus where V: Lattice { AlgebraicStatus::Element }
    fn join_k_path_into(&mut self, _byte_cnt: usize, _prune: bool) -> bool where V: Lattice { false }
    fn insert_prefix<K: AsRef<[u8]>>(&mut self, _prefix: K) -> bool { false }
    fn remove_prefix(&mut self, _n: usize) -> bool { false }
    fn meet_into<Z: ZipperSubtries<V, A>>(&mut self, _read_zipper: &Z, _prune: bool) -> AlgebraicStatus where V: Lattice { AlgebraicStatus::Element }
    fn meet_2<'z, ZA: ZipperSubtries<V, A>, ZB: ZipperSubtries<V, A>>(&mut self, _rz_a: &ZA, _rz_b: &ZB) -> AlgebraicStatus where V: Lattice { AlgebraicStatus::Element }
    fn subtract_into<Z: ZipperSubtries<V, A>>(&mut self, _read_zipper: &Z, _prune: bool) -> AlgebraicStatus where V: DistributiveLattice { AlgebraicStatus::Element }
    fn restrict<Z: ZipperSubtries<V, A>>(&mut self, _read_zipper: &Z) -> AlgebraicStatus { AlgebraicStatus::Element }
    fn restricting<Z: ZipperSubtries<V, A>>(&mut self, _read_zipper: &Z) -> bool { false }
    fn remove_branches(&mut self, prune: bool) -> bool { false }
    fn take_map(&mut self, prune: bool) -> Option<PathMap<V, A>> { None }
    fn remove_unmasked_branches(&mut self, _mask: ByteMask, prune: bool) {}
    fn create_path(&mut self) -> bool { false }
    fn prune_path(&mut self) -> usize { 0 }
    fn prune_ascend(&mut self) -> usize { 0 }
}
