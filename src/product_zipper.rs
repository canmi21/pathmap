
use crate::alloc::{Allocator, GlobalAlloc};
use crate::utils::ByteMask;
use crate::trie_node::*;
use crate::zipper::*;
use zipper_priv::*;

/// A [Zipper] type that moves through a Cartesian product trie created by extending each path in a primary
/// trie with the root of the next secondardary trie, doing it recursively for all provided tries
pub struct ProductZipper<'factor_z, 'trie, V: Clone + Send + Sync, A: Allocator = GlobalAlloc> {
    z: read_zipper_core::ReadZipperCore<'trie, 'static, V, A>,
    /// All of the seconday factors beyond the primary factor
    secondaries: Vec<TrieRef<'trie, V, A>>,
    /// The indices in the zipper's path that correspond to the start-point of each secondary factor,
    /// which is conceptually the same as the end-point of each indexed factor
    factor_paths: Vec<usize>,
    /// We need to hang onto the zippers for the life of this object, so their trackers stay alive
    source_zippers: Vec<Box<dyn zipper_priv::ZipperReadOnlyPriv<'trie, V, A> + 'factor_z>>
}

impl<'factor_z, 'trie, V: Clone + Send + Sync + Unpin + 'trie, A: Allocator + 'trie> ProductZipper<'factor_z, 'trie, V, A> {
    /// Creates a new `ProductZipper` from the provided zippers
    ///
    /// WARNING: passing `other_zippers` that are not at node roots may lead to a panic.  This is
    /// an implementation issue, but would be very difficult to fix and may not be worth fixing.
    pub fn new<PrimaryZ, OtherZ, ZipperList>(mut primary_z: PrimaryZ, other_zippers: ZipperList) -> Self
        where
        PrimaryZ: ZipperMoving + ZipperReadOnlySubtries<'trie, V, A> + 'factor_z,
        OtherZ: ZipperReadOnlySubtries<'trie, V, A> + 'factor_z,
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
        source_zippers.push(Box::new(primary_z) as Box<dyn zipper_priv::ZipperReadOnlyPriv<V, A>>);

        //Get TrieRefs for the remaining zippers
        for other_z in other_z_iter {
            let trie_ref: TrieRef<'trie, V, A> = other_z.trie_ref_at_path("").into();
            secondaries.push(trie_ref);
            source_zippers.push(Box::new(other_z));
        }

        Self{z: core_z, factor_paths: Vec::with_capacity(secondaries.len()), secondaries, source_zippers}
    }
    /// Creates a new `ProductZipper` from a single zipper, with the expectation that more zippers
    /// will be added using [new_factors](Self::new_factors)
    pub fn new_with_primary<PrimaryZ>(mut primary_z: PrimaryZ) -> Self
        where PrimaryZ: ZipperMoving + ZipperReadOnlySubtries<'trie, V, A> + 'factor_z,
    {
        let mut source_zippers = Vec::new();

        //Get the core out of the primary zipper
        //This unwrap won't fail because all the types that implement `ZipperMoving` have cores
        let core_z = primary_z.take_core().unwrap();
        source_zippers.push(Box::new(primary_z) as Box<dyn zipper_priv::ZipperReadOnlyPriv<V, A>>);

        Self{z: core_z, factor_paths: Vec::new(), secondaries: vec![], source_zippers}
    }
    /// Appends additional factors to a `ProductZipper`.  This is useful when dealing with
    /// factor zippers of different types
    ///
    /// WARNING: the same warning as above applies about passing other zippers that aren't at node roots
    pub fn new_factors<OtherZ, ZipperList>(&mut self, other_zippers: ZipperList)
        where
        OtherZ: ZipperReadOnlySubtries<'trie, V, A> + 'factor_z,
        ZipperList: IntoIterator<Item=OtherZ>,
    {
        let other_z_iter = other_zippers.into_iter();
        for other_z in other_z_iter {
            let trie_ref: TrieRef<'trie, V, A> = other_z.trie_ref_at_path("").into();
            self.secondaries.push(trie_ref);
            self.source_zippers.push(Box::new(other_z));
        }
    }
    /// Returns the number of factors composing the `ProductZipper`
    ///
    /// The minimum returned value will be 1 because the primary factor is counted.
    pub fn factor_count(&self) -> usize {
        self.secondaries.len() + 1
    }
    /// Returns the index of the factor containing the `ProductZipper` focus
    ///
    /// Returns `0` if the focus is in the primary factor.  The returned value will always be
    /// `zipper.focus_factor() < zipper.factor_count()`.
    pub fn focus_factor(&self) -> usize {
        match self.factor_paths.last() {
            Some(factor_path_len) => {
                let factor_idx = self.factor_paths.len();
                if *factor_path_len < self.path().len() {
                    factor_idx
                } else {
                    factor_idx - 1
                }
            },
            None => 0
        }
    }
    /// Returns a slice of the path indices that represent the end-points of the portion of the path from each
    /// factor
    ///
    /// The returned slice will have a length of [`focus_factor`](Self::focus_factor), so the factor
    /// containing the current focus has will not be included.
    ///
    /// Indices will be offsets into the buffer returned by [path](ZipperMoving::path).  To get an offset into
    /// [origin_path](ZipperAbsolutePath::origin_path), add the length of the prefix path from
    /// [root_prefix_path](ZipperAbsolutePath::root_prefix_path).
    pub fn path_indices(&self) -> &[usize] {
        &self.factor_paths
    }
    /// Reserves a path buffer of at least `len` bytes.  Will never shrink the path buffer
    /// NOTE, this doesn't offer any value over the standard `reserve_buffers` method which is now implemented
    /// on many zipper types
    #[deprecated]
    pub fn reserve_path_buffer(&mut self, reserve_len: usize) {
        const AVG_BYTES_PER_NODE: usize = 8;
        self.reserve_buffers(reserve_len, (reserve_len / AVG_BYTES_PER_NODE) + 1);
    }
    #[inline]
    fn has_next_factor(&mut self) -> bool {
        self.factor_paths.len() < self.secondaries.len()
    }
    fn enroll_next_factor(&mut self, unchecked_descent: usize) {
        //If there is a `_secondary_root_val`, it lands at the same path as the value where the
        // paths are joined.  And the value from the earlier zipper takes precedence
        let (secondary_root, partial_path, _secondary_root_val) = self.secondaries[self.factor_paths.len()].borrow_raw_parts();

        //SAFETY: We won't drop the `secondaries` vec until we're done with the stack of node references
        let secondary_root: TaggedNodeRef<'trie, V, A> = unsafe{ core::mem::transmute(secondary_root) };

        //TODO! Dealing with hidden root path in a secondary factor is very nasty.  I'm going to punt
        // on handling this until we move this feature out of the experimental stage.
        //See "WARNING" in ProductZipper creation methods
        assert_eq!(partial_path.len(), 0);

        self.z.deregularize();
        self.z.push_node(secondary_root, unchecked_descent);
        self.factor_paths.push(self.path().len() - unchecked_descent);
    }
    /// Internal method to descend across the boundary between two factor zippers if the focus is on a value
    ///
    /// The ProductZipper's internal representation can be a bit tricky.  See the documentation on
    /// `product_zipper_test4` for more discussion.
    #[inline]
    fn ensure_descend_next_factor(&mut self) {
        if self.factor_paths.len() < self.secondaries.len() && self.z.child_count() == 0 {

            //We don't want to push the same factor on the stack twice
            if let Some(factor_path_len) = self.factor_paths.last() {
                if *factor_path_len == self.path().len() {
                    return
                }
            }

            self.enroll_next_factor(0);
        }
    }
    /// Internal method to make sure `self.factor_paths` is correct after an ascend method
    #[inline]
    fn fix_after_ascend(&mut self) {
        while let Some(factor_path_start) = self.factor_paths.last() {
            if self.z.path().len() < *factor_path_start {
                self.factor_paths.pop();
            } else {
                break
            }
        }
    }
}

