use crate::{
    trie_map::PathMap,
    arena_compact::{ACTMmapZipper, ACTVecZipper, ACTVec},
    utils::ByteMask,
    zipper::{
        PrefixZipper,
        ReadZipperTracked,
        ReadZipperUntracked,
        Zipper,
        ZipperAbsolutePath,
        ZipperConcrete,
        ZipperConcretePriv,
        ZipperForking,
        ZipperIteration,
        ZipperMoving,
        ZipperPathBuffer,
        ZipperReadOnlyConditionalValues,
        ZipperReadOnlyConditionalIteration,
        ZipperReadOnlyValues,
        ZipperValues,
    },
};

/// "Polymorphic" Zipper that allows a single zipper type to represent different zippers.
///
/// Created for the purpose of querying data from multiple sources (`PathMap` & `ArenaCompact`)
///
/// Each zipper method will dispatch between different zipper implementations.
pub enum PolyZipper<'trie> {
    /// Tracked PathMap read zipper
    PathMap(ReadZipperTracked<'trie, 'trie, ()>),
    /// Unracked PathMap read zipper.
    /// The reason this exists is to allow forking the zipper.
    /// Forking a Tracked read zipper return an Untracked read zipper.
    PathMapU(ReadZipperUntracked<'trie, 'trie, ()>),
    /// Memory-mapped ACT.
    /// Prefix is necessary here such that MORK can see ACT under a specific path
    ACTMmapPrefix(PrefixZipper<'trie, ACTMmapZipper<'trie, ()>>),
    /// `Vec<u8>` based ACT
    /// Prefix is necessary here such that MORK can see ACT under a specific path
    ACTVecPrefix(PrefixZipper<'trie, ACTVecZipper<'trie, ()>>),
}

impl<'trie> core::convert::Into<PolyZipper<'trie>> for ReadZipperTracked<'trie, 'trie, ()> {
    fn into(self) -> PolyZipper<'trie> {
        PolyZipper::PathMap(self)
    }
}

type WitnessOf<'trie, T> = <T as ZipperReadOnlyConditionalValues<'trie, ()>>::WitnessT;

pub enum PolyZipperWitness<'trie> {
    PathMap(WitnessOf<'trie, ReadZipperTracked<'trie, 'trie, ()>>),
    PathMapU(WitnessOf<'trie, ReadZipperUntracked<'trie, 'trie, ()>>),
    ACTMmapPrefix(WitnessOf<'trie, ACTMmapZipper<'trie, ()>>),
    ACTVecPrefix(WitnessOf<'trie, ACTVecZipper<'trie, ()>>),
}

impl<'trie> ZipperReadOnlyConditionalValues<'trie, ()> for PolyZipper<'trie> {
    type WitnessT = PolyZipperWitness<'trie>;
    fn witness<'w>(&self) -> Self::WitnessT {
        match self {
            Self::PathMap(pm) => PolyZipperWitness::PathMap(pm.witness()),
            Self::PathMapU(pm) => PolyZipperWitness::PathMapU(pm.witness()),
            Self::ACTMmapPrefix(act) => PolyZipperWitness::ACTMmapPrefix(act.witness()),
            Self::ACTVecPrefix(act) => PolyZipperWitness::ACTVecPrefix(act.witness()),
        }
    }
    fn get_val_with_witness<'w>(&self, witness: &'w Self::WitnessT) -> Option<&'w ()> where 'trie: 'w {
        match (self, witness) {
            (Self::PathMap(pm), PolyZipperWitness::PathMap(w)) => pm.get_val_with_witness(w),
            (Self::PathMapU(pm), PolyZipperWitness::PathMapU(w)) => pm.get_val_with_witness(w),
            (Self::ACTMmapPrefix(act), PolyZipperWitness::ACTMmapPrefix(w)) => act.get_val_with_witness(w),
            (Self::ACTVecPrefix(act), PolyZipperWitness::ACTVecPrefix(w)) => act.get_val_with_witness(w),
            _ => None,
        }
    }
}

impl<'trie> ZipperValues<()> for PolyZipper<'trie> {
    fn val(&self) -> Option<&()> {
        match self {
            Self::PathMap(pm) => pm.val(),
            Self::PathMapU(pm) => pm.val(),
            Self::ACTMmapPrefix(act) => act.val(),
            Self::ACTVecPrefix(act) => act.val(),
        }
    }
}

impl<'trie> ZipperReadOnlyValues<'trie, ()> for PolyZipper<'trie> {
    fn get_val(&self) -> Option<&'trie ()> {
        match self {
            Self::PathMap(pm) => pm.get_val(),
            Self::PathMapU(pm) => pm.get_val(),
            Self::ACTMmapPrefix(act) => act.get_val(),
            Self::ACTVecPrefix(act) => act.get_val(),
        }
    }
}

