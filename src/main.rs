#![allow(warnings)]

use std::alloc::{alloc, dealloc, Layout};
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::{mem, ptr};
use std::time::Instant;
use ringmap::ring::*;
use ringmap::bytize::*;
use ringmap::bytetrie::{ByteTrieNodePtr, store_new, register, unregister, init, MMAP, store_prepared};
use rayon::prelude::*;
use std::ffi::{CString, c_void};


fn main() {
    init(false);

    let args: Vec<_> = (0..4).collect();
    // println!("main thread id {}", threadid(std::thread::current()));

    // let _ = args.par_iter().map(|arg| {
    register();

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
        let r = unsafe { u64::from_be_bytes(*(buffer as *const [u8; 8])) };
        r
    }

    let mut buffer = unsafe { alloc(Layout::new::<u64>()) };

    let mut first = true;
    for N in [1000, 10, 100, 1000, 10000, 100000, 1000000, 10000000, 100000000] { // 1000, 10, 100, 1000, 10000, 100000, 1000000, 10000000, 100000000
        for overlap in [0.0001, 0.01, 0.2, 0.5, 0.8, 0.99, 0.9999] {
            let O = ((1. - overlap) * N as f64) as u64;
            let t0 = Instant::now();
            unsafe {
                let mut vnl = store_new();
                let mut vnr = store_new();
                // println!("{:?}", vnl);
                for i in (0..N) { vnl.insert(gen_key(i, buffer), i); }
                // println!("{:?}", vnl);
    
                for i in 0..N { assert_eq!(vnl.get(gen_key(i, buffer)), Some(i).as_ref()); }
                for i in N..2*N { assert_eq!(vnl.get(gen_key(i, buffer)), None); }
                // let mut c: Vec<u64> = Vec::with_capacity(N as usize);
                // vnl.items().for_each(|(k, v)| {
                //     assert!(0 <= *v && *v < N);
                //     assert_eq!(k, gen_key(*v, buffer));
                //     c.push(parse_key(&k[..], buffer));
                // });
                // c.sort();
                // assert_eq!(c, (0..N).collect::<Vec<u64>>());
                for i in O..(N+O) { vnr.insert(gen_key(i, buffer), i); }
    
                let j = vnl.join(&vnr);
                // let m = vnl.meet(&vnr);
                // let mut l_no_r = vnl.subtract(&vnr);
                // for i in 0..O { assert_eq!(l_no_r.get(prefix_key(&i)), vnl.get(prefix_key(&i))); }
                // for i in N..(2*N) { assert!(!l_no_r.contains(prefix_key(&i))); }
                //
                for i in O..N { assert!(vnl.contains(gen_key(i, buffer)) && vnr.contains(gen_key(i, buffer))); }
                for i in 0..O { assert!(vnl.contains(gen_key(i, buffer)) && !vnr.contains(gen_key(i, buffer))); }
                for i in N..(N+O) { assert!(!vnl.contains(gen_key(i, buffer)) && vnr.contains(gen_key(i, buffer))); }
                for i in 0..(2*N) { assert_eq!(j.contains(gen_key(i, buffer)), (vnl.contains(gen_key(i, buffer)) || vnr.contains(gen_key(i, buffer)))); }
                // for i in 0..(2*N) { assert_eq!(m.contains(gen_key(i, buffer)), (vnl.contains(gen_key(i, buffer)) && vnr.contains(gen_key(i, buffer)))); }
                for i in 0..(N+O) { assert_eq!(j.get(gen_key(i, buffer)), vnl.get(gen_key(i, buffer)).join(&vnr.get(gen_key(i, buffer)))); }
                // for i in O..N { assert_eq!(m.get(gen_key(i, buffer)), vnl.get(gen_key(i, buffer)).meet(&vnr.get(gen_key(i, buffer)))); }
                // for i in 0..(2*N) { println!("{} {} {} {}", i, r.contains(i), vnl.contains(i), vnr.contains(i)); } // assert!(r.contains(i));
            }
            if !first { println!("{} ns/it N={N}, overlap={overlap} ", t0.elapsed().as_nanos() as f64/N as f64) };
        }
        first = false;
    }

    unregister();
    unsafe { dealloc(buffer, Layout::new::<u64>()); }

    // *arg}).collect::<Vec<_>>();
}
/*
623.9 ns/it N=10, overlap=0.0001
609.9 ns/it N=10, overlap=0.01
673 ns/it N=10, overlap=0.2
625 ns/it N=10, overlap=0.5
581.9 ns/it N=10, overlap=0.8
577.9 ns/it N=10, overlap=0.99
569.9 ns/it N=10, overlap=0.9999
103.86 ns/it N=100, overlap=0.0001
90.74 ns/it N=100, overlap=0.01
89.04 ns/it N=100, overlap=0.2
87.84 ns/it N=100, overlap=0.5
86.93 ns/it N=100, overlap=0.8
95.65 ns/it N=100, overlap=0.99
79.62 ns/it N=100, overlap=0.9999
104.017 ns/it N=1000, overlap=0.0001
91.058 ns/it N=1000, overlap=0.01
85.83 ns/it N=1000, overlap=0.2
89.735 ns/it N=1000, overlap=0.5
85.309 ns/it N=1000, overlap=0.8
75.775 ns/it N=1000, overlap=0.99
73.401 ns/it N=1000, overlap=0.9999
82.8702 ns/it N=10000, overlap=0.0001
83.2317 ns/it N=10000, overlap=0.01
83.2077 ns/it N=10000, overlap=0.2
82.9834 ns/it N=10000, overlap=0.5
83.468 ns/it N=10000, overlap=0.8
82.3764 ns/it N=10000, overlap=0.99
82.6559 ns/it N=10000, overlap=0.9999
104.4009 ns/it N=100000, overlap=0.0001
101.5477 ns/it N=100000, overlap=0.01
96.21495 ns/it N=100000, overlap=0.2
90.64192 ns/it N=100000, overlap=0.5
88.48256 ns/it N=100000, overlap=0.8
85.27081 ns/it N=100000, overlap=0.99
87.51369 ns/it N=100000, overlap=0.9999
101.991636 ns/it N=1000000, overlap=0.0001
101.427655 ns/it N=1000000, overlap=0.01
100.24615 ns/it N=1000000, overlap=0.2
100.862822 ns/it N=1000000, overlap=0.5
96.24758 ns/it N=1000000, overlap=0.8
94.982349 ns/it N=1000000, overlap=0.99
96.18851 ns/it N=1000000, overlap=0.9999
108.6765099 ns/it N=10000000, overlap=0.0001
108.3751297 ns/it N=10000000, overlap=0.01
105.5928277 ns/it N=10000000, overlap=0.2
101.4903627 ns/it N=10000000, overlap=0.5
99.8253515 ns/it N=10000000, overlap=0.8
98.6600692 ns/it N=10000000, overlap=0.99
99.8898589 ns/it N=10000000, overlap=0.9999
130.02989718 ns/it N=100000000, overlap=0.0001
130.91532138 ns/it N=100000000, overlap=0.01
127.77262829 ns/it N=100000000, overlap=0.2
124.67983606 ns/it N=100000000, overlap=0.5
119.7867142 ns/it N=100000000, overlap=0.8
116.31670936 ns/it N=100000000, overlap=0.99
119.49858375 ns/it N=100000000, overlap=0.9999
 */