impl<'trie, V: Clone + Send + Sync + Unpin + 'trie, A: Allocator + 'trie> ZipperMoving for ProductZipper<'_, 'trie, V, A> {
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
        let mut descended = 0;

        while descended < k.len() {
            let this_step = self.z.descend_to_existing(&k[descended..]);
            if this_step == 0 {
                break
            }
            descended += this_step;

            if self.has_next_factor() {
                if self.z.child_count() == 0 && self.factor_paths.last().map(|l| *l).unwrap_or(0) < self.path().len() {
                    self.enroll_next_factor(0);
                }
            } else {
                break
            }
        }
        descended
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
    #[inline]
    fn descend_to_byte(&mut self, k: u8) -> bool {
        if !self.z.descend_to_byte(k) {
            if self.has_next_factor() {
                if self.z.path_parent_byte_is_path_end() {
                    debug_assert!(self.factor_paths.last().map(|l| *l).unwrap_or(0) < self.path().len());
                    self.enroll_next_factor(1);
                    self.z.regularize();
                    true
                } else { false }
            } else { false }
        } else { true }
    }
    fn descend_indexed_byte(&mut self, child_idx: usize) -> bool {
        let result = self.z.descend_indexed_byte(child_idx);
        self.ensure_descend_next_factor();
        result
    }
    fn descend_first_byte(&mut self) -> bool {
        let result = self.z.descend_first_byte();
        self.ensure_descend_next_factor();
        result
    }
    fn descend_until(&mut self) -> bool {
        let result = self.z.descend_until();
        self.ensure_descend_next_factor();
        result
    }
    fn to_next_sibling_byte(&mut self) -> bool {
        if self.factor_paths.last().cloned().unwrap_or(0) == self.path().len() {
            self.factor_paths.pop();
        }
        let moved = self.z.to_next_sibling_byte();
        self.ensure_descend_next_factor();
        moved
    }
    fn to_prev_sibling_byte(&mut self) -> bool {
        if self.factor_paths.last().cloned().unwrap_or(0) == self.path().len() {
            self.factor_paths.pop();
        }
        let moved = self.z.to_prev_sibling_byte();
        self.ensure_descend_next_factor();
        moved
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

impl<'trie, V: Clone + Send + Sync + Unpin + 'trie, A: Allocator + 'trie> ZipperIteration for ProductZipper<'_, 'trie, V, A> { } //Use the default impl for all methods

impl<'trie, V: Clone + Send + Sync + Unpin + 'trie, A: Allocator + 'trie> ZipperValues<V> for ProductZipper<'_, 'trie, V, A> {
    fn val(&self) -> Option<&V> {
        self.z.get_val()
    }
}

impl<'factor_z, 'trie, V: Clone + Send + Sync + Unpin + 'trie, A: Allocator + 'trie> ZipperReadOnlyValues<'trie, V> for ProductZipper<'factor_z, 'trie, V, A> {
    fn get_val(&self) -> Option<&'trie V> { self.z.get_val() }
}

impl<'factor_z, 'trie, V: Clone + Send + Sync + Unpin + 'trie, A: Allocator + 'trie> ZipperReadOnlyConditionalValues<'trie, V> for ProductZipper<'factor_z, 'trie, V, A> {
    type WitnessT = ();
    fn witness<'w>(&self) -> Self::WitnessT { () }
    fn get_val_with_witness<'w>(&self, _witness: &'w Self::WitnessT) -> Option<&'w V> where 'trie: 'w { self.get_val() }
}

impl<'trie, V: Clone + Send + Sync + Unpin + 'trie, A: Allocator + 'trie> Zipper for ProductZipper<'_, 'trie, V, A> {
    fn path_exists(&self) -> bool {
        self.z.path_exists()
    }
    fn is_val(&self) -> bool {
        self.z.is_val()
    }
    fn child_count(&self) -> usize {
        self.z.child_count()
    }
    fn child_mask(&self) -> ByteMask {
        self.z.child_mask()
    }
}

impl<V: Clone + Send + Sync + Unpin, A: Allocator> ZipperConcrete for ProductZipper<'_, '_, V, A> {
    fn is_shared(&self) -> bool { self.z.is_shared() }
}

impl<V: Clone + Send + Sync + Unpin, A: Allocator> ZipperConcretePriv for ProductZipper<'_, '_, V, A> {
    fn shared_node_id(&self) -> Option<u64> { self.z.shared_node_id() }
}

impl<V: Clone + Send + Sync + Unpin, A: Allocator> zipper_priv::ZipperPriv for ProductZipper<'_, '_, V, A> {
    type V = V;
    type A = A;
    fn get_focus(&self) -> AbstractNodeRef<'_, Self::V, Self::A> { self.z.get_focus() }
    fn try_borrow_focus(&self) -> Option<&TrieNodeODRc<Self::V, Self::A>> { self.z.try_borrow_focus() }
}

