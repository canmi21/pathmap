
//GOAT, partial adaptation of Adam's code

use std::collections::HashMap;
use std::fs::File;
use std::io::stdout;
use std::rc::Rc;
use num_traits::ToPrimitive;
use crate::trie_node::*;
use crate::old_cursor::ByteTrieNodeIter;
use crate::trie_map::BytesTrieMap;
use crate::dense_byte_node::*;
use crate::zipper::Zipper;
use crate::zipper::zipper_priv::ZipperPriv;

const BASE: usize = 256;

//GOAT, remove debug bound on UsefulNumber
pub trait UsefulNumber<const N : usize> : num_traits::PrimInt + num_traits::ops::saturating::SaturatingAdd + std::ops::Mul + std::ops::Add + std::ops::AddAssign + num_traits::FromPrimitive + num_traits::ToPrimitive + num_traits::ToBytes + num_traits::FromBytes<Bytes=[u8; N]> + core::fmt::Debug {}
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
        v.push(i.to_le_bytes().as_ref()[0]);
        i = i.checked_add(&step).unwrap();
    }
    v
}

fn repetition<const N : usize, R : UsefulNumber<N>>(step: R, offset: R) -> Vec<usize> {
    debug_assert!(offset >= R::zero());
    debug_assert!(offset < step);
    let mut result = Vec::with_capacity(step.to_usize().unwrap_or(0));
    let mut last = R::zero();
    let mut count = 0usize;

    let limit = R::from(BASE).unwrap() * step;
    let mut i = offset;
    while i < limit {
        let xb = i / R::from(BASE).unwrap();
        if xb == last { count += 1; }
        else {
            last = xb;
            result.push(count);
            count = 1;
        }
        i = i.checked_add(&step).unwrap();
    }
    result.push(count);
    result
}

fn decompose_seq(xs: &[usize]) -> (usize, &[usize]) {
    let n = xs.len();
    if n == 0 { return (0, &[]) }
    let mut pi = vec![0; n];
    for i in 1..n {
        let mut j = pi[i - 1];
        while j > 0 && xs[j] != xs[i] {
            j = pi[j - 1]
        }
        if xs[j] == xs[i] { pi[i] = j + 1 }
        else { pi[i] = 0 }
    }
    let p = n - pi[pi.len()-1];
    return if n % p == 0 { (n, &xs[..p]) }
           else { (1, xs) };
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
    let mut seq = Vec::with_capacity(n);
    let mut c = 0;
    let mut it = pat.iter().cycle().scan(0, |state, &x| {
        *state += x;
        Some(*state)
    });

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
            if k >= next_numbers[0] {
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
                new_node.set_val(k, cf.value.as_ref().unwrap().clone());
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
                    new_node.set_val(k, cf.value.as_ref().unwrap().clone());
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
                    new_node.set_child(k, cf.rec.as_ref().unwrap().clone());
                }
            }
        }
    }
    new_node
}

fn visit<V : Clone, F : std::io::Write>(c: &TrieNodeODRc<V>, file: &mut F, hm: &mut HashMap<u64, ()>) {
    let dense_node = c.borrow().as_dense().unwrap();
    let address = dense_node as *const _ as u64;
    let mut it = ByteTrieNodeIter::new(dense_node);
    while let Some((k, cf)) = it.next() {
        if let Some(r) = cf.rec.as_ref() {
            let other_address = Rc::as_ptr(r.as_rc()) as *const DenseByteNode<V> as u64;
            if !hm.contains_key(&other_address) { visit(r, file, hm); hm.insert(other_address, ()); }
            file.write(format!("n{address} -> n{other_address} [label={k}]\n").as_bytes());
        }
        if let Some(_) = cf.value.as_ref() {
            file.write(format!("n{address} -> {k}\n").as_bytes());
        }
    }
}


fn graphviz<V : Clone, F : std::io::Write>(map: &BytesTrieMap<V>, file: &mut F) {
    file.write("strict digraph G { \n ranksep=3\n".as_bytes());
    let mut hm = HashMap::new();
    visit(&map.root(), file, &mut hm);
    file.write("}".as_bytes());
}


