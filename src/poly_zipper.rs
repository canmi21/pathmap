
#[cfg(test)]
mod tests {
    use crate as pathmap;
    use crate::PathMap;
    use crate::zipper::*;

    #[cfg(feature = "arena_compact")]
    use crate::arena_compact::{ACTMmapZipper, ACTVecZipper, ACTVec};

    #[cfg(not(feature = "arena_compact"))]
    #[derive(PolyZipper)]
    pub enum TestPolyZipper<'trie, V: Clone + Send + Sync + Unpin = ()> {
        /// Tracked PathMap read zipper
        PathMap(ReadZipperTracked<'trie, 'trie, V>),
        /// Unracked PathMap read zipper.
        /// The reason this exists is to allow forking the zipper.
        /// Forking a Tracked read zipper return an Untracked read zipper.
        PathMapU(ReadZipperUntracked<'trie, 'trie, V>),
    }

    #[cfg(feature = "arena_compact")]
    #[derive(PolyZipper)]
    pub enum TestPolyZipper<'trie, V: Clone + Send + Sync + Unpin = ()> {
        /// Tracked PathMap read zipper
        PathMap(ReadZipperTracked<'trie, 'trie, V>),
        /// Unracked PathMap read zipper.
        /// The reason this exists is to allow forking the zipper.
        /// Forking a Tracked read zipper return an Untracked read zipper.
        PathMapU(ReadZipperUntracked<'trie, 'trie, V>),
        /// Memory-mapped ACT.
        /// Prefix is necessary here such that MORK can see ACT under a specific path
        ACTMmapPrefix(PrefixZipper<'trie, ACTMmapZipper<'trie, V>>),
        /// `Vec<u8>` based ACT
        /// Prefix is necessary here such that MORK can see ACT under a specific path
        ACTVecPrefix(PrefixZipper<'trie, ACTVecZipper<'trie, V>>),
    }


    crate::zipper::zipper_moving_tests::zipper_moving_tests!(poly_zipper_pm,
        |keys: &[&[u8]]| {
            keys.iter().map(|k| (k, ())).collect::<PathMap<()>>()
        },
        |btm: &mut PathMap<()>, path: &[u8]| -> _ {
            TestPolyZipper::PathMapU(btm.read_zipper_at_path(path))
        }
    );

    crate::zipper::zipper_iteration_tests::zipper_iteration_tests!(poly_zipper_pm,
        |keys: &[&[u8]]| {
            keys.iter().map(|k| (k, ())).collect::<PathMap<()>>()
        },
        |btm: &mut PathMap<()>, path: &[u8]| -> _ {
            TestPolyZipper::PathMapU(btm.read_zipper_at_path(path))
        }
    );

    #[cfg(feature = "arena_compact")]
    crate::zipper::zipper_moving_tests::zipper_moving_tests!(poly_zipper_act,
        |keys: &[&[u8]]| {
            let btm = keys.iter().map(|k| (k, ())).collect::<PathMap<()>>();
            ACTVec::from_zipper(btm.read_zipper(), |()| 0)
        },
        |act: &mut ACTVec, path: &[u8]| -> _ {
            TestPolyZipper::ACTVecPrefix(PrefixZipper::new(&[], act.read_zipper_at_path(path)))
        }
    );

    #[cfg(feature = "arena_compact")]
    crate::zipper::zipper_iteration_tests::zipper_iteration_tests!(poly_zipper_act,
        |keys: &[&[u8]]| {
            let btm = keys.iter().map(|k| (k, ())).collect::<PathMap<()>>();
            ACTVec::from_zipper(btm.read_zipper(), |()| 0)
        },
        |act: &mut ACTVec, path: &[u8]| -> _ {
            TestPolyZipper::ACTVecPrefix(PrefixZipper::new(&[], act.read_zipper_at_path(path)))
        }
    );
}
