//! Functionality for applying various morphisms to [PathMap] and [Zipper]s
//!
//! Morphisms are a generalization of the [fold](std::iter::Iterator::fold) pattern, but for a trie or
//! sub-trie, as opposed to a list / sequence.  Similarly they rely on results being aggregated into
//! intermediate structures that are themselves joined together to produce a result.
//!
//! ### Supported Morphisms:
//!
//! #### Catamorphism
//!
//! Process a trie from the leaves towards the root.  This algorithm proceeds in a depth-first order.
//!
//! - A `map` closure is called on each leaf value; e.g. a value at location where there is no other value
//! deeper in the trie accessible from that location.  As the algoritm ascends.
//! - A `collapse` closure is called to merge a value along the path with the intermediate structure being
//! carried up the trie.
//! - An `alg` closure integrates intermediate structures from downstream branches into a single structure
//! for that location.
//!
//! The word "catamorphism" comes from the Greek for "down", because the root is considered the bottom
//! of the trie.  This is confusing because elsewhere we use the convention that `descend_` "deeper"
//! into the trie means moving further from the root while `ascend` moves closer to the root.  The docs
//! will stick to this convention in-spite of the Greek meaning.
//!
//! **NOTE**: The traversal order, while depth-first, is subtly different from the order of
//! [ZipperIteration::to_next_val] and [ZipperIteration::to_next_step].  The [Zipper] methods visit values
//! first before descending to the branches below, while the `cata` methods call the `mapper` on the deepest
//! values first, before returning to higher levels where `collapse` is called.
//!
//! #### Anamorphism
//!
//! Generate a trie from the root.  Conceptually it is the inverse of catamorphism.  This algorithm proceeds
//! from a starting point corresponding to a root of a trie, and generates branches and leaves recursively.
//!
//! ### Jumping Morphisms and the `jump` closure
//!
//! Ordinary morphism methods conceptually operate on a trie of bytes.  Therefore they execute the `alg`
//! closure for all non-leaf path positions, regardless of the existence of values.
//!
//! By contrast, `_jumping` methods conceptually operate on a trie of values.  That means the `alg` closure
//! is only called at "forking" path positions from which multiple downstream branches descend, and also for
//! the root.  There is an additional `jump` closure passed to these methods that can process an entire
//! substring from the path.
//!
//! In general, `jumping` methods will perform substantially better, so you should use them if your `alg`
//! closure simply passes the intermediate structure upwards when there is only one child branch.
//!
//! ### Side-Effecting vs Factored Iteration
//!
//! Many methods come (or will come) with a `_side_effect` and an ordinary or `factored` flavor.  The
//! algorithm is identical in both variants but the one to use depends on your situation.
//!
//! The `_side_effect` flavor of the methods will exhaustively traverse the entire trie, irrespective of
//! structural sharing within the trie.  So a subtrie that is included `n` times will be visited `n` times.
//! They take [`FnMut`] closure arguments, meaning the closures can produce side-effects, and making these
//! methods useful for things like serialization, etc.
//!
//! The ordinary `factored` flavor takes sharing into account, and thus will only visit each shared subtrie
//! once.  The methods take [`Fn`] closure arguments and they may cache and re-use intermediate results.
//! This means the intermediate result type must implement [`Clone`].
//!
//! In general, the ordinary methods should be preferred unless sife-effects are necessary, because many
//! operations produce structural sharing so the ordinary `factored` methods will likely be more efficient.
//!


//GOAT QUESTION: Why not combine map_f and collapse_F by passing `Option<W>` to the closure?

// GOAT!! Are these names (collapse, and alg) part of math canon?  They are not very descriptive and hopefully we can change them.
// "map" is good because it's a perfect equivalent to map in map->reduce.

use crate::trie_map::BytesTrieMap;
use crate::zipper::*;

pub trait Catamorphism<V> {
    /// Applies a catamorphism to the trie descending from the zipper's root
    ///
    /// ## Args
    /// - `map_f`: `mapper(v: &V, path: &[u8]) -> W`
    /// Maps value `v` at a leaf `path` into an intermediate result
    ///
    /// - `collapse_f`: `collapse(v: &V, w: W, path: &[u8]) -> W`
    /// Folds value `v` at a non-leaf `path` with the aggregated results from the trie below `path`
    ///
    /// - `alg_f`: `alg(m: [u64; 4], cs: &mut [W], path: &[u8]) -> W`
    /// Aggregates the results from the child branches, `cs`, descending from `path` into a single result
    ///
    /// The focus position of the zipper will be ignored and it will be immediately reset to the root.
    ///
    /// In all cases, the `path` arg is the [origin_path](ZipperAbsolutePath::origin_path)
    fn into_cata_side_effect<W, MapF, CollapseF, AlgF>(self, map_f: MapF, collapse_f: CollapseF, alg_f: AlgF) -> W
        where
        MapF: FnMut(&V, &[u8]) -> W,
        CollapseF: FnMut(&V, W, &[u8]) -> W,
        AlgF: FnMut(&[u64; 4], &mut [W], &[u8]) -> W;

    /// Applies a "jumping" catamorphism to the trie
    ///
    /// ## Args
    /// - `jump_f`: `FnMut(sub_path: &[u8], w: W, path: &[u8]) -> W`
    /// Elevates a result `w` descending from the relative path, `sub_path` to the current position at `path`
    ///
    /// See [into_cata_side_effect](ZipperMorphisms::into_cata_side_effect) for explanation of other behavior
    fn into_cata_jumping_side_effect<W, MapF, CollapseF, AlgF, JumpF>(self, map_f: MapF, collapse_f: CollapseF, alg_f: AlgF, jump_f: JumpF) -> W
        where
        MapF: FnMut(&V, &[u8]) -> W,
        CollapseF: FnMut(&V, W, &[u8]) -> W,
        AlgF: FnMut(&[u64; 4], &mut [W], &[u8]) -> W,
        JumpF: FnMut(&[u8], W, &[u8]) -> W;
}

