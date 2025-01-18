use crate::utils::byte_mask_set;
use crate::zipper::*;

pub trait ZipperMorphisms<V> {
    /// Applies a catamorphism to the trie, by traversing in a depth-first order, from the zipper's root
    ///
    /// ## Args
    /// - `map_f`: `mapper(&self, v: &V, path: &[u8]) -> W`
    /// Maps value `v` at a leaf `path` into an intermediate result
    ///
    /// - `collapse_f`: `collapse(&self, v: &V, w: &W, path: &[u8]) -> W`
    /// Folds value `v` at a non-leaf `path` with the aggregated results from the trie below `path`
    ///
    /// - `alg_f`: `alg(&self, m: [u64; 4], cs: &[W], path: &[u8]) -> W`
    /// Aggregates the results from the child branches, `cs`, descending from `path` into a single result
    ///
    /// In all cases, the `path` arg is the [origin_path](ZipperAbsolutePath::origin_path)
    ///
    /// GOAT!! Are these names (map, collapse, and alg) part of math canon?  They are not very descriptive and hopefully we can change them.
    ///
    /// **NOTE**: The traversal order, while depth-first, is subtly different from the order of
    /// [ZipperIteration::to_next_val].  `to_next_val` visits values first before descending to the
    /// branches below, while `cata` calls the `mapper` on the deepest values first, before returning
    /// to higher levels where `collapse` is called.
    fn into_cata<W, MapF, CollapseF, AlgF>(self, map_f: MapF, collapse_f: CollapseF, alg_f: AlgF) -> W
        where
        MapF: FnMut(&V, &[u8]) -> W,
        CollapseF: FnMut(&V, W, &[u8]) -> W,
        AlgF: FnMut(&[u64; 4], &mut [W], &[u8]) -> W;

}

impl<'a, Z, V: 'a> ZipperMorphisms<V> for Z where Z: ReadOnlyZipper<'a, V> + ZipperAbsolutePath {
    fn into_cata<W, MapF, CollapseF, AlgF>(mut self, mut map_f: MapF, mut collapse_f: CollapseF, mut alg_f: AlgF) -> W
        where
        MapF: FnMut(&V, &[u8]) -> W,
        CollapseF: FnMut(&V, W, &[u8]) -> W,
        AlgF: FnMut(&[u64; 4], &mut [W], &[u8]) -> W
    {
        let mut line_f = |rtrace: &[u8], w: W, path: &[u8]| {
            let mut r = Some(w);
            for b in rtrace {
                let mut m = [0u64; 4];
                byte_mask_set(&mut m, *b);
                let mut vec = [std::mem::take(&mut r).unwrap()];
                // note path provided here is incorrect, it'd need to be extended by sliced reverse rtrace
                r = Some(alg_f(&m, &mut vec[..], path));
            }
            r.unwrap()
        };


        //`stack` holds a "frame" at each forking point above the zipper position.  No frames exist for values
        let mut stack = Vec::<StackFrame<W>>::with_capacity(12);
        let mut frame_idx = 0;

        self.reset();

        //Push a stack frame for the root, and start on the first branch off the root
        stack.push(StackFrame::new(self.child_count(), self.child_mask()));
        if !self.descend_first_byte() {
            //Empty trie is a special case
            return alg_f(&[0; 4], &mut [], self.origin_path().unwrap());
        }

        loop {
            //Descend to the next forking point
            let mut at_leaf = false;
            while self.child_count() < 2 {
                if !self.descend_until() {
                    at_leaf = true;
                    break;
                }
            }

            if at_leaf {
                //Map the value from this leaf
                let cur_w = self.get_value().map(|v| map_f(v, self.origin_path().unwrap()));

                //Ascend back to the last fork point
                let cur_w = ascend_to_fork(&mut self, &mut map_f, &mut collapse_f, &mut line_f, cur_w);

                //Merge the result into the stack frame
                match cur_w {
                    Some(w) => stack[frame_idx].push_val(w),
                    None => stack[frame_idx].push_none(),
                }

                debug_assert!(stack[frame_idx].child_idx <= self.child_count());
                while stack[frame_idx].child_idx == self.child_count() {

                    //We finished all the children from this branch, so run the aggregate function
                    let stack_frame = &mut stack[frame_idx];
                    let mut w = alg_f(&stack_frame.child_mask, &mut stack_frame.children, self.origin_path().unwrap());
                    if frame_idx == 0 {
                        return w
                    } else {
                        frame_idx -= 1;

                        //Check to see if we have a value here we need to collapse
                        if let Some(v) = self.get_value() {
                            w = collapse_f(v, w, self.origin_path().unwrap());
                        }

                        //Ascend the rest of the way back up to the branch
                        let w = ascend_to_fork(&mut self, &mut map_f, &mut collapse_f, &mut line_f, Some(w)).unwrap();

                        //Merge the result into the stack frame
                        stack[frame_idx].push_val(w);
                        debug_assert!(stack[frame_idx].child_idx <= self.child_count());
                    }
                }

                //Position to descend the next child branch
                let descended = self.descend_indexed_branch(stack[frame_idx].child_idx);
                debug_assert!(descended);

            } else {
                //Push a new stack frame for this branch
                frame_idx += 1;
                let child_mask = self.child_mask();
                let child_cnt = self.child_count();
                if stack.len() <= frame_idx {
                    stack.push(StackFrame::new(child_cnt, child_mask));
                    debug_assert!(stack.len() > frame_idx);
                } else {
                    stack[frame_idx].reset(child_mask);
                }

                //Descend the first child branch
                self.descend_first_byte();
            }
        }
    }

}