impl<'trie, V: Clone + Send + Sync + Unpin + 'trie, A: Allocator + 'trie> ZipperPathBuffer for ProductZipper<'_, 'trie, V, A> {
    unsafe fn origin_path_assert_len(&self, len: usize) -> &[u8] { unsafe{ self.z.origin_path_assert_len(len) } }
    fn prepare_buffers(&mut self) { self.z.prepare_buffers() }
    fn reserve_buffers(&mut self, path_len: usize, stack_depth: usize) { self.z.reserve_buffers(path_len, stack_depth) }
}

impl<'trie, V: Clone + Send + Sync + Unpin + 'trie, A: Allocator + 'trie> ZipperAbsolutePath for ProductZipper<'_, 'trie, V, A> {
    fn origin_path(&self) -> &[u8] { self.z.origin_path() }
    fn root_prefix_path(&self) -> &[u8] { self.z.root_prefix_path() }
}

/// A [Zipper] type that moves through a Cartesian product trie created by extending each path in a primary
/// trie with the root of the next secondardary trie, doing it recursively for all provided tries
///
/// Compared to [ProductZipper], this is a generic virtual zipper that works without
/// inspecting the inner workings of primary and secondary zippers.  `ProductZipperG` is more general,
/// while `ProductZipper` is faster in situations where it can be used.
///
/// NOTE: In the future, this generic type will be renamed to `ProductZipper`, and the existing
/// [ProductZipper] will be renamed something else or removed entirely.
pub struct ProductZipperG<'trie, PrimaryZ, SecondaryZ, V>
    where
        V: Clone + Send + Sync,
{
    factor_paths: Vec<usize>,
    primary: PrimaryZ,
    secondary: Vec<SecondaryZ>,
    _marker: core::marker::PhantomData<&'trie V>
}

impl<'trie, PrimaryZ, SecondaryZ, V> ProductZipperG<'trie, PrimaryZ, SecondaryZ, V>
    where
        V: Clone + Send + Sync,
        PrimaryZ: ZipperMoving,
        SecondaryZ: ZipperMoving,
{
    /// Creates a new `ProductZipper` from the provided zippers
    pub fn new<ZipperList>(primary: PrimaryZ, other_zippers: ZipperList) -> Self
        where
            ZipperList: IntoIterator<Item=SecondaryZ>,
            PrimaryZ: ZipperValues<V>,
            SecondaryZ: ZipperValues<V>,
    {
        Self {
            factor_paths: Vec::new(),
            primary,
            secondary: other_zippers.into_iter().collect(),
            _marker: core::marker::PhantomData,
        }
    }

    /// Returns the index of the factor containing the `ProductZipper` focus
    ///
    /// Returns `0` if the focus is in the primary factor.  The returned value will always be
    /// `zipper.focus_factor() < zipper.factor_count()`.
    pub fn focus_factor(&self) -> usize {
        self.factor_idx(true).map_or(0, |x| x + 1)
    }

    /// Actual focus factor calculation.
    /// Returns a valid index into `self.factor_paths`, truncating to parents if requested.
    fn factor_idx(&self, truncate_up: bool) -> Option<usize> {
        let len = self.path().len();
        let mut factor = self.factor_paths.len().checked_sub(1)?;
        while truncate_up && self.factor_paths[factor] == len {
            factor = factor.checked_sub(1)?;
        }
        Some(factor)
    }

    /// Returns the number of factors composing the `ProductZipper`
    ///
    /// The minimum returned value will be 1 because the primary factor is counted.
    pub fn factor_count(&self) -> usize {
        self.secondary.len() + 1
    }

    /// Returns a slice of the path indices that represent the end-points of the portion of the path from each
    /// factor
    ///
    /// The returned slice will have a length of [`focus_factor`](Self::focus_factor), so the factor
    /// containing the current focus has will not be included.
    ///
    /// Indices will be offsets into the buffer returned by [path](ZipperMoving::path).  To get an offset into
    /// [origin_path](ZipperAbsolutePath::origin_path), add the length of the prefix path from
    /// [root_prefix_path](ZipperAbsolutePath::root_prefix_path).
    pub fn path_indices(&self) -> &[usize] {
        &self.factor_paths
    }

    /// Returns `true` if the last active factor zipper is positioned at the end of a valid path
    /// (i.e. a stitch point to the next zipper)
    fn is_path_end(&self) -> bool {
        if let Some(idx) = self.factor_idx(false) {
            self.secondary[idx].child_count() == 0 && self.secondary[idx].path_exists()
        } else {
            self.primary.child_count() == 0 && self.primary.path_exists()
        }
    }

    /// Remove top factors if they are at root
    fn exit_factors(&mut self) -> bool {
        let len = self.path().len();
        let mut exited = false;
        while self.factor_paths.last() == Some(&len) {
            self.factor_paths.pop();
            exited = true;
        }
        exited
    }

    /// Enter factors at current location if we're on the end of the factor's path
    fn enter_factors(&mut self) -> bool {
        let len = self.path().len();
        // enter the next factor if we can
        let mut entered = false;
        if self.factor_paths.len() < self.secondary.len() && self.is_path_end() {
            self.factor_paths.push(len);
            entered = true;
        }
        entered
    }

    /// A combination between `ascend_until` and `ascend_until_branch`.
    /// if `allow_stop_on_val` is `true`, behaves as `ascend_until`
    fn ascend_cond(&mut self, allow_stop_on_val: bool) -> bool {
        let mut plen = self.path().len();
        loop {
            while self.factor_paths.last() == Some(&plen) {
                self.factor_paths.pop();
            }
            if let Some(idx) = self.factor_idx(false) {
                let zipper = &mut self.secondary[idx];
                let before = zipper.path().len();
                let rv = if allow_stop_on_val {
                    zipper.ascend_until()
                } else {
                    zipper.ascend_until_branch()
                };
                let delta = before - zipper.path().len();
                plen -= delta;
                self.primary.ascend(delta);
                if rv {
                    return true;
                }
            } else {
                return if allow_stop_on_val {
                    self.primary.ascend_until()
                } else {
                    self.primary.ascend_until_branch()
                };
            }
        }
    }

    /// a combination between `to_next_sibling` and `to_prev_sibling`
    fn to_sibling_byte(&mut self, next: bool) -> bool {
        let Some(&byte) = self.path().last() else {
            return false;
        };
        assert!(self.ascend(1), "must ascend");
        let child_mask = self.child_mask();
        let Some(sibling_byte) = (if next {
            child_mask.next_bit(byte)
        } else {
            child_mask.prev_bit(byte)
        }) else {
            self.descend_to_byte(byte);
            return false;
        };
        self.descend_to_byte(sibling_byte)
    }
}