impl<'a, Z, V: 'a> Catamorphism<V> for Z where Z: ReadOnlyZipper<'a, V> + ZipperAbsolutePath {
    fn into_cata_side_effect<W, MapF, CollapseF, AlgF>(self, map_f: MapF, collapse_f: CollapseF, alg_f: AlgF) -> W
        where
        MapF: FnMut(&V, &[u8]) -> W,
        CollapseF: FnMut(&V, W, &[u8]) -> W,
        AlgF: FnMut(&[u64; 4], &mut [W], &[u8]) -> W,
    {
        cata_side_effect_body::<Self, V, W, MapF, CollapseF, AlgF, _, false>(self, map_f, collapse_f, alg_f, |_, _, _| unreachable!())
    }
    fn into_cata_jumping_side_effect<W, MapF, CollapseF, AlgF, JumpF>(self, map_f: MapF, collapse_f: CollapseF, alg_f: AlgF, jump_f: JumpF) -> W
        where
        MapF: FnMut(&V, &[u8]) -> W,
        CollapseF: FnMut(&V, W, &[u8]) -> W,
        AlgF: FnMut(&[u64; 4], &mut [W], &[u8]) -> W,
        JumpF: FnMut(&[u8], W, &[u8]) -> W
    {
        cata_side_effect_body::<Self, V, W, MapF, CollapseF, AlgF, JumpF, true>(self, map_f, collapse_f, alg_f, jump_f)
    }
}

impl<V: 'static + Clone + Send + Sync + Unpin> Catamorphism<V> for BytesTrieMap<V> {
    fn into_cata_side_effect<W, MapF, CollapseF, AlgF>(self, map_f: MapF, collapse_f: CollapseF, alg_f: AlgF) -> W
        where
        MapF: FnMut(&V, &[u8]) -> W,
        CollapseF: FnMut(&V, W, &[u8]) -> W,
        AlgF: FnMut(&[u64; 4], &mut [W], &[u8]) -> W,
    {
        let rz = self.into_read_zipper(&[]);
        cata_side_effect_body::<ReadZipperOwned<V>, V, W, MapF, CollapseF, AlgF, _, false>(rz, map_f, collapse_f, alg_f, |_, _, _| unreachable!())
    }
    fn into_cata_jumping_side_effect<W, MapF, CollapseF, AlgF, JumpF>(self, map_f: MapF, collapse_f: CollapseF, alg_f: AlgF, jump_f: JumpF) -> W
        where
        MapF: FnMut(&V, &[u8]) -> W,
        CollapseF: FnMut(&V, W, &[u8]) -> W,
        AlgF: FnMut(&[u64; 4], &mut [W], &[u8]) -> W,
        JumpF: FnMut(&[u8], W, &[u8]) -> W
    {
        let rz = self.into_read_zipper(&[]);
        cata_side_effect_body::<ReadZipperOwned<V>, V, W, MapF, CollapseF, AlgF, JumpF, true>(rz, map_f, collapse_f, alg_f, jump_f)
    }
}

#[inline(always)]
fn cata_side_effect_body<'a, Z, V: 'a, W, MapF, CollapseF, AlgF, JumpF, const JUMPING: bool>(mut z: Z, mut map_f: MapF, mut collapse_f: CollapseF, mut alg_f: AlgF, mut jump_f: JumpF) -> W
    where
    Z: ReadOnlyZipper<'a, V> + ZipperAbsolutePath,
    MapF: FnMut(&V, &[u8]) -> W,
    CollapseF: FnMut(&V, W, &[u8]) -> W,
    AlgF: FnMut(&[u64; 4], &mut [W], &[u8]) -> W,
    JumpF: FnMut(&[u8], W, &[u8]) -> W,
{
    //`stack` holds a "frame" at each forking point above the zipper position.  No frames exist for values
    let mut stack = Vec::<StackFrame<W>>::with_capacity(12);
    let mut frame_idx = 0;

    z.reset();
    z.prepare_buffers();

    //Push a stack frame for the root, and start on the first branch off the root
    stack.push(StackFrame::new(z.child_count(), z.child_mask()));
    if !z.descend_first_byte() {
        //Empty trie is a special case
        return alg_f(&[0; 4], &mut [], z.origin_path().unwrap());
    }

    loop {
        //Descend to the next forking point if we're jumping
        let mut is_leaf = false;
        while z.child_count() < 2 {
            if !z.descend_until() {
                is_leaf = true;
                break;
            }
        }

        if is_leaf {
            //Map the value from this leaf
            let mut cur_w = z.get_value().map(|v| map_f(v, z.origin_path().unwrap()));

            //Ascend back to the last fork point
            cur_w = ascend_to_fork::<Z, V, W, MapF, CollapseF, AlgF, JumpF, JUMPING>(&mut z, &mut map_f, &mut collapse_f, &mut alg_f, &mut jump_f, cur_w);

            //Merge the result into the stack frame
            match cur_w {
                Some(w) => stack[frame_idx].push_val(w),
                None => stack[frame_idx].push_none(),
            }

            debug_assert!(stack[frame_idx].child_idx <= z.child_count());
            while stack[frame_idx].child_idx == z.child_count() {

                //We finished all the children from this branch, so run the aggregate function
                let stack_frame = &mut stack[frame_idx];
                let mut w = alg_f(&stack_frame.child_mask, &mut stack_frame.children, z.origin_path().unwrap());
                if frame_idx == 0 {
                    return w
                } else {
                    frame_idx -= 1;

                    //Check to see if we have a value here we need to collapse
                    if let Some(v) = z.get_value() {
                        w = collapse_f(v, w, z.origin_path().unwrap());
                    }

                    //Ascend the rest of the way back up to the branch
                    w = ascend_to_fork::<Z, V, W, MapF, CollapseF, AlgF, JumpF, JUMPING>(&mut z, &mut map_f, &mut collapse_f, &mut alg_f, &mut jump_f, Some(w)).unwrap();

                    //Merge the result into the stack frame
                    stack[frame_idx].push_val(w);
                    debug_assert!(stack[frame_idx].child_idx <= z.child_count());
                }
            }

            //Position to descend the next child branch
            let descended = z.descend_indexed_branch(stack[frame_idx].child_idx);
            debug_assert!(descended);
        } else {
            //Push a new stack frame for this branch
            frame_idx += 1;
            let child_mask = z.child_mask();
            let child_cnt = z.child_count();
            if stack.len() <= frame_idx {
                stack.push(StackFrame::new(child_cnt, child_mask));
                debug_assert!(stack.len() > frame_idx);
            } else {
                stack[frame_idx].reset(child_mask);
            }

            //Descend the first child branch
            z.descend_first_byte();
        }
    }
}

