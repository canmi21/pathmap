//! Functionality for applying various morphisms to [PathMap] and [Zipper](crate::zipper::Zipper)s
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
//! [ZipperIteration::to_next_val](crate::zipper::ZipperIteration::to_next_val) and
//! [ZipperIteration::to_next_step](crate::zipper::ZipperIteration::to_next_step).  The
//! [ZipperIteration](crate::zipper::ZipperIteration) methods visit values first before descending to the
//! branches below, while the `cata` methods call the `mapper` on the deepest values first, before
//! returning to higher levels where `collapse` is called.
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

use reusing_vec::ReusingQueue;

use crate::trie_map::BytesTrieMap;
use crate::trie_node::TrieNodeODRc;
use crate::zipper::*;

pub trait Catamorphism<V> {
    /// Applies a catamorphism to the trie descending from the zipper's root, running the `alg_f` at every
    /// step (at every byte)
    ///
    /// ## Args
    /// - `map_f`: `mapper(v: &V, path: &[u8]) -> W`
    /// Maps value `v` at a leaf `path` into an intermediate result
    ///
    /// - `collapse_f`: `collapse(v: &V, w: W, path: &[u8]) -> W`
    /// Folds value `v` at a non-leaf `path` with the aggregated results from the trie below `path`
    ///
    /// - `alg_f`: `alg(mask: [u64; 4], cs: &mut [W], path: &[u8]) -> W`
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
    /// See [into_cata_side_effect](Catamorphism::into_cata_side_effect) for explanation of other behavior
    fn into_cata_jumping_side_effect<W, MapF, CollapseF, AlgF, JumpF>(self, map_f: MapF, collapse_f: CollapseF, alg_f: AlgF, jump_f: JumpF) -> W
        where
        MapF: FnMut(&V, &[u8]) -> W,
        CollapseF: FnMut(&V, W, &[u8]) -> W,
        AlgF: FnMut(&[u64; 4], &mut [W], &[u8]) -> W,
        JumpF: FnMut(&[u8], W, &[u8]) -> W;
}

