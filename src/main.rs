#![allow(warnings)]

use std::alloc::{alloc, dealloc, Layout};
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::io::Write;
use std::{mem, ptr};
use std::ptr::slice_from_raw_parts;
use std::thread::Thread;
use std::time::Instant;
use libc::{exit, free};
use ringmap::ring::*;
use ringmap::bytize::*;
use ringmap::bytetrie::{ByteTrieNodePtr, store_new, register, unregister, init, MMAP, store_prepared};
// use rayon::prelude::*;
use std::ffi::{CString, c_void};


// #[global_allocator]
// static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
// static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;



fn main() {
    init(false);
    // println!("{:?}", mmap, unsafe { libc::strerror(mmap as u64) });
    // check that the mmap is empty
    // let zeros = vec![0; 128];
    // assert_eq!(&zeros[..], unsafe { &*slice_from_raw_parts(mmap, 128) });
    //
    // // write values into the mmap
    // let incr: Vec<u8> = (0..128 as u8).collect();
    // unsafe { ptr::copy_nonoverlapping::<u8>(incr.as_ptr(), mmap, 128); }
    // // read values back
    // assert_eq!(&incr[..], unsafe { &*slice_from_raw_parts(mmap, 128) });

    // return;
    let args: Vec<_> = (0..4).collect();
    // println!("main thread id {}", threadid(std::thread::current()));

    // let mmap = unsafe { MmapOptions::map_mut() };


    // let results: Vec<i32> = args.par_iter().map(|arg| {
    // unsafe { MEM1.with(|mut MEM| { MEM = &(alloc(layout) as *mut u64) })  }
    register();

    // let mut btn = store_new();
    //
    // println!("initial {:?}", btn);
    // btn.insert(&[1, 2], 1);
    // println!("updated once {:?}", btn);
    // println!("values {:?}", unsafe { *load_values(btn) });

    // return;

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

    // fn gen_key<'a>(i: &'a u64) -> &'a [u8] {
    //     let s = i.to_string();
    //     let r = unsafe { std::mem::transmute::<&[u8], &'a [u8]>(s.as_bytes()) };
    //     mem::forget(s);
    //     r
    // }

    let mut buffer = unsafe { alloc(Layout::new::<u64>()) };
    // for i in 0..10000000 {
    //     let k = gen_key(i, buffer);
    //     let i_ = parse_key(k, buffer);
    //     // println!("{:?} {:b} {:b}", k, i, i_);
    //     assert_eq!(i, i_);
    // }

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
                let mut c: Vec<u64> = Vec::with_capacity(N as usize);
                vnl.items().for_each(|(k, v)| {
                    assert!(0 <= *v && *v < N);
                    assert_eq!(k, gen_key(*v, buffer));
                    c.push(parse_key(&k[..], buffer));
                });
                c.sort();
                assert_eq!(c, (0..N).collect::<Vec<u64>>());
                for i in O..(N+O) { vnr.insert(gen_key(i, buffer), i); }
    
                let j = vnl.join(&vnr);
                let m = vnl.meet(&vnr);
                // let mut l_no_r = vnl.subtract(&vnr);
                // for i in 0..O { assert_eq!(l_no_r.get(prefix_key(&i)), vnl.get(prefix_key(&i))); }
                // for i in N..(2*N) { assert!(!l_no_r.contains(prefix_key(&i))); }
                //
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

    // unsafe { dealloc(MEM as *mut u8, layout) }
    unregister();
    unsafe { dealloc(buffer, Layout::new::<u64>()); }

    // *arg
    // }).collect();
}
/*
837.3 ns/it N=10, overlap=0.0001
984.5 ns/it N=10, overlap=0.01
794.2 ns/it N=10, overlap=0.2
796.2 ns/it N=10, overlap=0.5
837.2 ns/it N=10, overlap=0.8
925.4 ns/it N=10, overlap=0.99
792.2 ns/it N=10, overlap=0.9999
230.75 ns/it N=100, overlap=0.0001
243.67 ns/it N=100, overlap=0.01
226.24 ns/it N=100, overlap=0.2
211.32 ns/it N=100, overlap=0.5
225.34 ns/it N=100, overlap=0.8
220.54 ns/it N=100, overlap=0.99
214.43 ns/it N=100, overlap=0.9999
199.522 ns/it N=1000, overlap=0.0001
197.188 ns/it N=1000, overlap=0.01
204.739 ns/it N=1000, overlap=0.2
207.393 ns/it N=1000, overlap=0.5
214.424 ns/it N=1000, overlap=0.8
228.475 ns/it N=1000, overlap=0.99
225.601 ns/it N=1000, overlap=0.9999
201.6607 ns/it N=10000, overlap=0.0001
198.8635 ns/it N=10000, overlap=0.01
205.7789 ns/it N=10000, overlap=0.2
213.1521 ns/it N=10000, overlap=0.5
222.7545 ns/it N=10000, overlap=0.8
244.3803 ns/it N=10000, overlap=0.99
248.4964 ns/it N=10000, overlap=0.9999
279.45612 ns/it N=100000, overlap=0.0001
282.85245 ns/it N=100000, overlap=0.01
275.66409 ns/it N=100000, overlap=0.2
312.798 ns/it N=100000, overlap=0.5
367.38437 ns/it N=100000, overlap=0.8
407.50769 ns/it N=100000, overlap=0.99
422.10635 ns/it N=100000, overlap=0.9999
311.57582 ns/it N=1000000, overlap=0.0001
312.269288 ns/it N=1000000, overlap=0.01
313.522701 ns/it N=1000000, overlap=0.2
319.542417 ns/it N=1000000, overlap=0.5
330.520893 ns/it N=1000000, overlap=0.8
443.040366 ns/it N=1000000, overlap=0.99
456.06876 ns/it N=1000000, overlap=0.9999
572.1911523 ns/it N=10000000, overlap=0.0001
572.0557287 ns/it N=10000000, overlap=0.01
751.5296861 ns/it N=10000000, overlap=0.2
1041.251731 ns/it N=10000000, overlap=0.5
1346.1233716 ns/it N=10000000, overlap=0.8
1506.9862864 ns/it N=10000000, overlap=0.99
1567.2107075 ns/it N=10000000, overlap=0.9999
1091.28914516 ns/it N=100000000, overlap=0.0001
1111.25386582 ns/it N=100000000, overlap=0.01
1187.82218912 ns/it N=100000000, overlap=0.2
1293.52842158 ns/it N=100000000, overlap=0.5
1269.42422144 ns/it N=100000000, overlap=0.8
3012.85650456 ns/it N=100000000, overlap=0.99
2905.14671898 ns/it N=100000000, overlap=0.9999
 */