#[inline(always)]
fn ascend_to_fork<'a, Z, V: 'a, W, MapF, CollapseF, AlgF, JumpF, const JUMPING: bool>(z: &mut Z, 
        map_f: &mut MapF,
        collapse_f: &mut CollapseF,
        alg_f: &mut AlgF,
        jump_f: &mut JumpF,
        mut cur_w: Option<W>
) -> Option<W>
    where
    Z: ReadOnlyZipper<'a, V> + ZipperAbsolutePath,
    MapF: FnMut(&V, &[u8]) -> W,
    CollapseF: FnMut(&V, W, &[u8]) -> W,
    AlgF: FnMut(&[u64; 4], &mut [W], &[u8]) -> W,
    JumpF: FnMut(&[u8], W, &[u8]) -> W,
{
    loop {
        let mut old_path_len = z.origin_path().unwrap().len();

        let ascended = z.ascend_until();
        debug_assert!(ascended);

        let at_fork = z.child_count() > 1;
        let at_root = z.at_root();

        let new_path_len = z.origin_path().unwrap().len();

        if JUMPING {
            if old_path_len > new_path_len+1 || (!at_fork && !at_root) {
                if let Some(w) = cur_w {
                    cur_w = Some(jump_f(&z.origin_path_assert_len(old_path_len)[new_path_len..], w, z.origin_path().unwrap()));
                }
            }
        } else {
            //This is the default code to call the `alg_f` for each intermediate path step
            // NOTE: the reason this is a special case rather than just a default JumpF is because I want
            // to be able to re-use the `path_buf`, but that's not possible through the defined interface
            while old_path_len > new_path_len {
                let byte = z.origin_path_assert_len(old_path_len).last().unwrap();
                old_path_len -= 1;
                if old_path_len > new_path_len || (!at_fork && !at_root) {
                    if let Some(w) = cur_w {
                        let mut mask = [0u64; 4];
                        let word_idx = (byte / 64) as usize;
                        mask[word_idx] = 1u64 << (byte % 64);
                        cur_w = Some(alg_f(&mask, &mut[w], &z.origin_path_assert_len(old_path_len)));
                    }
                }
            }
        }

        if at_root {
            return cur_w
        }

        if !at_fork {
            //This unwrap shouldn't panic, because `ascend_until` should only stop at values and forks
            let v = z.get_value().unwrap();
            match cur_w {
                None => { cur_w = Some(map_f(v, z.origin_path().unwrap())); },
                Some(w) => { cur_w = Some(collapse_f(v, w, z.origin_path().unwrap())); }
            }
        } else {
            break;
        }
    }
    cur_w
}

/// Internal structure to hold temporary info used inside morphism apply methods
struct StackFrame<W> {
    child_mask: [u64; 4],
    mask_word_idx: usize,
    child_idx: usize,
    remaining_children_mask: u64,
    children: Vec<W>,
}

impl<W> StackFrame<W> {
    /// Allocates a new StackFrame
    fn new(child_count: usize, child_mask: [u64; 4]) -> Self {
        let mut frame = Self {
            child_mask: <_>::default(),
            child_idx: 0,
            mask_word_idx: 0,
            remaining_children_mask: 0,
            children: Vec::with_capacity(child_count)
        };
        frame.reset(child_mask);
        frame
    }
    /// Resets a StackFrame to the state needed to iterate a new forking point
    fn reset(&mut self, child_mask: [u64; 4]) {
        self.mask_word_idx = 0;
        self.child_idx = 0;
        while child_mask[self.mask_word_idx] == 0 && self.mask_word_idx < 3 {
            self.mask_word_idx += 1;
        }
        self.remaining_children_mask = child_mask[self.mask_word_idx];
        self.child_mask = child_mask;
        self.children.clear();
    }
    fn push_val(&mut self, w: W) {
        self.children.push(w);
        self.child_idx += 1;
        debug_assert!(self.remaining_children_mask > 0); //If this assert trips, then it means we're trying to push a value for a non-existent child
        let index = self.remaining_children_mask.trailing_zeros();
        self.remaining_children_mask ^= 1u64 << index;
        self.advance_word_idx();
    }
    fn push_none(&mut self) {
        self.child_idx += 1;
        debug_assert!(self.remaining_children_mask > 0); //If this assert trips, then it means we're trying to push a value for a non-existent child
        let index = self.remaining_children_mask.trailing_zeros();
        self.remaining_children_mask ^= 1u64 << index;
        self.child_mask[self.mask_word_idx] ^= 1u64 << index;
        self.advance_word_idx();
    }
    fn advance_word_idx(&mut self) {
        if self.remaining_children_mask == 0 {
            if self.remaining_children_mask < 3 {
                self.mask_word_idx += 1;
                while self.child_mask[self.mask_word_idx] == 0 && self.mask_word_idx < 3 {
                    self.mask_word_idx += 1;
                }
                self.remaining_children_mask = self.child_mask[self.mask_word_idx];
            }
        }
    }
}