impl<'trie, PrimaryZ, SecondaryZ, V> ZipperAbsolutePath
    for ProductZipperG<'trie, PrimaryZ, SecondaryZ, V>
    where
        V: Clone + Send + Sync,
        PrimaryZ: ZipperAbsolutePath,
        SecondaryZ: ZipperMoving,
{
    fn origin_path(&self) -> &[u8] { self.primary.origin_path() }
    fn root_prefix_path(&self) -> &[u8] { self.primary.root_prefix_path() }
}

impl<'trie, PrimaryZ, SecondaryZ, V> ZipperConcretePriv
    for ProductZipperG<'trie, PrimaryZ, SecondaryZ, V>
    where
        V: Clone + Send + Sync,
        PrimaryZ: ZipperMoving + ZipperConcretePriv,
        SecondaryZ: ZipperMoving + ZipperConcretePriv,
{
    fn shared_node_id(&self) -> Option<u64> {
        if let Some(idx) = self.factor_idx(true) {
            self.secondary[idx].shared_node_id()
        } else {
            self.primary.shared_node_id()
        }
    }
}

impl<'trie, PrimaryZ, SecondaryZ, V> ZipperConcrete
    for ProductZipperG<'trie, PrimaryZ, SecondaryZ, V>
    where
        V: Clone + Send + Sync,
        PrimaryZ: ZipperMoving + ZipperConcrete,
        SecondaryZ: ZipperMoving + ZipperConcrete,
{
    fn is_shared(&self) -> bool {
        if let Some(idx) = self.factor_idx(true) {
            self.secondary[idx].is_shared()
        } else {
            self.primary.is_shared()
        }
    }
}

impl<'trie, PrimaryZ, SecondaryZ, V> ZipperPathBuffer
    for ProductZipperG<'trie, PrimaryZ, SecondaryZ, V>
    where
        V: Clone + Send + Sync,
        PrimaryZ: ZipperMoving + ZipperPathBuffer,
        SecondaryZ: ZipperMoving + ZipperPathBuffer,
{
    unsafe fn origin_path_assert_len(&self, len: usize) -> &[u8] { unsafe{ self.primary.origin_path_assert_len(len) } }
    fn prepare_buffers(&mut self) { self.primary.prepare_buffers() }
    fn reserve_buffers(&mut self, path_len: usize, stack_depth: usize) { self.primary.reserve_buffers(path_len, stack_depth) }
}

impl<'trie, PrimaryZ, SecondaryZ, V> ZipperValues<V>
    for ProductZipperG<'trie, PrimaryZ, SecondaryZ, V>
    where
        V: Clone + Send + Sync,
        PrimaryZ: ZipperMoving + ZipperValues<V>,
        SecondaryZ: ZipperMoving + ZipperValues<V>,
{
    fn val(&self) -> Option<&V> {
        if let Some(idx) = self.factor_idx(true) {
            self.secondary[idx].val()
        } else {
            self.primary.val()
        }
    }
}

impl<'trie, PrimaryZ, SecondaryZ, V> ZipperReadOnlyValues<'trie, V>
    for ProductZipperG<'trie, PrimaryZ, SecondaryZ, V>
    where
        V: Clone + Send + Sync,
        PrimaryZ: ZipperMoving + ZipperReadOnlyValues<'trie, V>,
        SecondaryZ: ZipperMoving + ZipperReadOnlyValues<'trie, V>,
{
    fn get_val(&self) -> Option<&'trie V> {
        if let Some(idx) = self.factor_idx(true) {
            self.secondary[idx].get_val()
        } else {
            self.primary.get_val()
        }
    }
}

impl<'trie, PrimaryZ, SecondaryZ, V> ZipperReadOnlyConditionalValues<'trie, V>
    for ProductZipperG<'trie, PrimaryZ, SecondaryZ, V>
    where
        V: Clone + Send + Sync,
        PrimaryZ: ZipperMoving + ZipperReadOnlyValues<'trie, V>,
        SecondaryZ: ZipperMoving + ZipperReadOnlyValues<'trie, V>,
{
    type WitnessT = ();
    fn witness<'w>(&self) -> Self::WitnessT { () }
    fn get_val_with_witness<'w>(&self, _witness: &'w Self::WitnessT) -> Option<&'w V> where 'trie: 'w {
        self.get_val()
    }
}

impl<'trie, PrimaryZ, SecondaryZ, V> Zipper for ProductZipperG<'trie, PrimaryZ, SecondaryZ, V>
    where
        V: Clone + Send + Sync,
        PrimaryZ: ZipperMoving + Zipper,
        SecondaryZ: ZipperMoving + Zipper,
{
    fn path_exists(&self) -> bool {
        if let Some(idx) = self.factor_idx(true) {
            self.secondary[idx].path_exists()
        } else {
            self.primary.path_exists()
        }
    }
    fn is_val(&self) -> bool {
        if let Some(idx) = self.factor_idx(true) {
            self.secondary[idx].is_val()
        } else {
            self.primary.is_val()
        }
    }
    fn child_count(&self) -> usize {
        if let Some(idx) = self.factor_idx(false) {
            self.secondary[idx].child_count()
        } else {
            self.primary.child_count()
        }
    }
    fn child_mask(&self) -> ByteMask {
        if let Some(idx) = self.factor_idx(false) {
            self.secondary[idx].child_mask()
        } else {
            self.primary.child_mask()
        }
    }
}