fn graphviz_stdout<V : Clone, F : std::io::Write>(map: &BytesTrieMap<V>) {
    graphviz(map, &mut stdout())
}


pub(crate) fn compressed_range<V : Clone, const NUM_SIZE : usize, R : UsefulNumber<NUM_SIZE>>(
    start: R, stop: R, step: R, value: V) -> Option<TrieNodeODRc<V>> {
    debug_assert!(start < stop);
    debug_assert!(step.to_i128().unwrap() <= 256);

    //Special case where the path is only 1-byte long
    if NUM_SIZE == 1 {
        let mut node = DenseByteNode::new();
        let mut n = start.to_u16().unwrap();
        while n < stop.to_u16().unwrap() {
            node.set_val(n.to_u8().unwrap(), value.clone());
            n = n + step.to_u16().unwrap();
        }
        return Some(TrieNodeODRc::new(node))
    }

    let split_start_bytes = start.to_be_bytes();
    let split_stop_bytes = stop.to_be_bytes();
    let split_start = split_start_bytes.as_ref();
    let split_stop = split_stop_bytes.as_ref();

    // let first_divisible = start - offset + (if offset != 0 { step } else { 0 });
    // if first_divisible >= stop { return None }

    let mut lv1 = Vec::new();
    let offset = start % step;
    let pat = pattern(step, offset);
    // println!("pat len={} {pat:?}", pat.len());
    let mut pat_it = pat.into_iter().cycle();
    let r1 = repetition(step, offset);
    // println!("rep len={} {r1:?}", r1.len());

    let stop_order = split_stop.len() - 1;
    // println!("Stop order {:?}", stop_order);

    //Build the base level
    let (rep, section) = decompose_seq(&r1[..]);
    for i in 0..rep {
        if i == 0 {
            for &tk in section.iter() {
                // println!("take {tk}");
                let mut node = DenseByteNode::with_capacity(tk);
                for _ in 0..tk {
                    let k = pat_it.next().unwrap();
                    // println!("set value {k}");
                    node.set_val(k, value.clone());
                }
                lv1.push(TrieNodeODRc::new(node));
                // println!(": {}", Rc::as_ptr(lv1[lv1.len()-1].as_rc()) as *const DenseByteNode<V> as u64);
            }
        } else {
            // for &tk in section.iter() {
            //     for _ in 0..tk {
            //         pat_it.next().unwrap();
            //     }
            // }
        }
    }

    //Build the remaining levels
    let mut lvs = Vec::with_capacity(NUM_SIZE);
    lvs.push(lv1);
    // let mut rs = Vec::with_capacity(NUM_SIZE);
    // rs.push(r1);
    for _ in 1..stop_order {
        let mut lv_prev_it = lvs[lvs.len()-1].iter().cycle();
        let mut lv_current = vec![];
        // let r = one_up(&rs[rs.len()-1][..], step.to_usize().unwrap());
        // println!("repeat {r:?}");
        // for &tk in r.iter() {
        for _ in 0..step.to_u16().unwrap() {
            let mut node = DenseByteNode::with_capacity(256);
            for k in 0..256 {
                let c = lv_prev_it.next().unwrap();
                node.set_child(k as u8, c.clone());
            }
            lv_current.push(TrieNodeODRc::new(node));
        }

        // rs.push(r);
        lvs.push(lv_current);
    }

    let mut prev_it = lvs.last().unwrap().iter().cycle();
    for _ in 0..split_start[0] { prev_it.next().unwrap(); }

    let mut new_node = DenseByteNode::new();

    if split_start[0] != split_stop[0] {
        let prev_node = prev_it.next().unwrap();
        debug_assert!(split_start[0] + 1 <= split_stop[0]);

        for (k, c) in (split_start[0] + 1 .. split_stop[0]).zip(prev_it.by_ref().take(BASE as usize)) {
            new_node.set_child(k, c.clone());
        }
        let next_node = prev_it.next().unwrap();

        // for the last numbers, iterate back from top to bottom
        if let Some(d_start) = start_generator(&split_start[..], 1, prev_node.borrow().as_dense().unwrap(), step) {
            // println!("start gen");
            new_node.set_child(split_start[0], TrieNodeODRc::new(d_start));
        }
        if let Some(d_stop) = stop_generator(&split_stop[..], 1, next_node.borrow().as_dense().unwrap(), step, offset) {
            // println!("stop gen");
            new_node.set_child(split_stop[0], TrieNodeODRc::new(d_stop));
        }
    } else {
        let node = prev_it.next().unwrap();
        let d_double = double_sided_generator(1, &split_start[..], &split_stop[..], node.borrow().as_dense().unwrap(), step, offset);
        // println!("two gen");
        new_node.set_child(split_start[0], TrieNodeODRc::new(d_double));
    }
    // let mut all_nodes = lvs.iter().enumerate().flat_map(|(l, layer)| layer.iter().enumerate().map(move |(j, n)| (l, j, n.borrow().as_dense().unwrap()))).collect::<Vec<_>>();
    // all_nodes.push((NUM_SIZE, 0, &new_node));

    // let mut it = ByteTrieNodeIter::new(&lvs[0][0].as_rc().as_dense().unwrap());
    // while let Some((k, v)) = it.next() { print!("{},", k); }
    // println!(": {}", Rc::as_ptr(lvs[0][0].as_rc()) as *const DenseByteNode<V> as u64);
    //
    let top = TrieNodeODRc::new(new_node);
    // let tmp = BytesTrieMap::new_with_root(top.clone());
    // let mut nav = tmp.read_zipper();
    // nav.descend_first_k_path(4);
    // println!("inpath {:?}", nav.path());
    // nav.ascend_byte();
    // let binding = nav.get_focus().into_option().unwrap();
    // it = ByteTrieNodeIter::new(&binding.as_rc().as_dense().unwrap());
    // while let Some((k, v)) = it.next() { print!("{},", k); }
    // println!(": {}", Rc::as_ptr(binding.as_rc()) as *const DenseByteNode<()> as u64);
    // drop(nav);
    // drop(tmp);

    Some(top)
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
        let mut i = start as usize;
        let map = BytesTrieMap::new_with_root(compressed_range(start, stop, step, ()).unwrap());

        let mut it = map.iter();
        while let Some((path, _)) = it.next() {
            let cn = u8::from_be_bytes(path.try_into().unwrap());
            assert_eq!(cn as usize, i);
            i += step as usize;
        }
        assert!((i - step as usize) < stop as usize);
        assert!(i >= stop as usize);
    }
}