/// Internal function to generate a new root trie node from an anamorphism
pub(crate) fn new_map_from_ana<V, W, AlgF>(w: W, mut alg_f: AlgF) -> BytesTrieMap<V>
    where
    V: 'static + Clone + Send + Sync + Unpin,
    W: Default,
    AlgF: FnMut(W, &mut Option<V>, &mut ChildBuilder<W>, &[u8])
{
    let mut stack = Vec::<(ChildBuilder<W>, usize)>::with_capacity(12);
    let mut frame_idx = 0;

    let mut z = WriteZipperOwned::new();
    let mut val = None;

    //The root is a special case
    stack.push((ChildBuilder::<W>::new(), 0));
    alg_f(w, &mut val, &mut stack[frame_idx].0, z.path());
    stack[frame_idx].0.finalize();
    if let Some(val) = core::mem::take(&mut val) {
        z.set_value(val);
    }
    loop {

        //Should we descend?
        if let Some(w) = stack[frame_idx].0.take_next() {
            //TODO Optimization Opportunity: There is likely a 2x speedup in here, that can be achieved by
            // setting all the children at the same time.  I'm going to hold off until the explicit path
            // API is fully settled, since ideally we'd call a WriteZipper method to set multiple downstream
            // paths, but we'd want some assurances those paths would actually be created, and not pruned
            // because they're empty.
            //BEWARE: This change needs to be accompanied by another change to allow an existing node's path
            // to be augmented, or the above change could be a performance step backwards.  Specifically,
            // consider a ListNode that is created with only one child, based on a mask.  Then, if the zipper
            // descends and tries to create the rest of a path, it would be a disaster if that took the form
            // of another node, as opposed to just putting the rest of the path into the existing node.

            let child_path_byte = stack[frame_idx].0.taken_child_byte();
            z.descend_to_byte(child_path_byte);
            let mut child_path_len = 1;

            if let Some(child_path_remains) = stack[frame_idx].0.taken_child_remaining_path() {
                z.descend_to(child_path_remains);
                child_path_len += child_path_remains.len();
            }

            debug_assert!(frame_idx < stack.len());
            frame_idx += 1;
            if frame_idx == stack.len() {
                stack.push((ChildBuilder::<W>::new(), child_path_len));
            } else {
                stack[frame_idx].0.reset();
                stack[frame_idx].1 = child_path_len;
            }

            //Run the alg if we just descended
            alg_f(w, &mut val, &mut stack[frame_idx].0, z.path());
            stack[frame_idx].0.finalize();
            if let Some(val) = core::mem::take(&mut val) {
                z.set_value(val);
            }
        } else {
            //If not, we should ascend
            if frame_idx == 0 {
                break
            }
            z.ascend(stack[frame_idx].1);
            stack[frame_idx].0.reset();
            frame_idx -= 1;
        }
    }
    z.into_map()
}

/// A [Vec]-like struct for assembling all the downstream branches from a path in the trie
pub struct ChildBuilder<W> {
    child_mask: [u64; 4],
    cur_mask_word: usize,
    cur_struct_idx: usize,
    cur_path_idx: usize,
    real_child_structs_len: usize,
    real_child_paths_len: usize,
    child_paths: Vec<(usize, Vec<u8>)>,
    child_structs: Vec<W>,
}