impl<'trie, PrimaryZ, SecondaryZ, V> ZipperMoving for ProductZipperG<'trie, PrimaryZ, SecondaryZ, V>
    where
        V: Clone + Send + Sync,
        PrimaryZ: ZipperMoving,
        SecondaryZ: ZipperMoving,
{
    fn at_root(&self) -> bool {
        self.path().is_empty()
    }
    fn reset(&mut self) {
        self.factor_paths.clear();
        for secondary in &mut self.secondary {
            secondary.reset();
        }
        self.primary.reset();
    }
    #[inline]
    fn path(&self) -> &[u8] {
        self.primary.path()
    }
    fn val_count(&self) -> usize {
        unimplemented!("method will probably get removed")
    }
    fn descend_to_existing<K: AsRef<[u8]>>(&mut self, path: K) -> usize {
        let mut path = path.as_ref();
        let mut descended = 0;
        'descend: while !path.is_empty() {
            self.enter_factors();
            let good;
            if let Some(idx) = self.factor_idx(false) {
                good = self.secondary[idx].descend_to_existing(path);
                self.primary.descend_to(&path[..good]);
            } else {
                good = self.primary.descend_to_existing(path);
            };
            if good == 0 {
                break 'descend;
            }
            descended += good;
            path = &path[good..];
        }
        self.enter_factors();
        descended
    }
    fn descend_to<K: AsRef<[u8]>>(&mut self, path: K) -> bool {
        let path = path.as_ref();
        let good = self.descend_to_existing(path);
        if good == path.len() {
            return true;
        }
        let rest = &path[good..];
        if let Some(idx) = self.factor_idx(false) {
            self.secondary[idx].descend_to(rest);
        }

        self.primary.descend_to(rest);
        false
    }
    #[inline]
    fn descend_to_byte(&mut self, k: u8) -> bool {
        self.descend_to([k])
    }
    fn descend_indexed_byte(&mut self, child_idx: usize) -> bool {
        let mask = self.child_mask();
        let Some(byte) = mask.indexed_bit::<true>(child_idx) else {
            return false;
        };
        self.descend_to_byte(byte)
    }
    #[inline]
    fn descend_first_byte(&mut self) -> bool {
        self.descend_indexed_byte(0)
    }
    fn descend_until(&mut self) -> bool {
        self.enter_factors();
        let rv = if let Some(idx) = self.factor_idx(false) {
            let zipper = &mut self.secondary[idx];
            let before = zipper.path().len();
            let rv = zipper.descend_until();
            let path = zipper.path();
            if path.len() > before {
                self.primary.descend_to(&path[before..]);
            }
            rv
        } else {
            self.primary.descend_until()
        };
        self.enter_factors();
        rv
    }
    #[inline]
    fn to_next_sibling_byte(&mut self) -> bool {
        self.to_sibling_byte(true)
    }
    #[inline]
    fn to_prev_sibling_byte(&mut self) -> bool {
        self.to_sibling_byte(false)
    }
    fn ascend(&mut self, mut steps: usize) -> bool {
        while steps > 0 {
            self.exit_factors();
            if let Some(idx) = self.factor_idx(false) {
                let len = self.path().len() - self.factor_paths[idx];
                let delta = len.min(steps);
                self.secondary[idx].ascend(delta);
                self.primary.ascend(delta);
                steps -= delta;
            } else {
                return self.primary.ascend(steps);
            }
        }
        true
    }
    #[inline]
    fn ascend_byte(&mut self) -> bool {
        self.ascend(1)
    }
    #[inline]
    fn ascend_until(&mut self) -> bool {
        self.ascend_cond(true)
    }
    #[inline]
    fn ascend_until_branch(&mut self) -> bool {
        self.ascend_cond(false)
    }
}

impl<'trie, PrimaryZ, SecondaryZ, V> ZipperIteration
for ProductZipperG<'trie, PrimaryZ, SecondaryZ, V>
    where
        V: Clone + Send + Sync,
        PrimaryZ: ZipperIteration,
        SecondaryZ: ZipperIteration,
{ } //Use the default impl for all methods

#[cfg(test)]
mod tests {
    use crate::utils::ByteMask;
    use crate::zipper::*;
    use crate::PathMap;
    use crate::morphisms::{Catamorphism, SplitCata};

    macro_rules! impl_product_zipper_tests {
        ($mod:ident, $ProductZipper:ident, $convert:ident) => {
            impl_product_zipper_tests!($mod, $ProductZipper, $convert, read_zipper);
        };
        ($mod:ident, $ProductZipper:ident, $convert:ident, $read_zipper_u64:ident) => {
            // --- START OF MACRO GENERATED MOD ---
            pub mod $mod {
                use super::*;
    /// Tests a very simple two-level product zipper
    #[test]
    fn product_zipper_test1() {
        let keys = [b"AAa", b"AAb", b"AAc"];
        let keys2 = [b"DDd", b"EEe", b"FFf"];
        let map = PathMap::from_iter(keys.into_iter().enumerate().map(|(i, v)| (v, i as u64)));
        let map2 = PathMap::from_iter(keys2.into_iter().enumerate().map(|(i, v)| (v, (i + 1000) as u64)));
        $convert!(*map);
        $convert!(*map2);

        let rz = map.$read_zipper_u64();
        let mut pz = $ProductZipper::new(rz, [map2.$read_zipper_u64()]);

        //Descend within the first factor
        assert!(pz.descend_to(b"AA"));
        assert_eq!(pz.path(), b"AA");
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.child_count(), 3);
        assert!(pz.descend_to(b"a"));
        assert_eq!(pz.path(), b"AAa");
        assert_eq!(pz.get_val(), Some(&0));
        assert_eq!(pz.child_count(), 3);

        //Step to the next factor
        assert!(pz.descend_to(b"DD"));
        assert_eq!(pz.path(), b"AAaDD");
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.child_count(), 1);
        assert!(pz.descend_to(b"d"));
        assert_eq!(pz.path(), b"AAaDDd");
        assert_eq!(pz.get_val(), Some(&1000));
        assert_eq!(pz.child_count(), 0);
        assert!(!pz.descend_to(b"GGg"));
        assert_eq!(pz.path(), b"AAaDDdGGg");
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.child_count(), 0);

        //Test Reset, if the zipper was in another factor
        pz.reset();
        assert_eq!(pz.path(), b"");
        assert!(pz.descend_to(b"AA"));
        assert_eq!(pz.path(), b"AA");
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.child_count(), 3);

