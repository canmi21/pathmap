
//GOAT, partial adaptation of Adam's code

use crate::trie_node::*;
use crate::old_cursor::ByteTrieNodeIter;
use crate::trie_map::BytesTrieMap;
use crate::dense_byte_node::*;

const BASE: usize = 256;

//GOAT, remove debug bound on UsefulNumber
pub(crate) trait UsefulNumber<const N : usize> : num_traits::PrimInt + num_traits::ops::saturating::SaturatingAdd + std::ops::Mul + std::ops::Add + std::ops::AddAssign + num_traits::FromPrimitive + num_traits::ToPrimitive + num_traits::ToBytes + num_traits::FromBytes<Bytes=[u8; N]> + core::fmt::Debug {}
impl UsefulNumber<1> for u8 {}
impl UsefulNumber<2> for u16 {}
impl UsefulNumber<4> for u32 {}
impl UsefulNumber<8> for u64 {}
impl UsefulNumber<16> for u128 {}

/// Returns a Vec composed of the first byte from each number in the range sequence
fn pattern<const N : usize, R : UsefulNumber<N>>(step: R, offset: R) -> Vec<u8> {
    debug_assert!(offset >= R::zero());
    debug_assert!(offset < step);
    let mut v = Vec::with_capacity(BASE);
    let mut i = offset;
    let limit = match R::from(BASE) {
        Some(base) => base * step,
        None => R::max_value()
    };
    while i < limit {
        v.push(i.to_be_bytes().as_ref()[0]);
        i = i.saturating_add(step);
    }
    v
}