impl<W: Default> ChildBuilder<W> {
    /// Internal method to make a new empty `ChildBuilder`
    fn new() -> Self {
        Self {
            child_mask: [0u64; 4],
            cur_mask_word: 0,
            cur_struct_idx: 0,
            cur_path_idx: 0,
            real_child_structs_len: 0,
            real_child_paths_len: 0,
            child_paths: vec![],
            child_structs: vec![] }
    }
    /// Internal method.  Clears a builder without freeing its memory
    fn reset(&mut self) {
        self.child_mask = [0u64; 4];
        self.cur_mask_word = 0;
        self.cur_struct_idx = 0;
        self.cur_path_idx = 0;
        self.real_child_structs_len = 0;
        self.real_child_paths_len = 0;
    }
    /// Internal method.  Called after the user code has run to fill the builder, but before we start to empty it
    fn finalize(&mut self) {
        self.cur_mask_word = 0;
        while self.cur_mask_word < 4 && self.child_mask[self.cur_mask_word] == 0 {
            self.cur_mask_word += 1;
        }
    }
    /// Internal method to get the next child from the builder in the push order.  Used by the anamorphism
    fn take_next(&mut self) -> Option<W> {
        if self.cur_struct_idx < self.real_child_structs_len {
            let w = &mut self.child_structs[self.cur_struct_idx];
            self.cur_struct_idx += 1;
            Some(core::mem::take(w))
        } else {
            None
        }
    }
    /// Internal method.  After [Self::take_next] returns `Some`, this method will return the first byte of the
    /// associated path.
    fn taken_child_byte(&mut self) -> u8 {
        let least_component = self.child_mask[self.cur_mask_word].trailing_zeros() as u8;
        debug_assert!(least_component < 64);
        let byte = (self.cur_mask_word * 64) as u8 + least_component;
        self.child_mask[self.cur_mask_word] ^= 1u64 << least_component;
        while self.cur_mask_word < 4 && self.child_mask[self.cur_mask_word] == 0 {
            self.cur_mask_word += 1;
        }
        byte
    }
    /// Internal method.  After [Self::take_next] returns `Some`, this method will return the associated path
    /// beyond the first byte, or `None` if the path is only 1-byte long
    fn taken_child_remaining_path(&mut self) -> Option<&[u8]> {
        debug_assert!(self.cur_path_idx <= self.real_child_paths_len);
        if self.cur_path_idx == self.real_child_paths_len {
            return None
        }
        if self.child_paths[self.cur_path_idx].0 != self.cur_struct_idx-1 {
            return None
        }
        let path = self.child_paths[self.cur_path_idx].1.as_slice();
        self.cur_path_idx += 1;
        Some(path)
    }
    /// Returns the number of children that have been pushed to the `ChildBuilder`, so far
    pub fn len(&self) -> usize {
        self.real_child_structs_len
    }
    /// Simultaneously sets all child branches with single-byte path continuations
    ///
    /// Panics if existing children have already been set / pushed, or if the number of bits set in `mask`
    /// doesn't match `children.len()`.
    pub fn set_child_mask<C: AsMut<[W]>>(&mut self, mask: [u64; 4], mut children: C) {
        if self.real_child_structs_len != 0 {
            panic!("set_mask called over existing children")
        }
        let children = children.as_mut();
        debug_assert_eq!(mask.iter().fold(0, |sum, word| sum + word.count_ones() as usize), children.len());
        if children.len() == 0 {
            return
        }
        self.real_child_structs_len = children.len();
        self.child_structs.clear();
        self.child_structs.extend(children.into_iter().map(|child| core::mem::take(child)));
        debug_assert_eq!(self.cur_mask_word, 0);
        while mask[self.cur_mask_word] == 0 {
            self.cur_mask_word += 1;
        }
        self.child_mask = mask;
    }
    /// Pushes a new child branch into the `ChildBuilder` with the specified `byte`
    ///
    /// Panics if `byte <=` the first byte of any previosuly pushed paths.
    pub fn push_byte(&mut self, byte: u8, w: W) {
        let mask_word = (byte / 64) as usize;
        if mask_word < self.cur_mask_word {
            panic!("children must be pushed in sorted order")
        }
        self.cur_mask_word = mask_word;
        let mask_delta = 1u64 << (byte % 64);
        if self.child_mask[mask_word] >= mask_delta {
            panic!("children must be pushed in sorted order")
        }
        self.child_mask[mask_word] |= mask_delta;

        //Push the `W`
        debug_assert!(self.real_child_structs_len <= self.child_structs.len());
        if self.real_child_structs_len < self.child_structs.len() {
            let child_struct = &mut self.child_structs[self.real_child_structs_len];
            *child_struct = w;
        } else {
            self.child_structs.push(w);
        }
        self.real_child_structs_len += 1;
    }
    /// Pushes a new child branch into the `ChildBuilder` with the specified `sub_path`
    ///
    /// Panics if `sub_path` fails to meet any of the following conditions:
    /// - `sub_path.len() > 0`.
    /// - `sub_path` must not begin with the same byte as any previously-pushed path.
    /// - `sub_path` must alphabetically sort after all previously pushed paths.
    ///
    /// For example, pushing `b"horse"` and then `b"hour"` is wrong.  Instead you should push `b"ho"`, and
    /// push the remaining parts of the path from the triggered closures downstream.
    //
    //TODO, could make a `push_unchecked` method to turn these these checks from `assert!` to `debug_assert`.
    // Not sure if it would make any difference.  Feels unlikely, but might be worth a try after we've implemented
    // the other speedup ideas
    pub fn push(&mut self, sub_path: &[u8], w: W) {
        assert!(sub_path.len() > 0);

        //Push the remaining path
        if sub_path.len() > 1 {
            debug_assert!(self.real_child_paths_len <= self.child_paths.len());
            if self.real_child_paths_len < self.child_paths.len() {
                let child_path = &mut self.child_paths[self.real_child_paths_len];
                child_path.1.clear();
                child_path.1.extend(&sub_path[1..]);
                child_path.0 = self.real_child_structs_len;
            } else {
                self.child_paths.push((self.real_child_structs_len, sub_path[1..].to_vec()));
            }
            self.real_child_paths_len += 1;
        }

        self.push_byte(sub_path[0], w);
    }
    /// Returns the child mask from the `ChildBuilder`, representing paths that have been pushed so far
    pub fn child_mask(&self) -> [u64; 4] {
        self.child_mask
    }
    //GOAT, feature removed.  See below
    // /// Returns an [`Iterator`] type to iterate over the `(sub_path, w)` pairs that have been pushed
    // pub fn iter(&self) -> ChildBuilderIter<'_, W> {
    //     self.into_iter()
    // }
}

//GOAT, the IntoIterator impl is obnoxious because I don't have a contiguous buffer that holds the path anymore
// It's unnecessary anyway, so I'm just going to chuck it
//
// impl<'a, W> IntoIterator for &'a ChildBuilder<W> {
//     type Item = (&'a[u8], &'a W);
//     type IntoIter = ChildBuilderIter<'a, W>;

//     fn into_iter(self) -> Self::IntoIter {
//         ChildBuilderIter {
//             cb: self,
//             cur_mask: self.child_mask[0],
//             mask_word: 0,
//             i: 0
//         }
//     }
// }

// /// An [`Iterator`] type for a [`ChildBuilder`]
// pub struct ChildBuilderIter<'a, W> {
//     cb: &'a ChildBuilder<W>,
//     cur_mask: u64,
//     mask_word: usize,
//     i: usize,
// }

// impl<'a, W> Iterator for ChildBuilderIter<'a, W> {
//     type Item = (&'a[u8], &'a W);
//     fn next(&mut self) -> Option<Self::Item> {
//         while self.mask_word < 4 {
//             let tz = self.cur_mask.trailing_zeros();
//             if tz < 64 {

//                 self.i += 1;
//             } else {
//                 self.mask_word += 1;
//             }
//         }
//         None
//     }
// }

#[cfg(test)]
mod tests {
    use core::ptr::slice_from_raw_parts_mut; //GOAT, UNSAFE does NOT belong in these tests.  We don't need every drop of perf in a correctness test!
    use crate::trie_map::BytesTrieMap;
    use super::*;

