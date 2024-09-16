#![allow(warnings)]

use std::alloc::{alloc, dealloc, Layout};
use std::hash::Hash;
use std::ptr;
use std::time::Instant;
use ringmap::ring::*;
use ringmap::bytize::*;
use ringmap::trie_map::BytesTrieMap;

// #[global_allocator]
// static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
// static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn main() {
    fn gen_key<'a>(i: u64, buffer: *mut u8) -> &'a [u8] {
        let ir = u64::from_be(i);
        unsafe { ptr::write_unaligned(buffer as *mut u64, ir) };
        let bs = (8 - ir.trailing_zeros()/8) as usize;
        let l = bs.max(1);
        unsafe { std::slice::from_raw_parts(buffer.byte_offset((8 - l) as isize), l) }
    }

    fn parse_key<'a>(k: &'a [u8], buffer: *mut u8) -> u64 {
        let kp = unsafe { k.as_ptr() } as *const u64;
        let shift = 64usize.saturating_sub(k.len()*8);
        //(*kp) & (!0u64 >> shift)
        let r = unsafe { u64::from_be_bytes(*(buffer as *const [u8; 8])) };
        r
    }

    let mut buffer = unsafe { alloc(Layout::new::<u64>()) };

    let mut first = true;
    for N in [1000, 10, 100, 1000, 10000, 100000, 1000000, 10000000, 100000000] {
        for overlap in [0.0001, 0.01, 0.2, 0.5, 0.8, 0.99, 0.9999] {
            let O = ((1. - overlap) * N as f64) as u64;
            let t0 = Instant::now();
            {
                let mut vnl = BytesTrieMap::new();
                let mut vnr = BytesTrieMap::new();
                for i in 0..N { vnl.insert(gen_key(i, buffer), i); }
                // println!("{:?}", vnl.root);
                for i in 0..N { assert_eq!(vnl.get(gen_key(i, buffer)), Some(i).as_ref()); }
                for i in N..2*N { assert_eq!(vnl.get(gen_key(i, buffer)), None); }
                let mut c: Vec<u64> = Vec::with_capacity(N as usize);
                vnl.items().for_each(|(k, v)| {
                    assert!(0 <= *v && *v < N);
                    assert_eq!(k, gen_key(*v, buffer));
                    c.push(parse_key(&k[..], buffer));
                });
                // let mut c_: Vec<u64> = Vec::with_capacity(N as usize);
                // let mut it = vnl.item_cursor();
                // loop {
                //     match it.next() {
                //         None => {
                //             break
                //         }
                //         Some((k, v)) => {
                //             assert!(0 <= *v && *v < N);
                //             assert_eq!(k, gen_key(*v, buffer));
                //             c_.push(parse_key(k, buffer));
                //         }
                //     }
                // }
                // assert_eq!(c, c_);
                c.sort();
                assert_eq!(c, (0..N).collect::<Vec<u64>>());
                for i in O..(N+O) { vnr.insert(gen_key(i, buffer), i); }

                let j = vnl.join(&vnr);
                let m = vnl.meet(&vnr);
                // let mut l_no_r = vnl.subtract(&vnr);
                // for i in 0..O { assert_eq!(l_no_r.get(gen_key(i, buffer)), vnl.get(gen_key(i, buffer))); }
                // for i in N..(2*N) { assert!(!l_no_r.contains(gen_key(i, buffer))); }

                for i in O..N { assert!(vnl.contains(gen_key(i, buffer)) && vnr.contains(gen_key(i, buffer))); }
                for i in 0..O { assert!(vnl.contains(gen_key(i, buffer)) && !vnr.contains(gen_key(i, buffer))); }
                for i in N..(N+O) { assert!(!vnl.contains(gen_key(i, buffer)) && vnr.contains(gen_key(i, buffer))); }
                for i in 0..(2*N) { assert_eq!(j.contains(gen_key(i, buffer)), (vnl.contains(gen_key(i, buffer)) || vnr.contains(gen_key(i, buffer)))); }
                for i in 0..(2*N) { assert_eq!(m.contains(gen_key(i, buffer)), (vnl.contains(gen_key(i, buffer)) && vnr.contains(gen_key(i, buffer)))); }
                for i in 0..(N+O) { assert_eq!(j.get(gen_key(i, buffer)), vnl.get(gen_key(i, buffer)).join(&vnr.get(gen_key(i, buffer)))); }
                for i in O..N { assert_eq!(m.get(gen_key(i, buffer)), vnl.get(gen_key(i, buffer)).meet(&vnr.get(gen_key(i, buffer)))); }
                // for i in 0..(2*N) { println!("{} {} {} {}", i, r.contains(i), vnl.contains(i), vnr.contains(i)); } // assert!(r.contains(i));
            }
            if !first { println!("{} ns/it N={N}, overlap={overlap} ", t0.elapsed().as_nanos() as f64/N as f64) };
        }
        first = false;
    }

    unsafe { dealloc(buffer, Layout::new::<u64>()); }

}
