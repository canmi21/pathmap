#![feature(likely_unlikely)]

use std::hint::{unlikely, likely};
use divan::{Bencher, Divan};
use pathmap::utils::find_prefix_overlap;
use rand::prelude::StdRng;
use rand_distr::{Exp, Triangular};
use pathmap::fuzzer::*;
use rand::SeedableRng;
use rand_distr::Distribution;
use std::marker::PhantomData;
use std::process::Termination;


const PAGE_SIZE: usize = 4096;
const TO_TEST: usize = 1000000;

fn count_shared_reference<'a, 'b>(p: &'a [u8], q: &'b [u8]) -> usize {
    p.iter().zip(q)
        .take_while(|(x, y)| x == y).count()
}

#[inline]
fn count_shared_bare(a: &[u8], b: &[u8]) -> usize {
    let mut cnt = 0;
    loop {
        if unlikely(cnt == a.len() || cnt == b.len()) {break}
        if unlikely(unsafe{ a.get_unchecked(cnt) != b.get_unchecked(cnt) }) {break}
        cnt += 1;
    }
    cnt
}

#[inline(always)]
unsafe fn same_page<'a, const VECTOR_SIZE: usize>(slice: &'a [u8]) -> bool {
    let address = slice.as_ptr() as usize;
    // Mask to keep only the last 12 bits
    let offset_within_page = address & (PAGE_SIZE - 1);
    // Check if the 16/32/64th byte from the current offset exceeds the page boundary
    offset_within_page < PAGE_SIZE - VECTOR_SIZE
}