impl<'trie> ZipperConcretePriv for PolyZipper<'trie> {
    fn shared_node_id(&self) -> Option<u64> {
        match self {
            Self::PathMap(pm) => pm.shared_node_id(),
            Self::PathMapU(pm) => pm.shared_node_id(),
            Self::ACTMmapPrefix(act) => act.shared_node_id(),
            Self::ACTVecPrefix(act) => act.shared_node_id(),
        }
    }
}

impl<'trie> Zipper for PolyZipper<'trie> {
    fn path_exists(&self) -> bool {
        match self {
            Self::PathMap(pm) => pm.path_exists(),
            Self::PathMapU(pm) => pm.path_exists(),
            Self::ACTMmapPrefix(act) => act.path_exists(),
            Self::ACTVecPrefix(act) => act.path_exists(),
        }
    }
    fn is_val(&self) -> bool {
        match self {
            Self::PathMap(pm) => pm.is_val(),
            Self::PathMapU(pm) => pm.is_val(),
            Self::ACTMmapPrefix(act) => act.is_val(),
            Self::ACTVecPrefix(act) => act.is_val(),
        }
    }
    fn child_count(&self) -> usize {
        match self {
            Self::PathMap(pm) => pm.child_count(),
            Self::PathMapU(pm) => pm.child_count(),
            Self::ACTMmapPrefix(act) => act.child_count(),
            Self::ACTVecPrefix(act) => act.child_count(),
        }
    }
    fn child_mask(&self) -> ByteMask {
        match self {
            Self::PathMap(pm) => pm.child_mask(),
            Self::PathMapU(pm) => pm.child_mask(),
            Self::ACTMmapPrefix(act) => act.child_mask(),
            Self::ACTVecPrefix(act) => act.child_mask(),
        }
    }
}

impl<'trie> ZipperConcrete for PolyZipper<'trie> {
    fn is_shared(&self) -> bool {
        match self {
            Self::PathMap(pm) => pm.is_shared(),
            Self::PathMapU(pm) => pm.is_shared(),
            Self::ACTMmapPrefix(act) => act.is_shared(),
            Self::ACTVecPrefix(act) => act.is_shared(),
        }
    }
}

impl<'trie> ZipperMoving for PolyZipper<'trie> {
    fn val_count(&self) -> usize {
        match self {
            Self::PathMap(pm) => pm.val_count(),
            Self::PathMapU(pm) => pm.val_count(),
            Self::ACTMmapPrefix(act) => act.val_count(),
            Self::ACTVecPrefix(act) => act.val_count(),
        }
    }
    fn path(&self) -> &[u8] {
        match self {
            Self::PathMap(pm) => pm.path(),
            Self::PathMapU(pm) => pm.path(),
            Self::ACTMmapPrefix(act) => act.path(),
            Self::ACTVecPrefix(act) => act.path(),
        }
    }
    fn descend_to<K: AsRef<[u8]>>(&mut self, path: K) -> bool {
        match self {
            Self::PathMap(pm) => pm.descend_to(path),
            Self::PathMapU(pm) => pm.descend_to(path),
            Self::ACTMmapPrefix(act) => act.descend_to(path),
            Self::ACTVecPrefix(act) => act.descend_to(path),
        }
    }
    fn ascend(&mut self, n: usize) -> bool {
        match self {
            Self::PathMap(pm) => pm.ascend(n),
            Self::PathMapU(pm) => pm.ascend(n),
            Self::ACTMmapPrefix(act) => act.ascend(n),
            Self::ACTVecPrefix(act) => act.ascend(n),
        }
    }
    fn ascend_until(&mut self) -> bool {
        match self {
            Self::PathMap(pm) => pm.ascend_until(),
            Self::PathMapU(pm) => pm.ascend_until(),
            Self::ACTMmapPrefix(act) => act.ascend_until(),
            Self::ACTVecPrefix(act) => act.ascend_until(),
        }
    }
    fn ascend_until_branch(&mut self) -> bool {
        match self {
            Self::PathMap(pm) => pm.ascend_until_branch(),
            Self::PathMapU(pm) => pm.ascend_until_branch(),
            Self::ACTMmapPrefix(act) => act.ascend_until_branch(),
            Self::ACTVecPrefix(act) => act.ascend_until_branch(),
        }
    }
}

impl<'trie> ZipperIteration for PolyZipper<'trie> {

}