    #[test]
    fn cata_test1() {
        let tests = [
            (vec![], 0), //Empty special case
            (vec!["1"], 1), //A branch at the root
            (vec!["1", "2"], 3),
            (vec!["1", "2", "3", "4", "5", "6"], 21),
            (vec!["a1", "a2"], 3), //A branch above the root
            (vec!["a1", "a2", "a3", "a4", "a5", "a6"], 21),
            (vec!["12345"], 5), //A deep leaf
            (vec!["1", "12", "123", "1234", "12345"], 15), //Values along the path
            (vec!["123", "123456", "123789"], 18), //A branch that also has a value
            (vec!["12", "123", "123456", "123789"], 20),
            (vec!["1", "2", "123", "123765", "1234", "12345", "12349"], 29) //A challenging mix of everything
        ];
        for (keys, expected_sum) in tests {
            let map: BytesTrieMap<()> = keys.into_iter().map(|v| (v, ())).collect();
            let zip = map.read_zipper();

            let map_f = |_v: &(), path: &[u8]| {
                let val = (*path.last().unwrap() as char).to_digit(10).unwrap();
                // println!("map path=\"{}\" val={val}", String::from_utf8_lossy(path));
                val
            };
            let collapse_f = |_v: &(), upstream: u32, path: &[u8]| {
                let this_digit = (*path.last().unwrap() as char).to_digit(10).unwrap();
                // println!("collapse path=\"{}\", upstream={upstream}, this_digit={this_digit}, sum={}", String::from_utf8_lossy(path), upstream + this_digit);
                upstream + this_digit
            };
            let alg_f = |_child_mask: &[u64; 4], children: &mut [u32], _path: &[u8]| {
                // println!("aggregate path=\"{}\", children={children:?}", String::from_utf8_lossy(_path));
                children.into_iter().fold(0, |sum, child| sum + *child)
            };
            let jump_f = |_subpath: &[u8], w: u32, _path: &[u8]| {
                // println!("jump sub-path=\"{}\", upstream={w} path=\"{}\",", String::from_utf8_lossy(_subpath), String::from_utf8_lossy(_path));
                w
            };

            //Test both the jumping and non-jumping versions, since they ought to do the same thing
            let sum = zip.clone().into_cata_side_effect(map_f, collapse_f, alg_f);
            assert_eq!(sum, expected_sum);

            let sum = zip.into_cata_jumping_side_effect(map_f, collapse_f, alg_f, jump_f);
            assert_eq!(sum, expected_sum);
        }
    }