impl<Z, V> Catamorphism<V> for Z where Z: Zipper<V> + ZipperAbsolutePath {
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

#[inline]
fn cata_side_effect_body<Z, V, W, MapF, CollapseF, AlgF, JumpF, const JUMPING: bool>(mut z: Z, mut map_f: MapF, mut collapse_f: CollapseF, mut alg_f: AlgF, mut jump_f: JumpF) -> W
    where
    Z: Zipper<V> + ZipperAbsolutePath,
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
        let empty_w = alg_f(&[0; 4], &mut [], z.origin_path().unwrap());
        if let Some(root_val) = z.value() {
            return collapse_f(root_val, empty_w, z.origin_path().unwrap())
        } else {
            return empty_w
        }
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
            let mut cur_w = z.value().map(|v| map_f(v, z.origin_path().unwrap()));

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
                    if let Some(root_val) = z.value() {
                        return collapse_f(root_val, w, z.origin_path().unwrap())
                    } else {
                        return w
                    }
                } else {
                    frame_idx -= 1;

                    //Check to see if we have a value here we need to collapse
                    if let Some(v) = z.value() {
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
    Z: Zipper<V> + ZipperAbsolutePath,
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
            //We don't call the jump_f with a char that's part of the bitmask to the alg_f, so we
            // need to pretend like we're running the `jump_f` one char deeper in the trie than the
            // actual zipper focus
            if old_path_len > new_path_len+1 {
                if let Some(w) = cur_w {
                    let old_origin_path = unsafe{ z.origin_path_assert_len(old_path_len) };
                    let jump_path = &old_origin_path[new_path_len+1..];
                    let origin_path = &old_origin_path[..new_path_len+1];
                    cur_w = Some(jump_f(jump_path, w, origin_path));
                }
            }
        } else {
            //This is the default code to call the `alg_f` for each intermediate path step
            // NOTE: the reason this is a special case rather than just a default JumpF is because I want
            // to be able to re-use the `path_buf`, but that's not possible through the defined interface
            while old_path_len > new_path_len {
                let byte = unsafe{ z.origin_path_assert_len(old_path_len) }.last().unwrap();
                old_path_len -= 1;
                if old_path_len > new_path_len || (!at_fork && !at_root) {
                    if let Some(w) = cur_w {
                        let mut mask = [0u64; 4];
                        let word_idx = (byte / 64) as usize;
                        mask[word_idx] = 1u64 << (byte % 64);
                        cur_w = Some(alg_f(&mask, &mut[w], & unsafe{ z.origin_path_assert_len(old_path_len) }));
                    }
                }
            }
        }

        if at_root {
            return cur_w
        }

        if !at_fork {
            //This unwrap shouldn't panic, because `ascend_until` should only stop at values and forks
            let v = z.value().unwrap();
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

#[macro_export]
macro_rules! cata {
    ($V:ty, $W:ty, $z:expr, $map_f:expr, $collapse_f:expr, $alg_f:expr, $jump_f:expr) => {{
     use crate::zipper::zipper_priv::ZipperMovingPriv;
    let mut z = $z;
    //`stack` holds a "frame" at each forking point above the zipper position.  No frames exist for values
    let mut stack = Vec::<crate::morphisms::StackFrame<$W>>::with_capacity(12);
    let mut frame_idx = 0;

    z.reset();
    z.prepare_buffers();

    //Push a stack frame for the root, and start on the first branch off the root
    stack.push(crate::morphisms::StackFrame::new(z.child_count(), z.child_mask()));
    if !z.descend_first_byte() {
        //Empty trie is a special case
        let empty_w = $alg_f(&[0; 4], &mut [], z.origin_path().unwrap());
        if let Some(root_val) = z.value() {
            $collapse_f(root_val, empty_w, z.origin_path().unwrap())
        } else {
            empty_w
        }
    } else {
    'outer: loop {
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
            let mut cur_w = z.value().map(|v| $map_f(v, z.origin_path().unwrap()));

            //Ascend back to the last fork point
            // ascend_to_fork::<Z, V, W, MapF, CollapseF, AlgF, JumpF, JUMPING>(&mut z, &mut map_f, &mut collapse_f, &mut alg_f, &mut jump_f, cur_w);
            loop {
                let mut old_path_len = z.origin_path().unwrap().len();

                let ascended = z.ascend_until();
                debug_assert!(ascended);

                let at_fork = z.child_count() > 1;
                let at_root = z.at_root();

                let new_path_len = z.origin_path().unwrap().len();

                if true {
                    //We don't call the jump_f with a char that's part of the bitmask to the alg_f, so we
                    // need to pretend like we're running the `jump_f` one char deeper in the trie than the
                    // actual zipper focus
                    if old_path_len > new_path_len+1 {
                        if let Some(w) = cur_w {
                            let old_origin_path = unsafe{ z.origin_path_assert_len(old_path_len) };
                            let jump_path = &old_origin_path[new_path_len+1..];
                            let origin_path = &old_origin_path[..new_path_len+1];
                            cur_w = Some($jump_f(jump_path, w, origin_path));
                        }
                    }
                } else {
                    //This is the default code to call the `alg_f` for each intermediate path step
                    // NOTE: the reason this is a special case rather than just a default JumpF is because I want
                    // to be able to re-use the `path_buf`, but that's not possible through the defined interface
                    while old_path_len > new_path_len {
                        let byte = unsafe{ z.origin_path_assert_len(old_path_len) }.last().unwrap();
                        old_path_len -= 1;
                        if old_path_len > new_path_len || (!at_fork && !at_root) {
                            if let Some(w) = cur_w {
                                let mut mask = [0u64; 4];
                                let word_idx = (byte / 64) as usize;
                                mask[word_idx] = 1u64 << (byte % 64);
                                cur_w = Some($alg_f(&mask, &mut[w], & unsafe{ z.origin_path_assert_len(old_path_len) }));
                            }
                        }
                    }
                }

                if at_root {
                    break;
                }

                if !at_fork {
                    //This unwrap shouldn't panic, because `ascend_until` should only stop at values and forks
                    let v = z.value().unwrap();
                    match cur_w {
                        None => { cur_w = Some($map_f(v, z.origin_path().unwrap())); }
                        Some(w) => { cur_w = Some($collapse_f(v, w, z.origin_path().unwrap())); }
                    }
                } else {
                    break;
                }
            }

            //Merge the result into the stack frame
            match cur_w {
                Some(w) => stack[frame_idx].push_val(w),
                None => stack[frame_idx].push_none(),
            }

            debug_assert!(stack[frame_idx].child_idx <= z.child_count());
            while stack[frame_idx].child_idx == z.child_count() {

                //We finished all the children from this branch, so run the aggregate function
                let stack_frame = &mut stack[frame_idx];
                let mut w = $alg_f(&stack_frame.child_mask, &mut stack_frame.children, z.origin_path().unwrap());
                if frame_idx == 0 {
                    if let Some(root_val) = z.value() {
                        break 'outer $collapse_f(root_val, w, z.origin_path().unwrap());
                    } else {
                        break 'outer w;
                    }
                } else {
                    frame_idx -= 1;

                    //Check to see if we have a value here we need to collapse
                    if let Some(v) = z.value() {
                        w = $collapse_f(v, w, z.origin_path().unwrap());
                    }

                    //Ascend the rest of the way back up to the branch
                    // w = ascend_to_fork::<Z, V, W, MapF, CollapseF, AlgF, JumpF, JUMPING>(&mut z, &mut map_f, &mut collapse_f, &mut alg_f, &mut jump_f, Some(w)).unwrap();
                    let mut cur_w = Some(w);
                    loop {
                        let mut old_path_len = z.origin_path().unwrap().len();

                        let ascended = z.ascend_until();
                        debug_assert!(ascended);

                        let at_fork = z.child_count() > 1;
                        let at_root = z.at_root();

                        let new_path_len = z.origin_path().unwrap().len();

                        if true {
                            //We don't call the jump_f with a char that's part of the bitmask to the alg_f, so we
                            // need to pretend like we're running the `jump_f` one char deeper in the trie than the
                            // actual zipper focus
                            if old_path_len > new_path_len+1 {
                                if let Some(w) = cur_w {
                                    let old_origin_path = unsafe{ z.origin_path_assert_len(old_path_len) };
                                    let jump_path = &old_origin_path[new_path_len+1..];
                                    let origin_path = &old_origin_path[..new_path_len+1];
                                    cur_w = Some($jump_f(jump_path, w, origin_path));
                                }
                            }
                        } else {
                            //This is the default code to call the `alg_f` for each intermediate path step
                            // NOTE: the reason this is a special case rather than just a default JumpF is because I want
                            // to be able to re-use the `path_buf`, but that's not possible through the defined interface
                            while old_path_len > new_path_len {
                                let byte = unsafe{ z.origin_path_assert_len(old_path_len) }.last().unwrap();
                                old_path_len -= 1;
                                if old_path_len > new_path_len || (!at_fork && !at_root) {
                                    if let Some(w) = cur_w {
                                        let mut mask = [0u64; 4];
                                        let word_idx = (byte / 64) as usize;
                                        mask[word_idx] = 1u64 << (byte % 64);
                                        cur_w = Some($alg_f(&mask, &mut[w], & unsafe{ z.origin_path_assert_len(old_path_len) }));
                                    }
                                }
                            }
                        }

                        if at_root {
                            break;
                        }

                        if !at_fork {
                            //This unwrap shouldn't panic, because `ascend_until` should only stop at values and forks
                            let v = z.value().unwrap();
                            match cur_w {
                                None => { cur_w = Some($map_f(v, z.origin_path().unwrap())); }
                                Some(w) => { cur_w = Some($collapse_f(v, w, z.origin_path().unwrap())); }
                            }
                        } else {
                            break;
                        }
                    }
                    w = cur_w.unwrap();

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
                stack.push(crate::morphisms::StackFrame::new(child_cnt, child_mask));
                debug_assert!(stack.len() > frame_idx);
            } else {
                stack[frame_idx].reset(child_mask);
            }

            //Descend the first child branch
            z.descend_first_byte();
        }
    }
    }
    }};
}


/// Internal structure to hold temporary info used inside morphism apply methods
pub struct StackFrame<W> {
    pub child_mask: [u64; 4],
    pub mask_word_idx: usize,
    pub child_idx: usize,
    pub remaining_children_mask: u64,
    pub children: Vec<W>,
}

impl<W> StackFrame<W> {
    /// Allocates a new StackFrame
    pub fn new(child_count: usize, child_mask: [u64; 4]) -> Self {
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
    pub fn reset(&mut self, child_mask: [u64; 4]) {
        self.mask_word_idx = 0;
        self.child_idx = 0;
        while child_mask[self.mask_word_idx] == 0 && self.mask_word_idx < 3 {
            self.mask_word_idx += 1;
        }
        self.remaining_children_mask = child_mask[self.mask_word_idx];
        self.child_mask = child_mask;
        self.children.clear();
    }
    pub fn push_val(&mut self, w: W) {
        self.children.push(w);
        self.child_idx += 1;
        debug_assert!(self.remaining_children_mask > 0); //If this assert trips, then it means we're trying to push a value for a non-existent child
        let index = self.remaining_children_mask.trailing_zeros();
        self.remaining_children_mask ^= 1u64 << index;
        self.advance_word_idx();
    }
    pub fn push_none(&mut self) {
        self.child_idx += 1;
        debug_assert!(self.remaining_children_mask > 0); //If this assert trips, then it means we're trying to push a value for a non-existent child
        let index = self.remaining_children_mask.trailing_zeros();
        self.remaining_children_mask ^= 1u64 << index;
        self.child_mask[self.mask_word_idx] ^= 1u64 << index;
        self.advance_word_idx();
    }
    pub fn advance_word_idx(&mut self) {
        if self.remaining_children_mask == 0 {
            if self.mask_word_idx < 3 {
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
    AlgF: FnMut(W, &mut Option<V>, &mut TrieBuilder<V, W>, &[u8])
{
    let mut stack = Vec::<(TrieBuilder<V, W>, usize)>::with_capacity(12);
    let mut frame_idx = 0;

    let mut new_map = BytesTrieMap::new();
    let mut z = new_map.write_zipper();
    let mut val = None;

    //The root is a special case
    stack.push((TrieBuilder::<V, W>::new(), 0));
    alg_f(w, &mut val, &mut stack[frame_idx].0, z.path());
    stack[frame_idx].0.finalize();
    if let Some(val) = core::mem::take(&mut val) {
        z.set_value(val);
    }
    loop {
        //Should we descend?
        if let Some(w_or_node) = stack[frame_idx].0.take_next() {
            //TODO Optimization Opportunity: There is likely a 2x speedup in here, that can be achieved by
            // setting all the children at the same time.  The reason is that the current behavior will create
            // a smaller node (ListNode), and then upgrade it if necessary.  But if we know in advance what
            // the node needs to hold, we could allocate the correct node right off the bat.
            //
            // I'm going to hold off on implementing this until the explicit path API is fully settled,
            // since ideally we'd call a WriteZipper method to set multiple downstream paths, but we'd want
            // some assurances those paths would actually be created, and not pruned because they're empty.
            //
            //I'm thinking this needs to take the form of a TrieNode trait method that looks something like:
            // `fn set_children_and_values(child_mask, val_mask, branches: &[??], vals: &[V])` or possibly
            // a node creation method.  Since this is designed to be used by anamorphism (trie construction)
            // perhaps we don't actually need the downstream nodes at all, and can fill them in later.
            //
            //BEWARE: This change needs to be accompanied by another change to allow an existing node's path
            // to be augmented, or the above change could be a performance step backwards.  Specifically,
            // consider a ListNode that is created with only one child, based on a mask.  Then, if the zipper
            // descends and tries to create the rest of a path, it would be a disaster if that took the form
            // of another node, as opposed to just putting the rest of the path into the existing node.

            let child_path_byte = stack[frame_idx].0.taken_child_byte();
            z.descend_to_byte(child_path_byte);
            let mut child_path_len = 1;

            if let Some(child_path_remains) = stack[frame_idx].0.taken_child_remaining_path(child_path_byte) {
                z.descend_to(child_path_remains);
                child_path_len += child_path_remains.len();
            }

            match w_or_node {
                // Recursive path with more Ws
                WOrNode::W(w) => {
                    debug_assert!(frame_idx < stack.len());
                    frame_idx += 1;
                    if frame_idx == stack.len() {
                        stack.push((TrieBuilder::<V, W>::new(), child_path_len));
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
                },
                // Path from a graft, we shouldn't descend
                WOrNode::Node(node) => {
                    z.core().graft_internal(Some(node));
                    z.ascend(child_path_len);
                }
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
    drop(z);
    new_map
}

/// A [Vec]-like struct for assembling all the downstream branches from a path in the trie
//GOAT, Ideally I would skip the `val` argument to the anamorphism closure, and add a `set_val` method
// to TrieBuilder.  I'm a little on the fence about it, however, because it increases the size of the
// TrieBuilder by an `Option<V>`, which is stored for every stack frame, as opposed to just the one place
// it's needed.  However, this change opens up the possibility to embed the WriteZipper into the TrieBuilder,
// which can make some operations a bit more efficient.
//
//GOAT, If we exposed an interface that allowed values to be set in bulk, (e.g. with a mask), we could
// plumb it straight through to a node interface
pub struct TrieBuilder<V, W> {
    child_mask: [u64; 4],
    cur_mask_word: usize,
    child_paths: ReusingQueue<Vec<u8>>,
    child_structs: ReusingQueue<WOrNode<V, W>>,
}

/// Internal structure 
enum WOrNode<V, W> {
    W(W),
    Node(TrieNodeODRc<V>)
}

impl<V, W: Default> Default for WOrNode<V, W> {
    fn default() -> Self {
        //GOAT, the default impl here is mainly to facilitate core::mem::take, therefore, the default
        // should be the cheapest thing to create.  At some point that will be a TrieNodeODRc pointing
        // at a static empty node, but that's currently not implemented yet.
        // Alternatively we could use a MaybeUninit.
        Self::W(W::default())
    }
}

impl<V: Clone + Send + Sync, W: Default> TrieBuilder<V, W> {
    /// Internal method to make a new empty `TrieBuilder`
    fn new() -> Self {
        Self {
            child_mask: [0u64; 4],
            cur_mask_word: 0,
            child_paths: ReusingQueue::new(),
            child_structs: ReusingQueue::new() }
    }
    /// Internal method.  Clears a builder without freeing its memory
    fn reset(&mut self) {
        self.child_mask = [0u64; 4];
        self.cur_mask_word = 0;
        self.child_structs.clear();
        self.child_paths.clear();
    }
    /// Internal method.  Called after the user code has run to fill the builder, but before we start to empty it
    fn finalize(&mut self) {
        self.cur_mask_word = 0;
        while self.cur_mask_word < 4 && self.child_mask[self.cur_mask_word] == 0 {
            self.cur_mask_word += 1;
        }
    }
    /// Internal method to get the next child from the builder in the push order.  Used by the anamorphism
    fn take_next(&mut self) -> Option<WOrNode<V, W>> {
        self.child_structs.pop_front().map(|element| core::mem::take(element))
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
    fn taken_child_remaining_path(&mut self, byte: u8) -> Option<&[u8]> {
        if self.child_paths.get(0).map(|path| path[0]) != Some(byte) {
            None
        } else {
            self.child_paths.pop_front().map(|v| &v.as_slice()[1..])
        }
    }
    /// Returns the number of children that have been pushed to the `TrieBuilder`, so far
    pub fn len(&self) -> usize {
        self.child_structs.len()
    }
    /// Simultaneously sets all child branches with single-byte path continuations
    ///
    /// Panics if existing children have already been set / pushed, or if the number of bits set in `mask`
    /// doesn't match `children.len()`.
    pub fn set_child_mask<C: AsMut<[W]>>(&mut self, mask: [u64; 4], mut children: C) {
        if self.child_structs.len() != 0 {
            panic!("set_mask called over existing children")
        }
        let children = children.as_mut();
        debug_assert_eq!(mask.iter().fold(0, |sum, word| sum + word.count_ones() as usize), children.len());
        if children.len() == 0 {
            return
        }
        self.child_structs.clear();
        for child in children {
            self.child_structs.push_val(WOrNode::W(core::mem::take(child)));
        }
        debug_assert_eq!(self.cur_mask_word, 0);
        while mask[self.cur_mask_word] == 0 {
            self.cur_mask_word += 1;
        }
        self.child_mask = mask;
    }
    /// Pushes a new child branch into the `TrieBuilder` with the specified `byte`
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
            panic!("children must be pushed in sorted order and each initial byte must be unique")
        }
        self.child_mask[mask_word] |= mask_delta;

        //Push the `W`
        self.child_structs.push_val(WOrNode::W(w));
    }
    /// Pushes a new child branch into the `TrieBuilder` with the specified `sub_path`
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
            let child_path = self.child_paths.push_mut();
            child_path.clear();
            child_path.extend(sub_path);
        }

        self.push_byte(sub_path[0], w);
    }
    /// Behaves like [push](Self::push), but will tolerate inputs in any order, and inputs and with
    /// overlapping initial bytes
    ///
    /// DISCUSSION: This method is handy when you are generating paths composed of data types that can't be
    /// cleanly separated at byte boundaries; for example UTF-8 encoded `char`s.  This method saves you the
    /// extra work of handling the case where different structures encode with the same initial byte, and of
    /// concerning yourself with partial encoding generally.
    ///
    /// This method is much higher overhead than the ordinary `push` method, and also it introduces
    /// some ambiguity in the order in which the closure is run for the children.  Specifically it means that
    /// the same path location in the trie may be visited multiple times, and you cannot rely on closure
    /// execution proceeding in a strictly depth-first order.  Furthermore, the closure order may not match
    /// the traversal order of the completed trie.
    ///
    /// NOTE: Because a given location may be visited multiple times, values set by later-running closures
    /// will overwrite a value set by an earlier closure running at the same path.
    ///
    /// NOTE: If you push twice to an identical path within the same closure execution, the second push will
    /// overwrite the first.
    ///
    /// NOTE: use of this method will preclude any automatic multi-threading of the anamorphism on downstream
    /// paths.
    pub fn tolerant_push(&mut self, sub_path: &[u8], w: W) {
        let byte = match sub_path.get(0) {
            Some(byte) => byte,
            None => return
        };

        //Find the index in the `child_structs` vec based on the initial byte
        let mask_word = (byte / 64) as usize;
        let byte_remainder = byte % 64;
        let mut byte_index = 0;
        for i in 0..mask_word {
            byte_index += self.child_mask[i].count_ones();
        }
        if byte_remainder > 0 {
            byte_index += (self.child_mask[mask_word] & 0xFFFFFFFFFFFFFFFF >> (64-byte_remainder)).count_ones();
        }

        let mask_delta = 1u64 << (byte % 64);
        let collision = self.child_mask[mask_word] & mask_delta > 0;
        self.child_mask[mask_word] |= mask_delta;

//GOAT, my thinking on the data structure changes
//For the paths, we should go back to storing the whole path, and use the first byte for association.  No need
// to keep the index association
//For the W array, we ought to just push the Ws in, in the order we want.  Since we always iterate the W vec
//If we want to support direct-push of values, we ought to have a separate value mask
//There should be two value vecs.  One for direct values of the current node, and one for values associated
// with downstream children / paths.  (we could even piggy-back the downstream value on the path)

//GOAT, THE W vec should have the string pairs hanging off each element.  So the W vec is `Vec<(W, Vec<(Vec<u8>, W)>)>`
//GOAT, Actually the W needs to be an Option<W>, and at that point, we may as well split the 

//GOAT options:
// 1. Make one vec that holds length-1 children, and another that holds lenth > 1,
//     

//GOAT!!! Vec<(Option<W>, Vec<(Vec<u8>, W)>)>
//GOAT!!! ReusingQueue<SmallVec<(Vec<u8>, W)>>

        //GOAT, we need to reset self.cur_mask_word, scanning the whole child_mask, because any child byte may have been added
        // self.cur_mask_word = mask_word;
    }
    /// Returns the child mask from the `TrieBuilder`, representing paths that have been pushed so far
    pub fn child_mask(&self) -> [u64; 4] {
        self.child_mask
    }
    /// Grafts the subtrie below the focus of the `read_zipper` at the specified `byte`
    ///
    /// WARNING: This method is incompatible with [Self::set_child_mask] and must follow the same
    /// rules as [Self::push_byte]
    pub fn graft_at_byte<Z: Zipper<V>>(&mut self, byte: u8, read_zipper: &Z) {
        let mask_word = (byte / 64) as usize;
        if mask_word < self.cur_mask_word {
            panic!("children must be pushed in sorted order")
        }
        self.cur_mask_word = mask_word;
        let mask_delta = 1u64 << (byte % 64);
        if self.child_mask[mask_word] >= mask_delta {
            panic!("children must be pushed in sorted order and each initial byte must be unique")
        }
        self.child_mask[mask_word] |= mask_delta;

        //Clone the read_zipper's focus and push it
        let node = read_zipper.get_focus().into_option();
        self.child_structs.push_val(WOrNode::Node(node.unwrap())); //GOAT!! Currently we panic if the read_zipper is at an nonexistent path
    }
    //GOAT, feature removed.  See below
    // /// Returns an [`Iterator`] type to iterate over the `(sub_path, w)` pairs that have been pushed
    // pub fn iter(&self) -> TrieBuilderIter<'_, W> {
    //     self.into_iter()
    // }
}

//GOAT, the IntoIterator impl is obnoxious because I don't have a contiguous buffer that holds the path anymore
// It's unnecessary anyway, so I'm just going to chuck it
//
// impl<'a, W> IntoIterator for &'a TrieBuilder<W> {
//     type Item = (&'a[u8], &'a W);
//     type IntoIter = TrieBuilderIter<'a, W>;

//     fn into_iter(self) -> Self::IntoIter {
//         TrieBuilderIter {
//             cb: self,
//             cur_mask: self.child_mask[0],
//             mask_word: 0,
//             i: 0
//         }
//     }
// }

// /// An [`Iterator`] type for a [`TrieBuilder`]
// pub struct TrieBuilderIter<'a, W> {
//     cb: &'a TrieBuilder<W>,
//     cur_mask: u64,
//     mask_word: usize,
//     i: usize,
// }

// impl<'a, W> Iterator for TrieBuilderIter<'a, W> {
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
    use std::ops::Range;
    use crate::trie_map::BytesTrieMap;
    use crate::utils::BitMask;
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

    #[test]
    fn cata_test4() {
        #[derive(Debug, PartialEq)]
        enum Trie<V> {
            Value(V),
            Collapse(V, Box<Trie<V>>),
            Alg(Vec<(char, Trie<V>)>),
            Jump(String, Box<Trie<V>>)
        }
        use Trie::*;

        let mut btm = BytesTrieMap::new();
        let rs = ["arr", "arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });

        let s = btm.read_zipper().into_cata_jumping_side_effect(
            |v, _path| { Some(Box::new(Value(*v))) },
            |v, w, _path| { Some(Box::new(Collapse(*v, w.unwrap()))) },
            |cm, ws, _path| {
                let mut it = crate::utils::ByteMaskIter::new(cm.clone());
                Some(Box::new(Alg(ws.iter_mut().map(|w| (it.next().unwrap() as char, *std::mem::take(w).unwrap())).collect())))},
            |sp, w, _path| { Some(Box::new(Jump(std::str::from_utf8(sp).unwrap().to_string(), w.unwrap()))) }
        );

        assert_eq!(s, Some(Alg([
            ('a', Jump("rr".into(), Collapse(0, Jump("w".into(), Value(1).into()).into()).into())),
            ('b', Jump("ow".into(), Value(2).into())),
            ('c', Jump("annon".into(), Value(3).into())),
            ('r', Alg([
                ('o', Jump("m".into(), Alg([
                    ('\'', Jump("i".into(), Value(12).into())),
                    ('a', Jump("n".into(), Collapse(4, Alg([
                        ('e', Value(5)),
                        ('u', Jump("s".into(), Value(6).into()))
                    ].into()).into()).into())),
                    ('u', Jump("lus".into(), Value(7).into()))].into()).into())),
                ('u', Jump("b".into(), Alg([
                    ('e', Alg([
                        ('n', Jump("s".into(), Value(8).into())),
                        ('r', Value(9))].into())),
                    ('i', Jump("c".into(), Alg([
                        ('o', Jump("n".into(), Value(10).into())),
                        ('u', Jump("ndus".into(), Value(11).into()))].into()).into()))].into()).into()))].into()))].into()).into()));
    }

    #[test]
    fn cata_test5() {
        let empty = BytesTrieMap::<u64>::new();
        println!("{:?}", empty.into_cata_side_effect(|_, _| 1, |_, _, _| 2, |_, _, _| 3));

        let mut nonempty = BytesTrieMap::<u64>::new();
        nonempty.insert(&[1, 2, 3], !0);
        println!("{:?}", nonempty.into_cata_side_effect(|_, _| 1, |_, _, _| 2, |_, _, _| 3));
    }

    #[test]
    fn cata_test6() {
        let mut btm = BytesTrieMap::new();
        let rs = ["Hello, my name is", "Helsinki", "Hell"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });

        let mut map_cnt = 0;
        let mut collapse_cnt = 0;
        let mut alg_cnt = 0;
        let mut jump_cnt = 0;

        btm.read_zipper().into_cata_jumping_side_effect(
            |_, _path| {
                // println!("map: \"{}\"", String::from_utf8_lossy(_path));
                map_cnt += 1;
            },
            |_, _, _path| {
                // println!("collapse: \"{}\"", String::from_utf8_lossy(_path));
                collapse_cnt += 1;
            },
            |_, _, _path| {
                // println!("alg: \"{}\"", String::from_utf8_lossy(_path));
                alg_cnt += 1;
            },
            |_sub_path, _, _path| {
                // println!("jump: over \"{}\" to \"{}\"", String::from_utf8_lossy(_sub_path), String::from_utf8_lossy(_path));
                jump_cnt += 1;
            }
        );
        // println!("map_cnt={map_cnt}, collapse_cnt={collapse_cnt}, alg_cnt={alg_cnt}, jump_cnt={jump_cnt}");

        assert_eq!(map_cnt, 2);
        assert_eq!(collapse_cnt, 1);
        assert_eq!(alg_cnt, 2);
        assert_eq!(jump_cnt, 3);
    }

    /// Covers the full spectrum of byte values
    #[test]
    fn cata_test7() {
        let mut btm = BytesTrieMap::new();
        let rs = [[0, 0, 0, 0], [0, 255, 170, 170], [0, 255, 255, 255], [0, 255, 88, 88]];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r, i); });

        let mut map_cnt = 0;
        let mut collapse_cnt = 0;
        let mut alg_cnt = 0;
        let mut jump_cnt = 0;

        btm.read_zipper().into_cata_jumping_side_effect(
            |_, _path| {
                // println!("map: {_path:?}");
                map_cnt += 1;
            },
            |_, _, _path| {
                // println!("collapse: {_path:?}");
                collapse_cnt += 1;
            },
            |_, _, _path| {
                // println!("alg: {_path:?}");
                alg_cnt += 1;
            },
            |_sub_path, _, _path| {
                // println!("jump: over {_sub_path:?} to {_path:?}");
                jump_cnt += 1;
            }
        );
        // println!("map_cnt={map_cnt}, collapse_cnt={collapse_cnt}, alg_cnt={alg_cnt}, jump_cnt={jump_cnt}");

        assert_eq!(map_cnt, 4);
        assert_eq!(collapse_cnt, 0);
        assert_eq!(alg_cnt, 3);
        assert_eq!(jump_cnt, 4);
    }

    /// Tests that cata hits the root value
    #[test]
    fn cata_test8() {
        let keys = ["", "ab", "abc"];
        let btm: BytesTrieMap<usize> = keys.into_iter().enumerate().map(|(i, k)| (k, i)).collect();

        btm.into_cata_jumping_side_effect(
            |v, path| {
                // println!("map: {path:?}");
                assert_eq!(path, &[97, 98, 99]);
                assert_eq!(*v, 2);
            },
            |v, _, path| {
                // println!("collapse: {path:?}");
                match *v {
                    1 => assert_eq!(path, &[97, 98]),
                    0 => assert_eq!(path, &[]),
                    _ => unreachable!(),
                }
            },
            |_mask, _, path| {
                // println!("alg: {path:?}");
                assert_eq!(path, &[]);
            },
            |sub_path, _, path| {
                // println!("jump: over {sub_path:?} to {path:?}");
                assert_eq!(sub_path, &[98]);
                assert_eq!(path, &[97]);
            }
        )
    }

    /// Generate some basic tries using the [TrieBuilder::push_byte] API
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

    /// Test the [`TrieBuilder::set_child_mask`] API to set multiple children at once
    #[test]
    fn ana_test2() {
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
    /// Test the [`TrieBuilder::push`] API to set whole string paths
    #[test]
    fn ana_test3() {
        let map: BytesTrieMap<()> = BytesTrieMap::<()>::new_from_ana(3, |idx, val, children, _path| {
            // println!("path=\"{}\"", String::from_utf8_lossy(_path));
            if idx > 0 {
                children.push(b"Left:", idx-1);
                children.push(b"Right:", idx-1);
            } else {
                *val = Some(());
            }
        });
        // for (path, ()) in map.iter() {
        //     println!("{}", String::from_utf8_lossy(&path));
        // }
        assert_eq!(map.val_count(), 8);
        assert_eq!(map.get(b"Left:Right:Left:"), Some(&()));
        assert_eq!(map.get(b"Right:Left:Right:"), Some(&()));

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
        // for (path, ()) in map.iter() {
        //     println!("{}", String::from_utf8_lossy(&path));
        // }
        assert_eq!(map.val_count(), 128);
        assert_eq!(map.get(b"Right-Right+Left-Left"), Some(&()));
        assert_eq!(map.get(b"Left-Right-Right+Left"), Some(&()));

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
        // for (path, ()) in map.iter() {
        //     println!("{}", String::from_utf8_lossy(&path));
        // }
        assert_eq!(map.val_count(), 128);
        assert_eq!(map.get(b"Right+-+-+-"), Some(&()));
        assert_eq!(map.get(b"-+-+-+-"), Some(&()));
        assert_eq!(map.get(b"RightLeftRightLeftRightLeftRight"), Some(&()));
    }

    const GREETINGS: &[&str] = &["Hallo,Afrikaans", "Përshëndetje,Albanian", "እው ሰላም ነው,Amharic", "مرحبًا,Arabic",
        "Barev,Armenian", "Kamisaki,Aymara", "Salam,Azerbaijani", "Kaixo,Basque", "Вітаю,Belarusian", "হ্যালো,Bengali",
        "Zdravo,Bosnian", "Здравейте,Bulgarian", "ဟယ်လို,Burmese", "你好,Cantonese", "Hola,Catalan", "Kamusta,Cebuano",
        "Kamusta,Cebuano", "Moni,Chichewa", "Bonghjornu,Corsican", "Zdravo,Croatian", "Ahoj,Czech", "Hej,Danish",
        "Hallo,Dutch", "Hello,English", "Tere,Estonian", "Hello,Ewe", "سلام,Farsi (Persian)", "Bula,Fijian",
        "Kumusta,Filipino", "Hei,Finnish", "Bonjour,French", "Dia dhuit,Gaelic (Irish)", "Ola,Galician", "გამარჯობა,Georgian",
        "Guten tag,German", "γεια,Greek", "Mba'éichapa,Guarani", "Bonjou,Haitian Creole", "Aloha,Hawaiian",
        "שלום,Hebrew", "नमस्ते,Hindi", "Nyob zoo,Hmong", "Szia,Hungarian", "Halló,Icelandic", "Ndewo,Igbo",
        "TRASH-NO-COMMA", //Trash here, to test error-cases
        "Hello,Ilocano", "Halo,Indonesian", "Ciao,Italian", "こんにちは,Japanese", "Сәлеметсіз бе,Kazakh",
        "TRASH-NOTHING-AFTER-COMMA,", //Trash here, to test error-cases
        "សួស្តី,Khmer", "Mwaramutse,Kinyarwanda", "안녕하세요,Korean", "Slav,Kurdish", "ສະບາຍດີ,Lao", "Salve,Latin",
        ",TRASH-NOTHING-BEFORE-COMMA", //Trash here, to test error-cases
        "Sveika,Latvian", "Sveiki,Lithuanian", "Moien,Luxembourgish", "Salama,Malagasy", "Selamat pagi,Malay",
        "", //Trash here, empty string
        "Bongu,Maltese", "你好,Mandarin", "Kia ora,Maori", "नमस्कार,Marathi", "сайн уу,Mongolian", "Niltze Tialli Pialli,Nahuatl",
        "Ya’at’eeh,Navajo", "नमस्कार,Nepali", "Hei,Norwegian", "سلام,Pashto", "Cześć,Polish", "Olá,Portuguese",
        "ਸਤ ਸ੍ਰੀ ਅਕਾਲ,Punjabi", "Akkam,Oromo", "Allianchu,Quechua", "Bunâ,Romanian", "Привет,Russian", "Talofa,Samoan",
        "Thobela,Sepedi", "Здраво,Serbian", "Dumela,Sesotho", "Ahoj,Slovak", "Zdravo,Slovenian", "Hello,Somali",
        "Hola,Spanish", "Jambo,Swahili", "Hallå,Swedish", "Kamusta,Tagalog", "Ia Orana,Tahitian", "Li-hó,Taiwanese",
        "வணக்கம்,Tamil", "สวัสดี,Thai", "Tashi delek,Tibetan", "Mālō e lelei,Tongan", "Avuxeni,Tsonga", "Merhaba,Turkish",
        "привіт,Ukrainian", "السلام عليكم,Urdu", "Salom,Uzbek", "Xin chào,Vietnamese", "Helo,Welsh", "Molo,Xhosa",
    ];

    /// Test pushing into the trie, one byte at a time
    #[test]
    fn ana_test4() {
        let mut greetings_vec = GREETINGS.to_vec();
        let btm = BytesTrieMap::<Range<usize>>::new_from_ana(0..greetings_vec.len(), |mut range, val, children, path| {
            let n = path.len();

            //Sort the keys in the range by the first byte of each substring
            let string_slice = &mut greetings_vec[range.clone()];
            string_slice.sort_by_key(|s| s.as_bytes().get(n));

            //Discard the strings that are too short (that have ended prematurely)
            while range.len() > 0 && greetings_vec[range.start].len() <= n { range.start += 1; }

            while range.len() > 0 {
                //Find the range of strings that start with the same byte as the first string
                let mut m = range.start + 1;
                let byte = greetings_vec[range.start].as_bytes()[n];
                while range.contains(&m) && greetings_vec[m].as_bytes()[n] == byte {
                    m += 1;
                }

                let (mut same_prefix_range, remaining) = (range.start..m, m..range.end);

                //If this is the end of a path, set the value
                if byte == b',' {

                    //Sort by the languages
                    //NOTE: there is some ambiguity in the desired behavior, since the validity test
                    // sorts the greetings but not the languages.  However, if we don't sort the languages,
                    // then non-contiguous and non-prefixing empty language strings can't be removed unless
                    // we attempt to represent the results of a node by a list instead of a range
                    let string_slice = &mut greetings_vec[same_prefix_range.clone()];
                    string_slice.sort_by_key(|s| &s[n+1..]);

                    //Discard the strings have nothing after the ','
                    while same_prefix_range.len() > 0 && greetings_vec[same_prefix_range.start].len() <= n+1 {
                        same_prefix_range.start += 1;
                    }
                    //If we didn't discard all the items
                    if same_prefix_range.len() > 0 {
                        *val = Some(same_prefix_range);
                    }
                } else {
                    //Recursive case
                    children.push_byte(byte, same_prefix_range);
                }
                range = remaining;
            }
        });

        let mut check: Vec<&str> = GREETINGS.into_iter().copied()
            .filter(|x| {
                let comma_idx = x.find(",").unwrap_or(0);
                comma_idx != 0 && comma_idx < x.len()-1
            })
            .collect();
        check.sort_by_key(|x| x.split_once(",").map(|s| s.0).unwrap_or(&""));
        let mut it = check.iter();

        let mut rz = btm.read_zipper();
        while let Some(range) = rz.to_next_val() {
            for language_idx in range.clone().into_iter() {
                let greeting = std::str::from_utf8(rz.path()).unwrap();
                let language = &greetings_vec[language_idx][rz.path().len()+1..];

                // println!("language: {}, greeting: {}", language, greeting);
                assert_eq!(*it.next().unwrap(), format!("{greeting},{language}"));
            }
        }
    }

    #[test]
    fn ana_test5() {
        let _btm = BytesTrieMap::<&str>::new_from_ana(GREETINGS, |string_slice, val, children, _path| {

            fn split_key(in_str: &str) -> (&str, &str) {
                let det = in_str.find(',').unwrap_or(usize::MAX);
                if det == 0 {
                    ("", &in_str[1..])
                } else if det == usize::MAX {
                    ("", in_str)
                } else {
                    (&in_str[0..det], &in_str[det+1..])
                }
            }

            if string_slice.len() == 1 {
                let (_, split_val) = split_key(string_slice[0]);
                *val = Some(split_val);
            } else {
                for i in 0..string_slice.len() {
                    let (key, _) = split_key(string_slice[0]);
                    children.tolerant_push(key.as_bytes(), &string_slice[i..i+1]);
                }
            }
        });

        let mut rz = _btm.read_zipper();
        while let Some(language) = rz.to_next_val() {
            //GOAT, this feature (and therefore this test) is WIP
            println!("language: {}, greeting: {}", language, std::str::from_utf8(rz.path()).unwrap());
        }
    }

    #[test]
    fn apo_test1() {
        let mut btm = BytesTrieMap::new();
        let rs = ["arro^w", "bow", "cann^on", "roman", "romane", "romanus^", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom^i"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });

        let mut alphabetic = [0u64; 4];
        for c in "abcdefghijklmnopqrstuvwxyz".bytes() { alphabetic.set_bit(c) }

        let trie_ref = btm.trie_ref_at_path([]);
        let counted = BytesTrieMap::new_from_ana(trie_ref, |trie_ref, _v, builder, loc| {

            let iter = crate::utils::ByteMaskIter::new(trie_ref.child_mask());
            for b in iter {
                if alphabetic.test_bit(b) {
                    let new_trie_ref = trie_ref.trie_ref_at_path([b]);
                    builder.push_byte(b, new_trie_ref);
                }
                // todo I didn't find a histogram/groupby function, so couldn't aggregate letter counts yet, just returning one
                else {
                    let new_map = BytesTrieMap::from_iter(loc.into_iter().copied().map(|x| ([x], 1)));
                    let temp_zipper = new_map.read_zipper();
                    builder.graft_at_byte(b, &temp_zipper)
                }
            }
        });

        println!("test");
        let mut rz = counted.read_zipper();
        while let Some(v) = rz.to_next_val() {
            // todo write out useful print function, that shows the count submaps
            println!("v: {}, p: {}", v, std::str::from_utf8(rz.path()).unwrap());
        }
    }
}
