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
564.9 ns/it N=10, overlap=0.0001
645 ns/it N=10, overlap=0.01
594.9 ns/it N=10, overlap=0.2
595.9 ns/it N=10, overlap=0.5
680 ns/it N=10, overlap=0.8
565.9 ns/it N=10, overlap=0.99
572.9 ns/it N=10, overlap=0.9999
146.02 ns/it N=100, overlap=0.0001
141.31 ns/it N=100, overlap=0.01
145.82 ns/it N=100, overlap=0.2
125.89 ns/it N=100, overlap=0.5
118.18 ns/it N=100, overlap=0.8
112.47 ns/it N=100, overlap=0.99
114.87 ns/it N=100, overlap=0.9999
170.497 ns/it N=1000, overlap=0.0001
161.174 ns/it N=1000, overlap=0.01
155.805 ns/it N=1000, overlap=0.2
166.252 ns/it N=1000, overlap=0.5
163.357 ns/it N=1000, overlap=0.8
137.868 ns/it N=1000, overlap=0.99
138.909 ns/it N=1000, overlap=0.9999
179.6164 ns/it N=10000, overlap=0.0001
178.9884 ns/it N=10000, overlap=0.01
174.1702 ns/it N=10000, overlap=0.2
165.7784 ns/it N=10000, overlap=0.5
152.1619 ns/it N=10000, overlap=0.8
138.0105 ns/it N=10000, overlap=0.99
142.7867 ns/it N=10000, overlap=0.9999
236.26316 ns/it N=100000, overlap=0.0001
239.13821 ns/it N=100000, overlap=0.01
231.99802 ns/it N=100000, overlap=0.2
216.47687 ns/it N=100000, overlap=0.5
198.04181 ns/it N=100000, overlap=0.8
188.44793 ns/it N=100000, overlap=0.99
184.56085 ns/it N=100000, overlap=0.9999
256.479799 ns/it N=1000000, overlap=0.0001
259.734436 ns/it N=1000000, overlap=0.01
252.164048 ns/it N=1000000, overlap=0.2
241.059391 ns/it N=1000000, overlap=0.5
229.72774 ns/it N=1000000, overlap=0.8
223.34026 ns/it N=1000000, overlap=0.99
225.680325 ns/it N=1000000, overlap=0.9999
298.1657102 ns/it N=10000000, overlap=0.0001
297.7815538 ns/it N=10000000, overlap=0.01
282.2295817 ns/it N=10000000, overlap=0.2
265.8572632 ns/it N=10000000, overlap=0.5
257.1067493 ns/it N=10000000, overlap=0.8
245.825708 ns/it N=10000000, overlap=0.99
257.0178079 ns/it N=10000000, overlap=0.9999
377.62043461 ns/it N=100000000, overlap=0.0001
377.17087218 ns/it N=100000000, overlap=0.01
364.60444348 ns/it N=100000000, overlap=0.2
344.58455692 ns/it N=100000000, overlap=0.5
324.39339809 ns/it N=100000000, overlap=0.8
309.81638094 ns/it N=100000000, overlap=0.99
316.61982249 ns/it N=100000000, overlap=0.9999
 */