    #[test]
    fn cata_test2() {
        let mut btm = BytesTrieMap::new();
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });

        //These algorithms should perform the same with both "jumping" and "non-jumping" versions

        // like val count, but without counting internal values
        let cnt = btm.read_zipper().into_cata_side_effect(
            |_v, _p| { 1usize },
            |_v, w, _p| { w },
            |_m, ws, _p| { ws.iter().sum() });
        assert_eq!(cnt, 11);

        let cnt = btm.read_zipper().into_cata_jumping_side_effect(
            |_v, _p| { 1usize },
            |_v, w, _p| { w },
            |_m, ws, _p| { ws.iter().sum() },
            |_subpath, w, _path| w
        );
        assert_eq!(cnt, 11);

        let longest = btm.read_zipper().into_cata_side_effect(
            |_v, p| { p.to_vec() },
            |_v, w, _p| { w },
            |_m, ws: &mut [Vec<u8>], _p| { ws.iter_mut().max_by_key(|p| p.len()).map_or(vec![], std::mem::take) });
        assert_eq!(std::str::from_utf8(longest.as_slice()).unwrap(), "rubicundus");

        let longest = btm.read_zipper().into_cata_jumping_side_effect(
            |_v, p| { p.to_vec() },
            |_v, w, _p| { w },
            |_m, ws: &mut [Vec<u8>], _p| { ws.iter_mut().max_by_key(|p| p.len()).map_or(vec![], std::mem::take) },
            |_subpath, w, _path| w
        );
        assert_eq!(std::str::from_utf8(longest.as_slice()).unwrap(), "rubicundus");

        let at_truncated = btm.read_zipper().into_cata_side_effect(
            |_v, _p| { vec![] },
            |v, _w, _p| { vec![v.clone()] },
            |_m, ws: &mut [_], _p| {
                let mut r = ws.first_mut().map_or(vec![], std::mem::take);
                for w in ws[1..].iter_mut() { r.extend(w.drain(..)); }
                r
            });
        assert_eq!(at_truncated, vec![3]);

        let at_truncated = btm.read_zipper().into_cata_jumping_side_effect(
            |_v, _p| { vec![] },
            |v, _w, _p| { vec![v.clone()] },
            |_m, ws: &mut [_], _p| {
                let mut r = ws.first_mut().map_or(vec![], std::mem::take);
                for w in ws[1..].iter_mut() { r.extend(w.drain(..)); }
                r
            },
            |_subpath, w, _path| w
        );
        assert_eq!(at_truncated, vec![3]);
    }

    #[test]
    fn cata_test3() {
        let tests = [
            (vec![], 0, 0),
            (vec!["i"], 1, 1), //1 leaf, 1 node
            (vec!["i", "ii"], 2, 1), //1 leaf, 2 total "nodes"
            (vec!["ii", "iiiii"], 5, 1), //1 leaf, 5 total "nodes"
            (vec!["ii", "iii", "iiiii", "iiiiiii"], 7, 1), //1 leaf, 7 total "nodes"
            (vec!["ii", "iiii", "iij", "iijjj"], 7, 3), //2 leaves, 1 fork, 7 total "nodes"
        ];
        for (keys, expected_sum_ordinary, expected_sum_jumping) in tests {
            let map: BytesTrieMap<()> = keys.into_iter().map(|v| (v, ())).collect();
            let zip = map.read_zipper();

            let map_f = |_v: &(), _path: &[u8]| {
                // println!("map path=\"{}\"", String::from_utf8_lossy(_path));
                1
            };
            let collapse_f = |_v: &(), upstream: u32, _path: &[u8]| {
                // println!("collapse path=\"{}\", upstream={upstream}", String::from_utf8_lossy(_path));
                upstream
            };
            let alg_f = |_child_mask: &[u64; 4], children: &mut [u32], path: &[u8]| {
                // println!("aggregate path=\"{}\", children={children:?}", String::from_utf8_lossy(path));
                let sum = children.into_iter().fold(0, |sum, child| sum + *child);
                if path.len() > 0 {
                    sum + 1
                } else {
                    sum
                }
            };

            //Test both the jumping and non-jumping versions
            let sum = zip.clone().into_cata_side_effect(map_f, collapse_f, alg_f);
            assert_eq!(sum, expected_sum_ordinary);

            let sum = zip.into_cata_jumping_side_effect(map_f, collapse_f, alg_f, |_subpath, w, _path| w);
            assert_eq!(sum, expected_sum_jumping);
        }
    }

    /// Generate some basic tries using the [ChildBuilder::push_byte] API
    #[test]
    fn ana_test1() {
        // Generate 5 'i's
        let mut invocations = 0;
        let map: BytesTrieMap<()> = BytesTrieMap::<()>::new_from_ana(5, |idx, val, children, _path| {
            // println!("path=\"{}\"", String::from_utf8_lossy(_path));
            *val = Some(());
            if idx > 0 {
                children.push_byte(b'i', idx - 1)
            }
            invocations += 1;
        });
        assert_eq!(map.val_count(), 5);
        assert_eq!(invocations, 6);

        // Generate all 3-lenght 'L' | 'R' permutations
        let mut invocations = 0;
        let map: BytesTrieMap<()> = BytesTrieMap::<()>::new_from_ana(3, |idx, val, children, _path| {
            // println!("path=\"{}\"", String::from_utf8_lossy(_path));
            if idx > 0 {
                children.push_byte(b'L', idx - 1);
                children.push_byte(b'R', idx - 1);
            } else {
                *val = Some(());
            }
            invocations += 1;
        });
        assert_eq!(map.val_count(), 8);
        assert_eq!(invocations, 15);
    }

    #[test]
    fn ana_test2() {
        // to be revisited with futu or reworked ana
        let file_content = "Hallo,Afrikaans\nPërshëndetje,Albanian\nእው ሰላም ነው,Amharic\nمرحبًا,Arabic\nBarev,Armenian\nKamisaki,Aymara\nSalam,Azerbaijani\nKaixo,Basque\nВітаю,Belarusian\nহ্যালো,Bengali\nZdravo,Bosnian\nЗдравейте,Bulgarian\nဟယ်လို,Burmese\n你好,Cantonese\nHola,Catalan\nKamusta,Cebuano\nMoni,Chichewa\nBonghjornu,Corsican\nZdravo,Croatian\nAhoj,Czech\nHej,Danish\nHallo,Dutch\nHello,English\nTere,Estonian\nHello,Ewe\nسلام,Farsi (Persian)\nBula,Fijian\nKumusta,Filipino\nHei,Finnish\nBonjour,French\nDia dhuit,Gaelic (Irish)\nOla,Galician\nგამარჯობა,Georgian\nGuten tag,German\nγεια,Greek\nMba'éichapa,Guarani\nBonjou,Haitian Creole\nAloha,Hawaiian\nשלום,Hebrew\nनमस्ते,Hindi\nNyob zoo,Hmong\nSzia,Hungarian\nHalló,Icelandic\nNdewo,Igbo\nHello,Ilocano\nHalo,Indonesian\nCiao,Italian\nこんにちは,Japanese\nСәлеметсіз бе,Kazakh\nសួស្តី,Khmer\nMwaramutse,Kinyarwanda\n안녕하세요,Korean\nSlav,Kurdish\nສະບາຍດີ,Lao\nSalve,Latin\nSveika,Latvian\nSveiki,Lithuanian\nMoien,Luxembourgish\nSalama,Malagasy\nSelamat pagi,Malay\nBongu,Maltese\n你好,Mandarin\nKia ora,Maori\nनमस्कार,Marathi\nсайн уу,Mongolian\nNiltze Tialli Pialli,Nahuatl\nYa’at’eeh,Navajo\nनमस्कार,Nepali\nHei,Norwegian\nسلام,Pashto\nCześć,Polish\nOlá,Portuguese\nਸਤ ਸ੍ਰੀ ਅਕਾਲ,Punjabi\nAkkam,Oromo\nAllianchu,Quechua\nBunâ,Romanian\nПривет,Russian\nTalofa,Samoan\nThobela,Sepedi\nЗдраво,Serbian\nDumela,Sesotho\nAhoj,Slovak\nZdravo,Slovenian\nHello,Somali\nHola,Spanish\nJambo,Swahili\nHallå,Swedish\nKamusta,Tagalog\nIa Orana,Tahitian\nLi-hó,Taiwanese\nவணக்கம்,Tamil\nสวัสดี,Thai\nTashi delek,Tibetan\nMālō e lelei,Tongan\nAvuxeni,Tsonga\nMerhaba,Turkish\nпривіт,Ukrainian\nالسلام عليكم,Urdu\nSalom,Uzbek\nXin chào,Vietnamese\nHelo,Welsh\nMolo,Xhosa\n";
        let mut lines = file_content.lines().collect::<Vec<_>>();

        let _btm = BytesTrieMap::<&str>::new_from_ana(lines.as_mut_slice(), |ss, val, children, _path| {
            // println!("at path {}", std::str::from_utf8(_path).unwrap_or("err"));
            if !ss.is_empty() {
                if ss.len() == 1 {
                    if ss[0] != "" {
                        let det = ss[0].find(',').unwrap_or(usize::MAX);
                        if det == 0 {
                            // println!("setting_singular {}", &ss[0][1..]);
                            *val = Some(&ss[0][1..])
                        } else if det == usize::MAX {
                            // println!("setting_singular_noprefix {}", &ss[0][1..]);
                            *val = Some(&ss[0])
                        } else {
                            // println!("pushing_singular {}", &ss[0][0..det]);
                            let k = ss[0][0..det].as_bytes();
                            ss[0] = &ss[0][det..];
                            children.push(k, &mut ss[0..1]);
                        }
                    }
                } else {
                    ss.sort_by_key(|s| s.chars().next());
                    // println!("chunk {} {}", ss.len(), ss.into_iter().map(|s| s.chars().next().unwrap_or('\'')).collect::<String>());
                    // println!("chunk {:?}", ss);
                    let mut i = 0;
                    let mut j = 1;
                    while ss[i].is_empty() {
                        i += 1;
                        j += 1;
                    }
                    let mut ib = ss[i].chars().next().unwrap();
                    // println!("processing {}", ss[i]);
                    if ib == ',' {
                        // println!("setting0 {}", ss[i]);
                        *val = Some(&ss[i][1..]);
                        ss[i] = "";
                    }
                    for s in unsafe { slice_from_raw_parts_mut(ss[j..].as_ptr().cast_mut(), ss.len() - 1).as_mut().unwrap() }.iter_mut() {
                        let jb = s.chars().next().unwrap();
                        // println!("processing {}", s);
                        if jb == ',' {
                            // println!("setting {}", &s[1..]);
                            *val = Some(&s[1..]);
                            *s = "";
                            j += 1;
                            continue;
                        }
                        if jb != ib {
                            let mut buf = [0u8; 4];
                            let bs = char::encode_utf8(ib, buf.as_mut()).as_bytes();
                            children.push(bs, unsafe { slice_from_raw_parts_mut(ss[i..j].as_ptr().cast_mut(), j - i).as_mut().unwrap() });
                            // println!("pushing {i}..{j} @ {}", std::str::from_utf8(bs).unwrap_or("err"));
                            i = j;
                            ib = jb;
                        }
                        let mut it = s.char_indices(); it.next().unwrap();
                        // println!("old {}, new {}", s, &s[it.next().unwrap().0..]);
                        *s = &s[it.next().unwrap().0..];
                        j += 1;
                    }
                    // currently doesn't push last chunk; whatever
                }
            }
        });

        let mut rz = _btm.read_zipper();
        while let Some(language) = rz.to_next_val() {
            println!("language: {}, greeting: {}", language, std::str::from_utf8(rz.path()).unwrap());
        }
    }

    /// Test the [`ChildBuilder::set_child_mask`] API to set multiple children at once
    #[test]
    fn ana_test3() {
        let map: BytesTrieMap<()> = BytesTrieMap::<()>::new_from_ana(([0u64; 4], 0), |(mut mask, idx), val, children, _path| {
            // println!("path=\"{}\"", String::from_utf8_lossy(_path));
            if idx < 5 {
                mask[1] |= 1u64 << 1+idx;
                let child_vec = vec![(mask, idx+1); idx+1];
                children.set_child_mask(mask , child_vec);
            } else {
                *val = Some(());
            }
        });
        assert_eq!(map.val_count(), 120); // 1 * 2 * 3 * 4 * 5
        // for (path, ()) in map.iter() {
        //     println!("{}", String::from_utf8_lossy(&path));
        // }
    }
    /// Test the [`ChildBuilder::push`] API to set whole string paths
    #[test]
    fn ana_test4() {
        let map: BytesTrieMap<()> = BytesTrieMap::<()>::new_from_ana(3, |idx, val, children, _path| {
            // println!("path=\"{}\"", String::from_utf8_lossy(_path));
            if idx > 0 {
                children.push(b"Left:", idx-1);
                children.push(b"Right:", idx-1);
            } else {
                *val = Some(());
            }
        });
        assert_eq!(map.val_count(), 8);
        assert_eq!(map.get(b"Left:Right:Left:"), Some(&()));
        assert_eq!(map.get(b"Right:Left:Right:"), Some(&()));
        // for (path, ()) in map.iter() {
        //     println!("{}", String::from_utf8_lossy(&path));
        // }

        //Try intermixing whole strings and bytes
        let map: BytesTrieMap<()> = BytesTrieMap::<()>::new_from_ana(7, |idx, val, children, _path| {
            // println!("path=\"{}\"", String::from_utf8_lossy(_path));
            if idx > 0 {
                if idx % 2 == 0 {
                    children.push_byte(b'+', idx-1);
                    children.push_byte(b'-', idx-1);
                } else {
                    children.push(b"Left", idx-1);
                    children.push(b"Right", idx-1);
                }
            } else {
                *val = Some(());
            }
        });
        assert_eq!(map.val_count(), 128);
        assert_eq!(map.get(b"Right-Right+Left-Left"), Some(&()));
        assert_eq!(map.get(b"Left-Right-Right+Left"), Some(&()));
        // for (path, ()) in map.iter() {
        //     println!("{}", String::from_utf8_lossy(&path));
        // }

        //Intermix them in the same child list
        let map: BytesTrieMap<()> = BytesTrieMap::<()>::new_from_ana(7, |idx, val, children, _path| {
            // println!("path=\"{}\"", String::from_utf8_lossy(_path));
            if idx > 0 {
                if idx % 2 == 0 {
                    children.push_byte(b'+', idx-1);
                    children.push(b"Left", idx-1);
                } else {
                    children.push_byte(b'-', idx-1);
                    children.push(b"Right", idx-1);
                }
            } else {
                *val = Some(());
            }
        });
        assert_eq!(map.val_count(), 128);
        assert_eq!(map.get(b"Right+-+-+-"), Some(&()));
        assert_eq!(map.get(b"-+-+-+-"), Some(&()));
        assert_eq!(map.get(b"RightLeftRightLeftRightLeftRight"), Some(&()));
        // for (path, ()) in map.iter() {
        //     println!("{}", String::from_utf8_lossy(&path));
        // }
    }
}