        //Try to descend to a non-existent path that would be within the first factor
        assert!(!pz.descend_to(b"aBBb"));
        assert_eq!(pz.path(), b"AAaBBb");
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.child_count(), 0);

        //Now descend to the second factor in one jump
        pz.reset();
        assert!(pz.descend_to(b"AAaDD"));
        assert_eq!(pz.path(), b"AAaDD");
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.child_count(), 1);
        pz.reset();
        assert!(pz.descend_to(b"AAaDDd"));
        assert_eq!(pz.path(), b"AAaDDd");
        assert_eq!(pz.get_val(), Some(&1000));
        assert_eq!(pz.child_count(), 0);
        assert!(!pz.descend_to(b"GG"));
        assert_eq!(pz.path(), b"AAaDDdGG");
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.child_count(), 0);

        //Make sure we can ascend out of a secondary factor; in this sub-test we'll hit the path middles
        assert!(pz.ascend(1));
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.path(), b"AAaDDdG");
        assert_eq!(pz.child_count(), 0);
        assert!(pz.ascend(3));
        assert_eq!(pz.path(), b"AAaD");
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.child_count(), 1);
        assert!(pz.ascend(2));
        assert_eq!(pz.path(), b"AA");
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.child_count(), 3);
        assert!(!pz.ascend(3));
        assert_eq!(pz.path(), b"");
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.child_count(), 1);
        assert!(pz.at_root());

        assert!(!pz.descend_to(b"AAaDDdGG"));
        assert_eq!(pz.path(), b"AAaDDdGG");
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.child_count(), 0);

        //Now try to hit the path transition points
        assert!(pz.ascend(2));
        assert_eq!(pz.path(), b"AAaDDd");
        assert_eq!(pz.get_val(), Some(&1000));
        assert_eq!(pz.child_count(), 0);
        assert!(pz.ascend(3));
        assert_eq!(pz.path(), b"AAa");
        assert_eq!(pz.get_val(), Some(&0));
        assert_eq!(pz.child_count(), 3);
        assert!(pz.ascend(3));
        assert_eq!(pz.path(), b"");
        assert_eq!(pz.get_val(), None);
        assert_eq!(pz.child_count(), 1);
        assert!(pz.at_root());
    }

    /// Tests a 3-level product zipper, with a catamorphism, and no funny-business in the tries
    #[test]
    fn product_zipper_test2() {
        let lpaths = ["abcdefghijklmnopqrstuvwxyz".as_bytes(), "arrow".as_bytes(), "x".as_bytes()];
        let rpaths = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes(), "a".as_bytes(), "bow".as_bytes()];
        let epaths = ["foo".as_bytes(), "pho".as_bytes()];
        let l = PathMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = PathMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = PathMap::from_iter(epaths.iter().map(|x| (x, ())));
        $convert!(l);
        $convert!(r);
        $convert!(e);
        let p = $ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);

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
        let l = PathMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = PathMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = PathMap::from_iter(epaths.iter().map(|x| (x, ())));
        $convert!(l);
        $convert!(r);
        $convert!(e);
        let p = $ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);

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
        assert_eq!(collapse_cnt, 25);
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
        let l = PathMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = PathMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = PathMap::from_iter(epaths.iter().map(|x| (x, ())));
        $convert!(l);
        $convert!(r);
        $convert!(e);
        let mut p = $ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);

        p.descend_to("abcdefghijklmnopqrstuvwxyzbow");
        assert!(p.is_val());
        assert_eq!(p.child_count(), 2);
        assert_eq!(p.child_mask(), ByteMask::from_iter([b'p', b'f']));

        p.descend_first_byte();
        p.ascend_byte();
        assert!(p.is_val());
        assert_eq!(p.child_count(), 2);
        assert_eq!(p.child_mask(), ByteMask::from_iter([b'p', b'f']));
    }

    #[test]
    fn product_zipper_test5() {
        let lpaths = ["abcdefghijklmnopqrstuvwxyz".as_bytes(), "arrow".as_bytes(), "x".as_bytes(), "arr".as_bytes()];
        let rpaths = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes(), "a".as_bytes(), "bow".as_bytes(), "bo".as_bytes()];
        let epaths = ["foo".as_bytes(), "pho".as_bytes(), "f".as_bytes()];
        let l = PathMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = PathMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = PathMap::from_iter(epaths.iter().map(|x| (x, ())));
        $convert!(l);
        $convert!(r);
        $convert!(e);

        {
            let mut p = $ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);
            assert!(p.descend_to("abcdefghijklmnopqrstuvwxyzbowfo"));
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbowfo");
            assert!(p.descend_first_byte());
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbowfoo");
        }
        {
            let mut p = $ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);
            p.descend_to("abcdefghijklmnopqrstuvwxyzbowf");
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbowf");
            assert!(p.is_val());
            assert!(p.descend_to("oo"));
            assert!(p.is_val());
        }
        {
            let mut p = $ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);
            p.descend_to("abcdefghijklmnopqrstuvwxyzbowfo");
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbowfo");
            assert!(p.ascend_byte());
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbowf");
            assert!(p.ascend_byte());
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbow");
            assert!(p.descend_to_byte(b'p'));
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbowp");
            assert!(p.descend_to_byte(b'h'));
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbowph");
            assert!(p.descend_to_byte(b'o'));
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbowpho");
            assert!(p.is_val());
            assert!(p.ascend_until());
            assert_eq!(p.path(), b"abcdefghijklmnopqrstuvwxyzbow");
            assert!(p.ascend(3));
            assert_eq!(vec![b'A', b'a', b'b'], p.child_mask().iter().collect::<Vec<_>>());
            assert!(p.descend_to("ABCDEFGHIJKLMNOPQRSTUVWXYZ"));
            assert_eq!(vec![b'f', b'p'], p.child_mask().iter().collect::<Vec<_>>())
        }
    }

    #[test]
    fn product_zipper_test6() {
        let lpaths = ["abcdefghijklmnopqrstuvwxyz".as_bytes(), "arrow".as_bytes(), "x".as_bytes(), "arr".as_bytes()];
        let rpaths = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes(), "a".as_bytes(), "bow".as_bytes(), "bo".as_bytes()];
        let epaths = ["foo".as_bytes(), "pho".as_bytes(), "f".as_bytes()];
        let l = PathMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = PathMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = PathMap::from_iter(epaths.iter().map(|x| (x, ())));
        $convert!(l);
        $convert!(r);
        $convert!(e);

        {
            let mut p = $ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);
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
        let a = PathMap::from_iter(apaths.iter().map(|x| (x, ())));
        let b = PathMap::from_iter(bpaths.iter().map(|x| (x, ())));
        let c = PathMap::from_iter(cpaths.iter().map(|x| (x, ())));
        $convert!(a);
        $convert!(b);
        $convert!(c);
        let mut p1 = $ProductZipper::new(a.read_zipper(), [b.read_zipper(), c.read_zipper()]);
        let mut p2 = $ProductZipper::new(a.read_zipper(), [b.read_zipper(), c.read_zipper()]);

        // Reference
        for _ in 0..23 {
            p1.descend_first_byte();
        }
        assert_eq!(p1.path_exists(), true);
        assert_eq!(p1.path(), b"arrowheadbowieclubhouse");
        assert!(p1.is_val());

        // Validate that I can do the same thing with descend_to()
        p2.descend_to(b"arrowheadbowieclubhouse");
        assert_eq!(p2.path_exists(), true);
        assert_eq!(p2.path(), b"arrowheadbowieclubhouse");
        assert!(p2.is_val());

        // Validate that I can back up, and re-descend
        {
            p2.ascend(20);
            assert_eq!(p2.path(), b"arr");
            assert_eq!(p2.path_exists(), true);
            assert!(p2.is_val());

            p2.descend_to(b"owheadbowieclub");
            assert_eq!(p2.path(), b"arrowheadbowieclub");
            assert_eq!(p2.path_exists(), true);
            assert!(p2.is_val());
        }

        //Now descend to a non-existent path off of the first factor, and re-ascend to
        // an existing path
        {
            p2.reset();
            // "arrowbow" should't exist because the path continues from "arrowhead"
            p2.descend_to(b"arrowbow");
            assert_eq!(p2.path(), b"arrowbow");
            assert_eq!(p2.path_exists(), false);

            // "arrowbowclub" should't exist because we started in a trie that doesn't exist
            p2.descend_to(b"club");
            assert_eq!(p2.path(), b"arrowbowclub");
            assert_eq!(p2.path_exists(), false);

            p2.ascend(9);
            assert_eq!(p2.path(), b"arr");
            assert_eq!(p2.path_exists(), true);
            assert!(p2.is_val());

            p2.descend_to(b"owheadbowieclub");
            assert_eq!(p2.path(), b"arrowheadbowieclub");
            assert_eq!(p2.path_exists(), true);
            assert!(p2.is_val());
        }

        //Now descend to a non-existent path off of the second factor, and re-ascend to
        // get back to an existing path
        {
            p2.reset();
            // "arrowheadbowclub" should't exist because the path continues from "bowie"
            p2.descend_to(b"arrowheadbowclub");
            assert_eq!(p2.path(), b"arrowheadbowclub");
            assert_eq!(p2.path_exists(), false);

            p2.ascend(5);
            assert_eq!(p2.path(), b"arrowheadbo");
            assert_eq!(p2.path_exists(), true);
            assert!(p2.is_val());

            p2.descend_to(b"wieclub");
            assert_eq!(p2.path(), b"arrowheadbowieclub");
            assert_eq!(p2.path_exists(), true);
            assert!(p2.is_val());
        }
    }

    #[test]
    fn product_zipper_test8() {
        let lpaths = ["abcdefghijklmnopqrstuvwxyz".as_bytes(), "arr".as_bytes(), "arrow".as_bytes(), "x".as_bytes()];
        let rpaths = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes(), "a".as_bytes(), "bo".as_bytes(), "bow".as_bytes(), "bat".as_bytes(), "bit".as_bytes()];
        let epaths = ["foo".as_bytes(), "pho".as_bytes(), "f".as_bytes()];
        let l = PathMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = PathMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = PathMap::from_iter(epaths.iter().map(|x| (x, ())));
        $convert!(l);
        $convert!(r);
        $convert!(e);

        let new_pz = || $ProductZipper::new(l.read_zipper(), [r.read_zipper(), e.read_zipper()]);

        let mut moving_pz = new_pz();
        let cata_pz = new_pz();
        cata_pz.into_cata_side_effect(|_, _, _, path| {
            // println!("{}", String::from_utf8_lossy(path));
            let overlap = crate::utils::find_prefix_overlap(path, moving_pz.path());
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
            assert_eq!(moving_pz.val(), fresh_pz.val());
            assert_eq!(moving_pz.child_count(), fresh_pz.child_count());
            assert_eq!(moving_pz.child_mask(), fresh_pz.child_mask());
        })
    }

    /// Tests the ProductZipper's implementation of `to_next_k_path` or `to_next_sibling_byte`, stepping across factors
    #[test]
    fn product_zipper_test9() {
        let paths = [
            vec![3, 196, 50, 193, 52],
            vec![3, 196, 50, 194, 49, 54],
            vec![3, 196, 50, 194, 49, 55],
        ];
        let map: PathMap<()> = paths.iter().map(|path| (path, ())).collect();
        $convert!(map);
        let mut z = $ProductZipper::new(map.read_zipper(), [map.read_zipper(), map.read_zipper()]);

        assert_eq!(z.descend_to([3, 196, 50, 193, 52, 3, 196, 50, 194, 49, 54]), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.to_next_k_path(2), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.child_mask(), ByteMask::from_iter([3]));
    }

    /// Reproduce a bug in ProductZipperG, where continuing to descend past a non-existent
    /// path somehow leads to re-entering the next factor
    #[test]
    fn product_zipper_testa() {
        let paths = [
            vec![3, 196, 101, 100, 103, 101, 193, 49, 194, 49, 56],
            vec![3, 196, 101, 100, 103, 101, 193, 49, 194, 50, 48],
        ];
        let map: PathMap<()> = paths.iter().map(|path| (path, ())).collect();
        $convert!(map);
        let mut z = $ProductZipper::new(map.read_zipper(), [map.read_zipper(), map.read_zipper()]);

        assert_eq!(z.path_exists(), true);
        assert_eq!(z.descend_to_byte(3), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.descend_to_byte(196), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.descend_to([101, 100, 103, 101]), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.descend_to_byte(193), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.path(), [3, 196, 101, 100, 103, 101, 193]);
        assert_eq!(z.descend_to_byte(194), false);
        assert_eq!(z.path(), [3, 196, 101, 100, 103, 101, 193, 194]);
        assert_eq!(z.path_exists(), false);
        assert_eq!(z.descend_to_byte(3), false);
        assert_eq!(z.path(), [3, 196, 101, 100, 103, 101, 193, 194, 3]);
        assert_eq!(z.path_exists(), false);
    }

    /// Test focussed heavily on `descend_to_byte`, which stitches at dangling paths
    #[test]
    fn product_zipper_testb() {
        let paths = [
            vec![3, 196, 101, 49],
            vec![3, 196, 101, 50],
        ];
        let mut map = PathMap::<()>::new();
        paths.into_iter().for_each(|path| { map.create_path(path); });
        map.insert([3, 196], ());
        map.insert([3, 196, 101, 49], ());
        $convert!(map);
        let mut z = $ProductZipper::new(map.read_zipper(), [map.read_zipper(), map.read_zipper()]);

        assert_eq!(z.path_exists(), true);
        assert_eq!(z.descend_to_byte(3), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.is_val(), false);
        assert_eq!(z.descend_to_byte(196), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.is_val(), true);
        assert_eq!(z.descend_to_byte(101), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.is_val(), false);
        assert_eq!(z.descend_to_byte(50), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.is_val(), false);
        assert_eq!(z.descend_to_byte(3), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.is_val(), false);
        assert_eq!(z.descend_to_byte(196), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.is_val(), true);
        assert_eq!(z.descend_to_byte(101), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.is_val(), false);
        assert_eq!(z.descend_to_byte(49), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.is_val(), true);
        assert_eq!(z.descend_to_byte(3), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.is_val(), false);
        assert_eq!(z.descend_to_byte(196), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.is_val(), true);
        assert_eq!(z.descend_to_byte(101), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.is_val(), false);
        assert_eq!(z.descend_to_byte(50), true);
        assert_eq!(z.path_exists(), true);
        assert_eq!(z.is_val(), false);
        assert_eq!(z.descend_to_byte(3), false);
        assert_eq!(z.path_exists(), false);
        assert_eq!(z.is_val(), false);
    }

    #[test]
    fn product_zipper_inspection_test() {
        let lpaths = ["abcdefghijklmnopqrstuvwxyz".as_bytes(), "arr".as_bytes(), "arrow".as_bytes(), "x".as_bytes()];
        let rpaths = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes(), "a".as_bytes(), "bo".as_bytes(), "bow".as_bytes(), "bat".as_bytes(), "bit".as_bytes()];
        let epaths = ["foo".as_bytes(), "pho".as_bytes(), "f".as_bytes()];
        let l = PathMap::from_iter(lpaths.iter().map(|x| (x, ())));
        let r = PathMap::from_iter(rpaths.iter().map(|x| (x, ())));
        let e = PathMap::from_iter(epaths.iter().map(|x| (x, ())));
        $convert!(l);
        $convert!(r);
        $convert!(e);
        let mut pz = $ProductZipper::new(l.read_zipper_at_borrowed_path(b"abcdefghijklm"), [r.read_zipper(), e.read_zipper()]);

        assert_eq!(pz.factor_count(), 3);
        assert_eq!(pz.focus_factor(), 0);
        assert_eq!(pz.path_indices().len(), 0);
        assert_eq!(pz.path(), b"");
        assert_eq!(pz.origin_path(), b"abcdefghijklm");

        assert!(pz.descend_to(b"nopqrstuvwxyz"));

        assert_eq!(pz.focus_factor(), 0);
        assert_eq!(pz.path(), b"nopqrstuvwxyz");
        assert_eq!(pz.origin_path(), b"abcdefghijklmnopqrstuvwxyz");

        assert!(pz.descend_to(b"AB"));

        assert_eq!(pz.focus_factor(), 1);
        assert_eq!(pz.path_indices()[0], 13);
        assert_eq!(pz.path().len(), 15);
        assert_eq!(pz.path(), b"nopqrstuvwxyzAB");
        assert_eq!(pz.origin_path(), b"abcdefghijklmnopqrstuvwxyzAB");

        pz.reset();
        assert_eq!(pz.child_mask(), ByteMask::from_iter([b'n']));
        assert!(pz.descend_to(b"nopqrstuvwxyzbowph"));
        assert_eq!(pz.focus_factor(), 2);
        assert_eq!(pz.path_indices()[0], 13);
        assert_eq!(pz.path_indices()[1], 16);
        assert_eq!(pz.path(), b"nopqrstuvwxyzbowph");
    }
            }
            // --- END OF MACRO GENERATED MOD ---
        };
    }

    macro_rules! noop { ($x:ident) => {}; (*$x:ident) => {}; }
    impl_product_zipper_tests!(pz_concrete, ProductZipper, noop);
    impl_product_zipper_tests!(pz_generic, ProductZipperG, noop);

    #[cfg(feature="arena_compact")]
    macro_rules! to_act {
        (*$x:ident) => {
            to_act!($x, |x| *x);
        };
        ($x:ident) => {
            to_act!($x, |_x| 0);
        };
        ($x:ident, $m:expr) => {
            let $x = crate::arena_compact::ArenaCompactTree::from_zipper($x.read_zipper(), $m);
        };
    }

    #[cfg(feature="arena_compact")]
    impl_product_zipper_tests!(pz_generic_act, ProductZipperG, to_act, read_zipper_u64);

    crate::zipper::zipper_moving_tests::zipper_moving_tests!(product_zipper,
        |keys: &[&[u8]]| {
            let mut btm = PathMap::new();
            keys.iter().for_each(|k| { btm.set_val_at(k, ()); });
            btm
        },
        |btm: &mut PathMap<()>, path: &[u8]| -> _ {
            ProductZipper::new::<_, TrieRef<()>, _>(btm.read_zipper_at_path(path), [])
    });

    crate::zipper::zipper_iteration_tests::zipper_iteration_tests!(product_zipper,
        |keys: &[&[u8]]| {
            let mut btm = PathMap::new();
            keys.iter().for_each(|k| { btm.set_val_at(k, ()); });
            btm
        },
        |btm: &mut PathMap<()>, path: &[u8]| -> _ {
            ProductZipper::new::<_, TrieRef<()>, _>(btm.read_zipper_at_path(path), [])
    });

    crate::zipper::zipper_moving_tests::zipper_moving_tests!(product_zipper_generic,
        |keys: &[&[u8]]| {
            let mut btm = PathMap::new();
            keys.iter().for_each(|k| { btm.set_val_at(k, ()); });
            btm
        },
        |btm: &mut PathMap<()>, path: &[u8]| -> _ {
            ProductZipperG::new::<[ReadZipperUntracked<()>; 0]>(btm.read_zipper_at_path(path), [])
    });

    crate::zipper::zipper_iteration_tests::zipper_iteration_tests!(product_zipper_generic,
        |keys: &[&[u8]]| {
            let mut btm = PathMap::new();
            keys.iter().for_each(|k| { btm.set_val_at(k, ()); });
            btm
        },
        |btm: &mut PathMap<()>, path: &[u8]| -> _ {
            ProductZipperG::new::<[ReadZipperUntracked<()>; 0]>(btm.read_zipper_at_path(path), [])
    });
}

//POSSIBLE FUTURE DIRECTION:
// A ProductZipper appears to create a new space for the purposes of zipper movement, but
// the space is an ephemeral projection.  Unlike other space operations, if the user tried
// to graft this space or materialize it into a map, they would get something that would
// not match their expectations.
//
// This is the reason the ProductZipper doesn't implement the `ZipperSubtries` trait, and
// why it cannot supply a source for `graft`, `make_map`, or any other algebraic ops.
//
// A more holistic way of performing this kind of transformation is likely desirable, but
// that has a number of unexplored complications such as the impact to exclusivity (is this
// de-facto aliasing?) and how the linkages could be parameterized after-the-fact, or
// re-parameterized en masse (without visiting each node in the sub-space), or when the
// parameters would be allowed to change vs. when they must remain constant.
//