fn repetition<const N : usize, R : UsefulNumber<N>>(step: R, offset: R) -> Vec<usize> {
    debug_assert!(offset >= R::zero());
    debug_assert!(offset < step);
    let mut result = Vec::with_capacity(step.to_usize().unwrap_or(0));
    let mut last = R::zero();
    let mut count = 1usize;

    let limit = match R::from(BASE) {
        Some(base) => base * step,
        None => R::max_value()
    };
    let mut i = offset;
    while i < limit {
        let xb = match R::from(BASE) {
            Some(base) => i / base,
            None => R::zero(),
        };
        if xb == last { count += 1; }
        else {
            last = xb;
            result.push(count);
            count = 1;
        }
        i = i.saturating_add(step);
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

fn one_up(pat: &[usize], n: usize) -> Vec<usize> {
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

fn bytes_number<const N : usize, R : UsefulNumber<N>>(bytes: &[u8]) -> R {
    R::from_be_bytes(bytes.try_into().unwrap())
}

fn start_generator<V : Clone, const N : usize, R : UsefulNumber<N>>(start_list: &[u8], pointer: usize, node: &DenseByteNode<V>, divider: R) -> Option<DenseByteNode<V>> {
    let next_numbers = &start_list[pointer..];
    // let start = bytes_number(start_list);

    if next_numbers.len() == 1 {
        let mut new_node = DenseByteNode::new();
        let mut it = ByteTrieNodeIter::new(node);
        while let Some((k, cf)) = it.next() {
            if k > next_numbers[0] {
                new_node.set_val(k, cf.value.as_ref().unwrap().clone());
            }
        }
        if new_node.node_is_empty() { None }
        else { Some(new_node) }
    } else if next_numbers.len() > 1 {
        let mut new_node = DenseByteNode::new();
        let mut it = ByteTrieNodeIter::new(node);
        while let Some((k, cf)) = it.next() {
            if k > next_numbers[0] {
                new_node.set_child(k, cf.rec.as_ref().unwrap().clone());
            } else if k == next_numbers[0] {
                if let Some(child) = start_generator(start_list, pointer+1, cf.rec.as_ref().unwrap().borrow().as_dense().unwrap(), divider) {
                    new_node.set_child(k, TrieNodeODRc::new(child));
                }
            }
        }
        if new_node.node_is_empty() { None }
        else { Some(new_node) }
    } else {
        None
    }
}

fn stop_generator<V : Clone, const N : usize, R : UsefulNumber<N>>(stop_list: &[u8], pointer: usize, node: &DenseByteNode<V>, divider: R, offset: R) -> Option<DenseByteNode<V>> {
    let next_numbers = &stop_list[pointer..];
    let stop: R = bytes_number(stop_list);

    if next_numbers.len() == 1 {
        let mut new_node = DenseByteNode::new();
        let mut it = ByteTrieNodeIter::new(node);
        while let Some((k, cf)) = it.next() {
            if k < next_numbers[0] {
                new_node.set_child(k, cf.rec.as_ref().unwrap().clone());
            }
        }
        if new_node.node_is_empty() { None }
        else { Some(new_node) }
    } else if next_numbers.len() > 1 {
        let mut r_buf = Vec::from(&stop_list[..(pointer + 1)]);
        for _ in 0..(next_numbers.len() - 1) { r_buf.push(0) }
        let r: R = bytes_number(&r_buf[..]);
        let remainder = r % divider;
        let next_step = r + offset - remainder + (if offset < remainder { divider } else { R::zero() });
        let mut new_node = DenseByteNode::new();
        let mut it = ByteTrieNodeIter::new(node);
        while let Some((k, cf)) = it.next() {
            if k < next_numbers[0] {
                new_node.set_child(k, cf.rec.as_ref().unwrap().clone());
            } else if k == next_numbers[0] && next_step < stop {
                if let Some(child) = stop_generator(stop_list, pointer+1, cf.rec.as_ref().unwrap().borrow().as_dense().unwrap(), divider, offset) {
                    new_node.set_child(k, TrieNodeODRc::new(child));
                }
            }
        }
        if new_node.node_is_empty() { None }
        else { Some(new_node) }
    } else {
        None
    }
}

fn double_sided_generator<V : Clone, const N : usize, R : UsefulNumber<N>>(pointer: usize, split_start: &[u8], split_stop: &[u8], help_node: &DenseByteNode<V>, step: R, offset: R) -> DenseByteNode<V> {
    let mut new_node = DenseByteNode::new();
    if split_start[pointer] == split_stop[pointer] {
        let n = help_node.get(split_start[pointer]).unwrap().rec.as_ref().unwrap().borrow().as_dense().unwrap();
        new_node.set_child(split_start[pointer], TrieNodeODRc::new(double_sided_generator(pointer + 1, split_start, split_stop, n, step, offset)));
    } else {
        if pointer == split_start.len() - 1 {
            let mut it = ByteTrieNodeIter::new(help_node);
            while let Some((k, cf)) = it.next() {
                if split_start[pointer] <= k && k < split_stop[pointer] {
                    new_node.set_child(k, cf.rec.as_ref().unwrap().clone());
                }
            }
        } else {
            if let Some(start_dict) = help_node.get(split_start[pointer])
                .and_then(|n_start| start_generator(split_start, pointer + 1, n_start.rec.as_ref().unwrap().borrow().as_dense().unwrap(), step)) {
                new_node.set_child(split_start[pointer], TrieNodeODRc::new(start_dict));
            }

            if let Some(stop_dict) = help_node.get(split_stop[pointer])
                .and_then(|n_stop| stop_generator(split_stop, pointer + 1, n_stop.rec.as_ref().unwrap().borrow().as_dense().unwrap(), step, offset)) {
                new_node.set_child(split_stop[pointer], TrieNodeODRc::new(stop_dict));
            }

            let mut it = ByteTrieNodeIter::new(help_node);
            while let Some((k, cf)) = it.next() {
                if split_start[pointer] < k && k < split_stop[pointer] {
                    new_node.set_child( k, cf.rec.as_ref().unwrap().clone());
                }
            }
        }
    }
    new_node
}

pub(crate) fn compressed_range<V : Clone, const NUM_SIZE : usize, R : UsefulNumber<NUM_SIZE>>(
    start: R, stop: R, step: R, value: V) -> Option<TrieNodeODRc<V>> {
    debug_assert!(start < stop);
    debug_assert!(step.to_i128().unwrap() <= 256);

    let split_start_bytes = start.to_be_bytes();
    let split_stop_bytes = stop.to_be_bytes();
    let split_start = split_start_bytes.as_ref();
    let split_stop = split_stop_bytes.as_ref();

    let offset = start % step;

    // let first_divisible = start - offset + (if offset != 0 { step } else { 0 });
    // if first_divisible >= stop { return None }

    let mut lv1 = Vec::new();
    let pat = pattern(step, offset);
    println!("pat len={} {pat:?}", pat.len());
    let mut pat_it = pat.into_iter().cycle();
    let r1 = repetition(step, offset);
    println!("rep len={} {r1:?}", r1.len());

    //Special case where the path is only 1-byte long
    if NUM_SIZE == 1 {
        let mut node = DenseByteNode::new();
        for _ in 0..r1[0] {
            let n = pat_it.next().unwrap();
            if start <= R::from(n).unwrap() && R::from(n).unwrap() < stop {
                node.set_val(n, value.clone());
            }
        }
        return Some(TrieNodeODRc::new(node))
    }

    let stop_order = split_stop.len() - 1;
    println!("Stop order {:?}", stop_order);

    //Build the base level
    for &tk in r1.iter() {
        let mut node = DenseByteNode::new();
        for k in pat_it.by_ref().take(tk) {
            node.set_val(k, value.clone());
        }
        lv1.push(TrieNodeODRc::new(node));
    }

    //Build the remaining levels
    let mut lvs = Vec::with_capacity(NUM_SIZE);
    lvs.push(lv1);
    let mut rs = Vec::with_capacity(NUM_SIZE);
    rs.push(r1);
    for _ in 1..stop_order {
        let mut lv_prev_it = lvs[lvs.len()-1].iter().cycle();
        let mut lv_current = vec![];
        let r = one_up(&rs[rs.len()-1][..], step.to_usize().unwrap());

        for &tk in r.iter() {
            let mut node = DenseByteNode::new();
            for (k, c) in lv_prev_it.by_ref().take(tk).enumerate() {
                node.set_child(k as u8, c.clone());
            }
            lv_current.push(TrieNodeODRc::new(node));
        }

        rs.push(r);
        lvs.push(lv_current);
    }

    let mut prev_it = lvs.last().unwrap().iter().cycle();
    for _ in 0..split_start[0] { prev_it.next().unwrap(); }
    return if split_start[0] != split_stop[0] {
        let prev_node = prev_it.next().unwrap();
        debug_assert!(split_start[0] + 1 <= split_stop[0]);
        let mut new_node = DenseByteNode::new();
        for (k, c) in (split_start[0] + 1 .. split_stop[0]).zip(prev_it.by_ref().take(BASE as usize)) {
            new_node.set_child(k, c.clone());
        }
        let next_node = prev_it.next().unwrap();

        // for the last numbers, iterate back from top to bottom
        if let Some(d_start) = start_generator(&split_start[..], 1, prev_node.borrow().as_dense().unwrap(), step) {
            new_node.set_child(split_start[0], TrieNodeODRc::new(d_start));
        }
        if let Some(d_stop) = stop_generator(&split_stop[..], 1, next_node.borrow().as_dense().unwrap(), step, offset) {
            new_node.set_child(split_stop[0], TrieNodeODRc::new(d_stop));
        }

        Some(TrieNodeODRc::new(new_node))
    } else {
        let node = prev_it.next().unwrap();
        let mut new_node = DenseByteNode::new();
        let d_double = double_sided_generator(1, &split_start[..], &split_stop[..], node.borrow().as_dense().unwrap(), step, offset);
        new_node.set_child(split_start[0], TrieNodeODRc::new(d_double));

        Some(TrieNodeODRc::new(new_node))
    }
}

pub fn goat_map() -> BytesTrieMap<()> {
    BytesTrieMap::new_with_root(compressed_range(0 as u32, 0 as u32, 0 as u32, ()).unwrap())
}

#[test]
fn range_generator_0() {
    let params: Vec<(u8, u8, u8)> = vec![
        (0, 255, 1), //Standard step-by-one, fill the whole range
        (2,  16, 3), //Step by 3, non-zero starting point
        (135, 255, 150), //Step should not cause an overflow
    ];

    for &(start, stop, step) in params.iter() {
        let mut i = start;
        let map = BytesTrieMap::new_with_root(compressed_range(start, stop, step, ()).unwrap());

        let mut it = map.iter();
        while let Some((path, _)) = it.next() {
            let cn = u8::from_be_bytes(path.try_into().unwrap());
            assert_eq!(cn, i);
            // println!("{cn:?} vs {i:?}");
            i = i.saturating_add(step);
        }
        assert!(i >= stop);
        assert!(i - step < stop);
    }
}

#[test]
fn range_generator_1() {
    let params: Vec<(u16, u16, u16)> = vec![
        (0, 20, 1), //Standard short step-by-one
    ];

    for &(start, stop, step) in params.iter() {
        let mut i = start;
        let map = BytesTrieMap::new_with_root(compressed_range(start, stop, step, ()).unwrap());

        let mut it = map.iter();
        while let Some((path, _)) = it.next() {
            let cn = u16::from_be_bytes(path.try_into().unwrap());
            assert_eq!(cn, i);
            // println!("{cn:?} vs {i:?}");
            i = i.saturating_add(step);
        }
        assert!(i >= stop);
        assert!(i - step < stop);
    }
}

#[test]
fn compressed_range_equivalent() {
    let params = vec![(0u32, 10000u32, 3u32)];

    for &(start, stop, step) in params.iter() {
        let mut i = start;
        let cr = BytesTrieMap::new_with_root(compressed_range(start, stop, step, ()).unwrap());

        let mut it = cr.cursor();
        while let Some((path, _)) = it.next() {
            let cn = u32::from_be_bytes(path.try_into().unwrap());
            i += step;
            assert_eq!(cn, i);
        }
        assert!(i < stop);
        assert!((i + step) > stop);
    }
}



















//GOAT Adam's code

// const BASE: i32 = 256;

// trait UsefulNumber<const N : usize> : num_traits::PrimInt + std::ops::Mul + std::ops::Add + std::ops::AddAssign + num_traits::FromPrimitive + num_traits::ToBytes + num_traits::FromBytes<Bytes=[u8; N]> {}
// impl UsefulNumber<1> for u8 {}
// impl UsefulNumber<2> for u16 {}
// impl UsefulNumber<4> for u32 {}
// impl UsefulNumber<8> for u64 {}
// impl UsefulNumber<16> for u128 {}

// fn pattern<const N : usize, R : UsefulNumber<N>>(step: R, offset: R) -> Vec<u8> {
//     assert!(offset >= R::zero());
//     assert!(offset < step);
//     let mut v = vec![];
//     let mut i = offset;
//     while i < R::from(BASE).unwrap()*step {
//         v.push(i.to_be_bytes().as_ref()[0]);
//         i += step;
//     }
//     v
// }

// fn repetition<const N : usize, R : UsefulNumber<N>>(step: R, offset: R) -> Vec<usize> {
//     assert!(offset >= R::zero());
//     assert!(offset < step);
//     let mut result = Vec::new();
//     let mut last = R::zero();
//     let mut count = 1usize;

//     let mut i = offset;
//     while i < R::from(BASE).unwrap()*step {
//         let xb = i/R::from(BASE).unwrap();
//         if xb == last { count += 1; }
//         else {
//             last = xb;
//             result.push(count);
//             count = 1;
//         }
//         i += step;
//     }
//     result.push(count);
//     result
// }

// // fn expand<'a, I>(it: I, m: &[i32]) -> impl Iterator<Item = I::Item> + 'a
// //     where
// //         I: Iterator + Clone + 'a,
// //         I::Item: 'a,
// // {
// //     it.zip(m.iter()).flat_map(|(e, &i)| repeat(e).take(i as usize))
// // }

// fn nth<I>(mut it: I, n: usize) -> I::Item
//     where
//         I: Iterator,
// {
//     for _ in 0..n-1 {
//         it.next();
//     }
//     it.next().unwrap()
// }

// fn one_up(pat: &[usize], n: usize) -> Vec<usize> {
//     let mut seq = Vec::new();
//     let mut c = 0;
//     let mut it = pat.iter().scan(0, |state, &x| {
//         *state += x;
//         Some(*state)
//     }).cycle();

//     for _ in 0..n {
//         let i = nth(&mut it, BASE as usize);
//         seq.push(i - c);
//         c = i;
//     }

//     seq
// }

// fn number_bytes<const N : usize, R : UsefulNumber<N>>(i: R) -> Vec<u8> {
//     Vec::from(i.to_be_bytes().as_ref())
// }

// fn bytes_number<const N : usize, R : UsefulNumber<N>>(bytes: &[u8]) -> R {
//     R::from_be_bytes(bytes.try_into().unwrap())
// }

// fn start_generator<V : Clone, const N : usize, R : UsefulNumber<N>>(start_list: &[u8], pointer: usize, node: &DenseByteNode<V>, divider: R) -> Option<DenseByteNode<V>> {
//     let next_numbers = &start_list[pointer..];
//     // let start = bytes_number(start_list);

//     if next_numbers.len() == 1 {
//         let mut new_node = DenseByteNode::new();
//         let mut it = ByteTrieNodeIter::new(node);
//         while let Some((k, cf)) = it.next() {
//             if k > next_numbers[0] {
//                 new_node.set_val(k, cf.value.as_ref().unwrap().clone());
//             }
//         }
//         if new_node.is_empty() { None }
//         else { Some(new_node) }
//     } else if next_numbers.len() > 1 {
//         let mut new_node = DenseByteNode::new();
//         let mut it = ByteTrieNodeIter::new(node);
//         while let Some((k, cf)) = it.next() {
//             if k > next_numbers[0] {
//                 new_node.set_child(k, cf.rec.as_ref().unwrap().clone());
//             } else if k == next_numbers[0] {
//                 if let Some(child) = start_generator(start_list, pointer+1, cf.rec.as_ref().unwrap().borrow().as_dense().unwrap(), divider) {
//                     new_node.set_child(k, TrieNodeODRc::new(child));
//                 }
//             }
//         }
//         if new_node.is_empty() { None }
//         else { Some(new_node) }
//     } else {
//         None
//     }
// }

// fn stop_generator<V : Clone, const N : usize, R : UsefulNumber<N>>(stop_list: &[u8], pointer: usize, node: &DenseByteNode<V>, divider: R, offset: R) -> Option<DenseByteNode<V>> {
//     let next_numbers = &stop_list[pointer..];
//     let stop: R = bytes_number(stop_list);

//     if next_numbers.len() == 1 {
//         let mut new_node = DenseByteNode::new();
//         let mut it = ByteTrieNodeIter::new(node);
//         while let Some((k, cf)) = it.next() {
//             if k < next_numbers[0] {
//                 new_node.set_child(k, cf.rec.as_ref().unwrap().clone());
//             }
//         }
//         if new_node.is_empty() { None }
//         else { Some(new_node) }
//     } else if next_numbers.len() > 1 {
//         let mut r_buf = Vec::from(&stop_list[..(pointer + 1)]);
//         for _ in 0..(next_numbers.len() - 1) { r_buf.push(0) }
//         let r: R = bytes_number(&r_buf[..]);
//         let remainder = r % divider;
//         let next_step = r + offset - remainder + (if offset < remainder { divider } else { R::zero() });
//         let mut new_node = DenseByteNode::new();
//         let mut it = ByteTrieNodeIter::new(node);
//         while let Some((k, cf)) = it.next() {
//             if k < next_numbers[0] {
//                 new_node.set_child(k, cf.rec.as_ref().unwrap().clone());
//             } else if k == next_numbers[0] && next_step < stop {
//                 if let Some(child) = stop_generator(stop_list, pointer+1, cf.rec.as_ref().unwrap().borrow().as_dense().unwrap(), divider, offset) {
//                     new_node.set_child(k, TrieNodeODRc::new(child));
//                 }
//             }
//         }
//         if new_node.is_empty() { None }
//         else { Some(new_node) }
//     } else {
//         None
//     }
// }

// fn double_sided_generator<V : Clone, const N : usize, R : UsefulNumber<N>>(pointer: usize, split_start: &[u8], split_stop: &[u8], help_node: &DenseByteNode<V>, step: R, offset: R) -> DenseByteNode<V> {
//     let mut new_node = DenseByteNode::new();
//     if split_start[pointer] == split_stop[pointer] {
//         let n = help_node.get(split_start[pointer]).unwrap().rec.as_ref().unwrap().borrow().as_dense().unwrap();
//         new_node.set_child(split_start[pointer], TrieNodeODRc::new(double_sided_generator(pointer + 1, split_start, split_stop, n, step, offset)));
//     } else {
//         if pointer == split_start.len() - 1 {
//             let mut it = ByteTrieNodeIter::new(help_node);
//             while let Some((k, cf)) = it.next() {
//                 if split_start[pointer] <= k && k < split_stop[pointer] {
//                     new_node.set_child(k, cf.rec.as_ref().unwrap().clone());
//                 }
//             }
//         } else {
//             if let Some(start_dict) = help_node.get(split_start[pointer])
//                 .and_then(|n_start| start_generator(split_start, pointer + 1, n_start.rec.as_ref().unwrap().borrow().as_dense().unwrap(), step)) {
//                 new_node.set_child(split_start[pointer], TrieNodeODRc::new(start_dict));
//             }

//             if let Some(stop_dict) = help_node.get(split_stop[pointer])
//                 .and_then(|n_stop| stop_generator(split_stop, pointer + 1, n_stop.rec.as_ref().unwrap().borrow().as_dense().unwrap(), step, offset)) {
//                 new_node.set_child(split_stop[pointer], TrieNodeODRc::new(stop_dict));
//             }

//             let mut it = ByteTrieNodeIter::new(help_node);
//             while let Some((k, cf)) = it.next() {
//                 if split_start[pointer] < k && k < split_stop[pointer] {
//                     new_node.set_child( k, cf.rec.as_ref().unwrap().clone());
//                 }
//             }
//         }
//     }
//     new_node
// }

// pub(crate) fn compressed_range<V : Clone, const N : usize, R : UsefulNumber<N>>(
//     start: R, stop: R, step: R, value: V) -> Option<TrieNodeODRc<V>> {
//     assert!(start < stop);
//     assert!(step <= R::from(256).unwrap());

//     let split_start_ = number_bytes(start);
//     let split_stop = number_bytes(stop);

//     let mut split_start = vec![0; (split_stop.len() - split_start_.len())];
//     split_start.extend(split_start_);
//     println!("split start {:?}", split_start);
//     assert!(split_stop.len() == split_start.len());

//     let offset = start % step;

//     // let first_divisible = start - offset + (if offset != 0 { step } else { 0 });
//     // if first_divisible >= stop { return None }

//     let stop_order = split_stop.len() - 1;
//     println!("Stop order {:?}", stop_order);

//     let mut lv1 = Vec::new();
//     let mut pat = pattern(step, offset);
//     println!("pat {:?}", pat);
//     let mut pat_it = pat.into_iter().cycle();
//     let r1 = repetition(step, offset);
//     println!("rep {:?}", r1);

//     if stop_order == 0 {
//         let mut node = DenseByteNode::new();
//         for _ in 0..r1[0] {
//             let n = pat_it.next().unwrap();
//             if start <= R::from(n).unwrap() && R::from(n).unwrap() < stop {
//                 node.set_val(n, value.clone());
//             }
//         }
//         // lv1.push(node);
//         return Some(TrieNodeODRc::new(node))
//     }

//     for &tk in &r1 {
//         let mut n = DenseByteNode::new();
//         for k in pat_it.by_ref().take(tk) {
//             n.set_val(k, value.clone());
//         }
//         lv1.push(n);
//     }

//     let mut lvs = vec![lv1];
//     let mut rs = vec![r1];

//     for _ in 1..stop_order {
//         let mut lv_prev_it = lvs[lvs.len()-1].iter().cycle();
//         let mut lv_current = vec![];
//         let r = one_up(&rs[rs.len()-1][..], step.to_usize().unwrap());

//         for &tk in &r {
//             let mut n = DenseByteNode::new();
//             for (k, c) in lv_prev_it.by_ref().take(tk).enumerate() {
//                 unsafe {
//                     let rc = std::rc::Rc::from_raw(c);
//                     std::rc::Rc::increment_strong_count(c);
//                     n.set_child(k as u8, TrieNodeODRc::new_from_rc(rc));
//                 }
//             }
//             lv_current.push(n);
//         }

//         rs.push(r);
//         lvs.push(lv_current);
//     }

//     let mut prev_it = lvs.last().unwrap().iter().cycle();
//     for _ in 0..split_start[0] { prev_it.next().unwrap(); }
//     return if split_start[0] != split_stop[0] {
//         let prev_node = prev_it.next().unwrap();
//         // split_start[0] + 1 <= split_stop[0] always holds
//         let mut new_node = DenseByteNode::new();
//         for (k, c) in (split_start[0] + 1 .. split_stop[0]).zip(prev_it.by_ref().take(BASE as usize)) {
//             new_node.set_child(k, TrieNodeODRc::new_from_rc(unsafe { std::rc::Rc::from_raw(c) }));
//             unsafe {
//                 let rc = std::rc::Rc::from_raw(c);
//                 std::rc::Rc::increment_strong_count(c);
//                 new_node.set_child(k, TrieNodeODRc::new_from_rc(rc));
//             }
//         }
//         let next_node = prev_it.next().unwrap();

//         // for the last numbers, iterate back from top to bottom
//         if let Some(d_start) = start_generator(&split_start[..], 1, prev_node, step) {
//             new_node.set_child(split_start[0], TrieNodeODRc::new(d_start));
//         }
//         if let Some(d_stop) = stop_generator(&split_stop[..], 1, next_node, step, offset) {
//             new_node.set_child(split_stop[0], TrieNodeODRc::new(d_stop));
//         }

//         Some(TrieNodeODRc::new(new_node))
//     } else {
//         let node = prev_it.next().unwrap();
//         let mut new_node = DenseByteNode::new();
//         let d_double = double_sided_generator(1, &split_start[..], &split_stop[..], node, step, offset);
//         new_node.set_child(split_start[0], TrieNodeODRc::new(d_double));

//         Some(TrieNodeODRc::new(new_node))
//     }
// }