#[cfg(target_arch = "x86_64")]
fn count_shared_sse2<'a, 'b>(p: &'a [u8], q: &'b [u8]) -> usize {
    use std::arch::x86_64::*;
    unsafe {
        let pl = p.len();
        let ql = q.len();
        let max_shared = pl.min(ql);
        if max_shared == 0 { return 0 }
        if same_page::<16>(p) && same_page::<16>(q) {
            let pv = _mm_loadu_si128(p.as_ptr() as _);
            let qv = _mm_loadu_si128(q.as_ptr() as _);
            let ev = _mm_cmpeq_epi8(pv, qv);
            let ne = (!_mm_movemask_epi8(ev)) as u16;
            if ne == 0 && max_shared > 16 {
                16 + count_shared_sse2(&p[16..], &q[16..])
            } else {
                (_tzcnt_u16(ne) as usize).min(max_shared)
            }
        } else {
            count_shared_reference(p, q)
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn count_shared_avx2<'a, 'b>(p: &'a [u8], q: &'b [u8]) -> usize {
    use core::arch::x86_64::*;
    unsafe {
        let pl = p.len();
        let ql = q.len();
        let max_shared = pl.min(ql);
        if max_shared == 0 { return 0 }
        if same_page::<32>(p) && same_page::<32>(q) {
            let pv = _mm256_loadu_si256(p.as_ptr() as _);
            let qv = _mm256_loadu_si256(q.as_ptr() as _);
            let ev = _mm256_cmpeq_epi8(pv, qv);
            let ne = !(_mm256_movemask_epi8(ev) as u32);
            if ne == 0 && max_shared > 32 {
                32 + count_shared_avx2(&p[32..], &q[32..])
            } else {
                (_tzcnt_u32(ne) as usize).min(max_shared)
            }
        } else {
            count_shared_reference(p, q)
        }
    }
}

/* Only takes 59% the time to run compared to count_shared_avx2
#[cfg(target_arch = "x86_64")]
fn count_shared_avx512<'a, 'b>(p: &'a [u8], q: &'b [u8]) -> usize {
    use core::arch::x86_64::*;
    unsafe {
        let pl = p.len();
        let ql = q.len();
        let max_shared = pl.min(ql);
        if same_page::<64>(p) && same_page::<64>(q) && max_shared != 0 {
            let pv = _mm512_loadu_si512(p.as_ptr() as _);
            let qv = _mm512_loadu_si512(q.as_ptr() as _);
            let ne = !_mm512_cmpeq_epi8_mask(pv, qv);
            if ne == 0 && max_shared > 64 {
                64 + count_shared_avx512(&p[64..], &q[64..])
            } else {
                (_tzcnt_u64(ne) as usize).min(max_shared)
            }
        } else {
            count_shared_reference(p, q)
        }
    }
}
*/

fn setup() -> Vec<(*const [u8], *const [u8])> {
    let max_len_sqrt = 20;
    let rng = StdRng::from_seed([0; 32]);
    let rng_ = StdRng::from_seed([!0; 32]);
    let path_fuzzer = Repeated {
        lengthd: Mapped{ d: Triangular::new(0f32, max_len_sqrt as f32,  8f32).unwrap(), f: |x| (x*x) as usize, pd: PhantomData::default() },
        itemd: Categorical { elements: { let mut v = vec![]; for i in 0..256 { v.push(i as u8) }; v },
            ed: Mapped { d: Exp::new(0.9f32).unwrap(), f: |x| (x as usize).min(255), pd: PhantomData::default() } }, pd: Default::default() };

    // let pairs = path_fuzzer.clone().sample_iter(rng).zip(path_fuzzer.clone().sample_iter(rng_)).take(TO_TEST).map(|(x, y)| (x.leak() as *const [u8], y.leak() as *const [u8])).collect::<Vec<_>>();
    let mut vec = Vec::with_capacity(TO_TEST*(max_len_sqrt*max_len_sqrt + 1));
    let pairs = path_fuzzer.clone().sample_iter(rng).zip(path_fuzzer.clone().sample_iter(rng_)).take(TO_TEST).map(|(x, y)| {
        let vl0 = vec.len();
        vec.extend_from_slice(&x[..]);
        let vl1 = vec.len();
        vec.extend_from_slice(&x[..]);
        let vl2 = vec.len();
        (&vec[vl0..vl1] as *const [u8], &vec[vl1..vl2] as *const [u8])
    }).collect::<Vec<_>>();
    std::hint::black_box(vec.leak());
    pairs
}

#[divan::bench()]
fn common_prefix_reference(bencher: Bencher) {
    let pairs = setup(); 

    pairs.iter().for_each(|(l, r)| {
        let l = unsafe { l.as_ref().unwrap() }; let r = unsafe { r.as_ref().unwrap() };
        let cnt = count_shared_reference(l, r);
        assert_eq!(&l[..cnt], &r[..cnt]);
        assert!(l.len() <= cnt || r.len() <= cnt || l[cnt] != r[cnt], "{l:?} {r:?} {:?}", cnt);
    });
    // println!("all tested reference");

    bencher.bench_local(|| {
        pairs.iter().for_each(|(l, r)| {
            let l = unsafe { l.as_ref().unwrap() }; let r = unsafe { r.as_ref().unwrap() };
            std::hint::black_box(count_shared_reference(&l[..], &r[..]));
        });
    });
}

#[divan::bench()]
fn common_prefix_bare(bencher: Bencher) {
    let pairs = setup();

    pairs.iter().for_each(|(l, r)| {
        let l = unsafe { l.as_ref().unwrap() }; let r = unsafe { r.as_ref().unwrap() };
        let cnt = count_shared_bare(l, r);
        assert_eq!(&l[..cnt], &r[..cnt]);
        assert!(l.len() <= cnt || r.len() <= cnt || l[cnt] != r[cnt], "{l:?} {r:?} {:?}", cnt);
    });
    // println!("all tested reference");

    bencher.bench_local(|| {
        pairs.iter().for_each(|(l, r)| {
            let l = unsafe { l.as_ref().unwrap() }; let r = unsafe { r.as_ref().unwrap() };
            std::hint::black_box(count_shared_bare(&l[..], &r[..]));
        });
    });
}

#[cfg(target_arch = "x86_64")]
#[divan::bench()]
fn common_prefix_sse2(bencher: Bencher) {
    let pairs = setup();

    pairs.iter().for_each(|(l, r)| {
        let l = unsafe { l.as_ref().unwrap() }; let r = unsafe { r.as_ref().unwrap() };
        let cnt = count_shared_sse2(l, r);
        assert_eq!(&l[..cnt], &r[..cnt]);
        assert!(l.len() <= cnt || r.len() <= cnt || l[cnt] != r[cnt], "{l:?} {r:?} {:?}", cnt);
    });
    println!("all tested sse2");

    bencher.bench_local(|| {
        pairs.iter().for_each(|(l, r)| {
            let l = unsafe { l.as_ref().unwrap() }; let r = unsafe { r.as_ref().unwrap() };
            std::hint::black_box(count_shared_sse2(&l[..], &r[..]));
        });
    });
}

#[cfg(target_arch = "x86_64")]
#[divan::bench()]
fn common_prefix_avx2(bencher: Bencher) {
    let pairs = setup();

    pairs.iter().for_each(|(l, r)| {
        let l = unsafe { l.as_ref().unwrap() }; let r = unsafe { r.as_ref().unwrap() };
        let cnt = count_shared_avx2(l, r);
        assert_eq!(&l[..cnt], &r[..cnt]);
        assert!(l.len() <= cnt || r.len() <= cnt || l[cnt] != r[cnt], "{l:?} {r:?} {:?}", cnt);
    });
    println!("all tested avx2");

    bencher.bench_local(|| {
        pairs.iter().for_each(|(l, r)| {
            let l = unsafe { l.as_ref().unwrap() }; let r = unsafe { r.as_ref().unwrap() };
            std::hint::black_box(count_shared_avx2(&l[..], &r[..]));
        });
    });
}

#[divan::bench()]
fn common_prefix_default(bencher: Bencher) {
    let pairs = setup();

    pairs.iter().for_each(|(l, r)| {
        let l = unsafe { l.as_ref().unwrap() }; let r = unsafe { r.as_ref().unwrap() };
        let cnt = find_prefix_overlap(l, r);
        assert_eq!(&l[..cnt], &r[..cnt]);
        assert!(l.len() <= cnt || r.len() <= cnt || l[cnt] != r[cnt], "{l:?} {r:?} {:?}", cnt);
    });

    bencher.bench_local(|| {
        pairs.iter().for_each(|(l, r)| {
            let l = unsafe { l.as_ref().unwrap() }; let r = unsafe { r.as_ref().unwrap() };
            std::hint::black_box(find_prefix_overlap(&l[..], &r[..]));
        });
    });
}

fn main() {
    // Run registered benchmarks.
    let divan = Divan::from_args()
        .sample_count(100);

    divan.main();
}