impl ZipperForking<()> for PolyZipper<'_> {
    type ReadZipperT<'t> = PolyZipper<'t> where Self: 't;
    fn fork_read_zipper<'a>(&'a self) -> PolyZipper<'a>
    {
        match self {
            Self::PathMap(pm) => PolyZipper::PathMapU(pm.fork_read_zipper()),
            Self::PathMapU(pm) => PolyZipper::PathMapU(pm.fork_read_zipper()),
            Self::ACTMmapPrefix(act) => PolyZipper::ACTMmapPrefix(act.fork_read_zipper()),
            Self::ACTVecPrefix(act) => PolyZipper::ACTVecPrefix(act.fork_read_zipper()),
        }
    }
}
impl<'trie> ZipperReadOnlyConditionalIteration<'trie, ()> for PolyZipper<'trie> {

}

impl<'trie> ZipperAbsolutePath for PolyZipper<'trie> {
    fn origin_path(&self) -> &[u8] {
        match self {
            Self::PathMap(pm) => pm.origin_path(),
            Self::PathMapU(pm) => pm.origin_path(),
            Self::ACTMmapPrefix(act) => act.origin_path(),
            Self::ACTVecPrefix(act) => act.origin_path(),
        }
    }
    fn root_prefix_path(&self) -> &[u8] {
        match self {
            Self::PathMap(pm) => pm.root_prefix_path(),
            Self::PathMapU(pm) => pm.root_prefix_path(),
            Self::ACTMmapPrefix(act) => act.root_prefix_path(),
            Self::ACTVecPrefix(act) => act.root_prefix_path(),
        }
    }
}

impl<'trie> ZipperPathBuffer for PolyZipper<'trie> {
    unsafe fn origin_path_assert_len(&self, len: usize) -> &[u8] {
        unsafe {
            match self {
                Self::PathMap(pm) => pm.origin_path_assert_len(len),
                Self::PathMapU(pm) => pm.origin_path_assert_len(len),
                Self::ACTMmapPrefix(act) => act.origin_path_assert_len(len),
                Self::ACTVecPrefix(act) => act.origin_path_assert_len(len),
            }
        }
    }
    fn prepare_buffers(&mut self) {
        match self {
            Self::PathMap(pm) => pm.prepare_buffers(),
            Self::PathMapU(pm) => pm.prepare_buffers(),
            Self::ACTMmapPrefix(act) => act.prepare_buffers(),
            Self::ACTVecPrefix(act) => act.prepare_buffers(),
        }
    }
    fn reserve_buffers(&mut self, path_len: usize, stack_depth: usize) {
        match self {
            Self::PathMap(pm) => pm.reserve_buffers(path_len, stack_depth),
            Self::PathMapU(pm) => pm.reserve_buffers(path_len, stack_depth),
            Self::ACTMmapPrefix(act) => act.reserve_buffers(path_len, stack_depth),
            Self::ACTVecPrefix(act) => act.reserve_buffers(path_len, stack_depth),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    crate::zipper::zipper_moving_tests::zipper_moving_tests!(poly_zipper_pm,
        |keys: &[&[u8]]| {
            keys.iter().map(|k| (k, ())).collect::<PathMap<()>>()
        },
        |btm: &mut PathMap<()>, path: &[u8]| -> _ {
            PolyZipper::PathMapU(btm.read_zipper_at_path(path))
        }
    );

    crate::zipper::zipper_iteration_tests::zipper_iteration_tests!(poly_zipper_pm,
        |keys: &[&[u8]]| {
            keys.iter().map(|k| (k, ())).collect::<PathMap<()>>()
        },
        |btm: &mut PathMap<()>, path: &[u8]| -> _ {
            PolyZipper::PathMapU(btm.read_zipper_at_path(path))
        }
    );

    crate::zipper::zipper_moving_tests::zipper_moving_tests!(poly_zipper_act,
        |keys: &[&[u8]]| {
            let btm = keys.iter().map(|k| (k, ())).collect::<PathMap<()>>();
            ACTVec::from_zipper(btm.read_zipper(), |()| 0)
        },
        |act: &mut ACTVec, path: &[u8]| -> _ {
            PolyZipper::ACTVecPrefix(PrefixZipper::new(&[], act.read_zipper_at_path(path)))
        }
    );

    crate::zipper::zipper_iteration_tests::zipper_iteration_tests!(poly_zipper_act,
        |keys: &[&[u8]]| {
            let btm = keys.iter().map(|k| (k, ())).collect::<PathMap<()>>();
            ACTVec::from_zipper(btm.read_zipper(), |()| 0)
        },
        |act: &mut ACTVec, path: &[u8]| -> _ {
            PolyZipper::ACTVecPrefix(PrefixZipper::new(&[], act.read_zipper_at_path(path)))
        }
    );
}
