
use core::marker::PhantomData;
use core::fmt::{Debug, Formatter};
use std::collections::HashMap;

use crate::trie_node::*;
use crate::ring::*;
use crate::dense_byte_node::CellByteNode;

pub struct EmptyNode<V> {
    phantom: PhantomData<V>
}

impl<V> Clone for EmptyNode<V> {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<V> Default for EmptyNode<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> Debug for EmptyNode<V> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "EmptyNode")
    }
}

impl<V> EmptyNode<V> {
    pub fn new() -> Self {
        Self {
            phantom: <_>::default()
        }
    }
}

impl<V: Clone + Send + Sync> TrieNode<V> for EmptyNode<V> {
    fn node_contains_partial_key(&self, _key: &[u8]) -> bool {
        false
    }
    fn node_get_child(&self, _key: &[u8]) -> Option<(usize, &dyn TrieNode<V>)> {
        None
    }
    //GOAT, Deprecated node_get_child_and_val_mut
    // fn node_get_child_and_val_mut(&mut self, _key: &[u8]) -> Option<(usize, &mut TrieNodeODRc<V>, &mut Option<V>)> {
    //     None
    // }
    fn node_get_child_mut(&mut self, _key: &[u8]) -> Option<(usize, &mut TrieNodeODRc<V>)> {
        None
    }
    fn node_replace_child(&mut self, _key: &[u8], _new_node: TrieNodeODRc<V>) -> &mut dyn TrieNode<V> {
        unreachable!() //Should not be called unless it's known that the node being replaced exists
    }
    fn node_contains_val(&self, _key: &[u8]) -> bool {
        false
    }
    fn node_get_val(&self, _key: &[u8]) -> Option<&V> {
        None
    }
    fn node_remove_val(&mut self, _key: &[u8]) -> Option<V> {
        None
    }
    fn node_get_val_mut(&mut self, _key: &[u8]) -> Option<&mut V> {
        None
    }
    fn node_set_val(&mut self, key: &[u8], val: V) -> Result<(Option<V>, bool), TrieNodeODRc<V>> {
        #[allow(unused_mut)]
        let mut replacement_node;
        #[cfg(all(not(feature = "all_dense_nodes"), not(feature = "bridge_nodes")))]
        {
            replacement_node = crate::line_list_node::LineListNode::new();
            replacement_node.node_set_val(key, val).unwrap_or_else(|_| panic!());
        }
        #[cfg(all(feature = "bridge_nodes", not(feature = "all_dense_nodes")))]
        {
            replacement_node = crate::bridge_node::BridgeNode::new(key, false, val.into());
        }
        #[cfg(feature = "all_dense_nodes")]
        {
            replacement_node = crate::dense_byte_node::DenseByteNode::new();
            replacement_node.node_set_val(key, val).unwrap_or_else(|_| panic!());
        }
        Err(TrieNodeODRc::new(replacement_node))
    }
    fn node_set_branch(&mut self, key: &[u8], new_node: TrieNodeODRc<V>) -> Result<bool, TrieNodeODRc<V>> {
        #[allow(unused_mut)]
        let mut replacement_node;
        #[cfg(all(not(feature = "all_dense_nodes"), not(feature = "bridge_nodes")))]
        {
            replacement_node = crate::line_list_node::LineListNode::new();
            replacement_node.node_set_branch(key, new_node).unwrap_or_else(|_| panic!());
        }
        #[cfg(all(feature = "bridge_nodes", not(feature = "all_dense_nodes")))]
        {
            replacement_node = crate::bridge_node::BridgeNode::new(key, true, new_node.into());
        }
        #[cfg(feature = "all_dense_nodes")]
        {
            replacement_node = crate::dense_byte_node::DenseByteNode::new();
            replacement_node.node_set_branch(key, new_node).unwrap_or_else(|_| panic!());
        }
        Err(TrieNodeODRc::new(replacement_node))
    }
    fn node_remove_all_branches(&mut self, _key: &[u8]) -> bool {
        false
    }
    fn node_remove_unmasked_branches(&mut self, _key: &[u8], _mask: [u64; 4]) {}
    fn node_is_empty(&self) -> bool { true }
    fn new_iter_token(&self) -> u128 {
        0
    }
    fn iter_token_for_path(&self, _key: &[u8]) -> (u128, &[u8]) {
        (0, &[])
    }
    fn next_items(&self, _token: u128) -> (u128, &[u8], Option<&TrieNodeODRc<V>>, Option<&V>) {
        (NODE_ITER_FINISHED, &[], None, None)
    }
    fn node_val_count(&self, _cache: &mut HashMap<*const dyn TrieNode<V>, usize>) -> usize {
        0
    }
    #[cfg(feature = "counters")]
    fn item_count(&self) -> usize {
        0
    }
    fn node_first_val_depth_along_key(&self, _key: &[u8]) -> Option<usize> {
        None
    }
    fn nth_child_from_key(&self, _key: &[u8], _n: usize) -> (Option<u8>, Option<&dyn TrieNode<V>>) {
        (None, None)
    }
    fn first_child_from_key(&self, _key: &[u8]) -> (Option<&[u8]>, Option<&dyn TrieNode<V>>) {
        (None, None)
    }
    fn count_branches(&self, _key: &[u8]) -> usize {
        0
    }
    fn node_branches_mask(&self, _key: &[u8]) -> [u64; 4] {
        [0; 4]
    }
    fn is_leaf(&self, _key: &[u8]) -> bool {
        true
    }