#[inline(always)]
fn ascend_to_fork<'a, Z, V: 'a, W, MapF, CollapseF, LineF>(z: &mut Z, map_f: &mut MapF, collapse_f: &mut CollapseF, line_f: &mut LineF, mut cur_w: Option<W>) -> Option<W>
    where
    Z: ReadOnlyZipper<'a, V> + ZipperAbsolutePath,
    MapF: FnMut(&V, &[u8]) -> W,
    CollapseF: FnMut(&V, W, &[u8]) -> W,
    LineF: FnMut(&[u8], W, &[u8]) -> W,
{
    loop {
        let mut acc = vec![];

        // we should be able to store the current path, get back how much we ascended, and slice based on that?
        // No need to go step by step in the traversal, but I don't know how to implement it
        loop {
            let last = z.path().last().unwrap();
            if z.ascend_byte() {
                if !z.is_value() {
                    acc.push(*last)
                } else {
                    break
                }
            } else {
                break
            }
        }

        cur_w = Some(line_f(&acc[..], cur_w.unwrap(), z.origin_path().unwrap()));

        if z.at_root() {
            return cur_w
        }

        if z.child_count() < 2 {
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

#[cfg(test)]
mod tests {
    use crate::trie_map::BytesTrieMap;
    use crate::utils::{byte_mask_from_iter, byte_mask_join, IntoByteMaskIter};
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

            let sum = zip.into_cata(
                |_v, path| {
                    let val = (*path.last().unwrap() as char).to_digit(10).unwrap();
                    // println!("map path=\"{}\" val={val}", String::from_utf8_lossy(path));
                    val
                },
                |_v, upstream, path| {
                    let this_digit = (*path.last().unwrap() as char).to_digit(10).unwrap();
                    // println!("collapse path=\"{}\", upstream={upstream}, this_digit={this_digit}, sum={}", String::from_utf8_lossy(path), upstream + this_digit);
                    upstream + this_digit
                },
                |_child_mask, children, _path| {
                    // println!("aggregate path=\"{}\", children={children:?}", String::from_utf8_lossy(_path));
                    children.into_iter().fold(0, |sum, child| sum + *child)
                }
            );
            // println!("sum = {sum}\n");
            assert_eq!(sum, expected_sum);
        }
    }

    #[test]
    fn cata_test2() {
        let mut btm = BytesTrieMap::new();
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });

        // like val count, but without counting internal values
        let cnt = btm.read_zipper().into_cata(
            |v, p| { 1usize },
            |v, w, p| { w },
            |m, ws, p| { ws.iter().sum() });
        assert_eq!(cnt, 11);

        let longest = btm.read_zipper().into_cata(
            |v, p| { p.to_vec() },
            |v, w, p| { w },
            |m, ws: &mut [Vec<u8>], p| { ws.iter_mut().max_by_key(|p| p.len()).map_or(vec![], std::mem::take) });
        assert_eq!(std::str::from_utf8(longest.as_slice()).unwrap(), "rubicundus");

        let at_truncated = btm.read_zipper().into_cata(
            |v, p| { vec![] },
            |v, w, p| { vec![v.clone()] },
            |m, ws: &mut [_], p| {
                let mut r = ws.first_mut().map_or(vec![], std::mem::take);
                for w in ws[1..].iter_mut() { r.extend(w.drain(..)); }
                r });
        assert_eq!(at_truncated, vec![3]);

        let char_set = btm.read_zipper().into_cata(
            |v, p| { Err(()) },
            |v, w, p| { w },
            |m, ws: &mut [_], p| {
                let mut r = m.clone();
                for w in ws.into_iter() {
                    if let Ok(cm) = w { r = byte_mask_join(*cm, r) }
                }
                Ok(r) });
        // assert_eq!(char_set, Ok(byte_mask_from_iter(rs.into_iter().flat_map(|s| s.as_bytes()).copied())));
        for b in rs.into_iter().flat_map(|s| s.as_bytes()) {
            print!("{}, ", *b as char);
        }
        println!("mask");
        for b in byte_mask_from_iter(rs.into_iter().flat_map(|s| s.as_bytes()).copied()).byte_mask_iter() {
            print!("{}, ", b as char);
        }
        println!("gotten");
        for b in char_set.ok().unwrap().byte_mask_iter() {
            print!("{}, ", b as char);
        }
        println!();
    }
}