#[test]
fn range_generator_1() {
    let params: Vec<(u16, u16, u16)> = vec![
        (0, 20, 2), //Standard short step-by-one
    ];

    for &(start, stop, step) in params.iter() {
        let mut i = start as usize;
        let cr_root = compressed_range(start, stop, step, ()).unwrap();

        let map = BytesTrieMap::new_with_root(cr_root);

        let mut it = map.iter();
        while let Some((path, _)) = it.next() {
            let cn = u16::from_be_bytes(path.try_into().unwrap());
            assert_eq!(cn as usize, i);
            i += step as usize;
        }
        assert!((i - step as usize) < stop as usize);
        assert!(i >= stop as usize);
    }
}

#[test]
fn compressed_range_equivalent() {
    let mut params = vec![
        (0u32, 10000u32, 42u32),
        (1000u32, 318021u32, 188u32),
        (1000u32, 318021u32, 3u32),
        (498230428u32, 1498230428u32, 128u32),
        (256u32, 65535u32, 2u32),
    ];

    for &(start, stop, step) in params.iter() {
        let mut i = start as usize;
        let cr_root = compressed_range(start, stop, step, ()).unwrap();
        let cr = BytesTrieMap::new_with_root(cr_root);

        // let mut f = File::create("/home/adam/Projects/PathMap/M42.graphviz").unwrap();
        // graphviz(&cr, &mut f);
        // std::hint::black_box(cr);
        let mut it = cr.read_zipper();
        while let Some(_) = it.to_next_val() {
            // println!("path {:?}", it.path());
            let cn = u32::from_be_bytes(it.path().try_into().unwrap());
            assert_eq!(cn as usize, i);
            i += step as usize;
        }
        assert!((i - step as usize) < stop as usize);
        assert!(i >= stop as usize);
    }
}