    fn prior_branch_key(&self, _key: &[u8]) -> &[u8] {
        &[]
    }

    fn get_sibling_of_child(&self, _key: &[u8], _next: bool) -> (Option<u8>, Option<&dyn TrieNode<V>>) {
        (None, None)
    }
    fn get_node_at_key(&self, _key: &[u8]) -> AbstractNodeRef<V> {
        AbstractNodeRef::None
    }
    fn take_node_at_key(&mut self, _key: &[u8]) -> Option<TrieNodeODRc<V>> {
        None
    }
    fn pjoin_dyn(&self, other: &dyn TrieNode<V>) -> AlgebraicResult<TrieNodeODRc<V>> where V: Lattice {
        if other.node_is_empty() {
            AlgebraicResult::None
        } else {
            AlgebraicResult::Identity(COUNTER_IDENT)
        }
    }
    fn join_into_dyn(&mut self, other: TrieNodeODRc<V>) -> (AlgebraicStatus, Result<(), TrieNodeODRc<V>>) where V: Lattice {
        if other.borrow().node_is_empty() {
            (AlgebraicStatus::None, Ok(()))
        } else {
            (AlgebraicStatus::Element, Err(other.clone()))
        }
    }
    fn drop_head_dyn(&mut self, _byte_cnt: usize) -> Option<TrieNodeODRc<V>> where V: Lattice {
        None
    }
    fn pmeet_dyn(&self, _other: &dyn TrieNode<V>) -> AlgebraicResult<TrieNodeODRc<V>> where V: Lattice {
        AlgebraicResult::None
    }
    fn psubtract_dyn(&self, _other: &dyn TrieNode<V>) -> AlgebraicResult<TrieNodeODRc<V>> where V: DistributiveLattice {
        AlgebraicResult::None
    }
    fn prestrict_dyn(&self, _other: &dyn TrieNode<V>) -> AlgebraicResult<TrieNodeODRc<V>> {
        AlgebraicResult::None
    }
    fn clone_self(&self) -> TrieNodeODRc<V> {
        TrieNodeODRc::new(self.clone())
    }
}

impl<V: Clone + Send + Sync> TrieNodeDowncast<V> for EmptyNode<V> {
    fn as_tagged(&self) -> TaggedNodeRef<V> {
        TaggedNodeRef::EmptyNode(self)
    }
    fn as_tagged_mut(&mut self) -> TaggedNodeRefMut<V> {
        TaggedNodeRefMut::Unsupported
    }
    fn convert_to_cell_node(&mut self) -> TrieNodeODRc<V> {
        TrieNodeODRc::new(CellByteNode::new())
    }
}
