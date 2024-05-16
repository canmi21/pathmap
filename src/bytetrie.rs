use std::alloc::{dealloc, Layout};
use std::{mem, ptr};
use std::arch::x86_64::{__m256, __m256i, _mm256_and_si256, _mm256_load_si256, _mm256_loadu_si256, _mm256_permute4x64_epi64, _mm256_popcnt_epi64, _mm256_set_epi64x};
use std::fmt::{Debug, Formatter};
use std::intrinsics::black_box;
use std::intrinsics::simd::simd_reduce_add_unordered;
use ethnum::*;
use crate::ring::u64s;


#[derive(Clone)]
#[repr(C, align(64))]
pub struct ByteTrieNode<V> {
    pub(crate) mask: __m256i,
    // pub(crate) mask: [u64; 4],
    pub(crate) values: Vec<V>
}

impl <V : Debug> Debug for ByteTrieNode<V> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f,
               "Node(mask: {:b} {:b} {:b} {:b}, values: {:?})",
               self.mask.u64(0), self.mask.u64(1), self.mask.u64(2), self.mask.u64(3),
               self.values)
    }
}

pub struct BytesTrieMapIter<'a, V> {
    prefix: Vec<u8>,
    btnis: Vec<ByteTrieNodeIter<'a, CoFree<V>>>,
}

impl <'a, V> BytesTrieMapIter<'a, V> {
    fn new(btm: &'a BytesTrieMap<V>) -> Self {
        Self {
            prefix: vec![],
            btnis: vec![ByteTrieNodeIter::new(&btm.root)],
        }
    }
}

impl <'a, V : Clone> Iterator for BytesTrieMapIter<'a, V> {
    type Item = (Vec<u8>, V);

    fn next(&mut self) -> Option<(Vec<u8>, V)> {
        loop {
            match self.btnis.last_mut() {
                None => { return None }
                Some(mut last) => {
                    match last.next() {
                        None => {
                            self.prefix.pop();
                            self.btnis.pop();
                        }
                        Some((b, cf)) => {
                            let mut k = self.prefix.clone();
                            k.push(b);

                            match unsafe { cf.rec.as_ref() } {
                                None => {}
                                Some(rec) => {
                                    self.prefix = k.clone();
                                    self.btnis.push(ByteTrieNodeIter::new(rec));
                                }
                            }

                            match cf.value {
                                None => {}
                                Some(v) => {
                                    return Some((k, v))
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CoFree<V> {
    pub(crate) rec: *mut ByteTrieNode<CoFree<V>>,
    pub(crate) value: Option<V>
}

pub struct BytesTrieMap<V> {
    pub root: ByteTrieNode<CoFree<V>>
}

impl <V : Clone> BytesTrieMap<V> {
    pub fn new() -> Self {
        Self {
            root: ByteTrieNode::new()
        }
    }

    pub fn at(&self, k: &[u8]) -> Option<&mut BytesTrieMap<V>> {
        let mut node = &self.root;

        if k.len() > 1 {
        for i in 0..k.len() - 1 {
            match node.get(k[i]) {
                Some(cf) => {
                    match unsafe { cf.rec.as_ref() } {
                        Some(r) => { node = r }
                        None => { return None }
                    }
                }
                None => { return None }
            }
        }
        }

        match node.get(k[k.len() - 1]) {
            None => { None }
            Some(CoFree{ rec: r, value: _ }) => {
                if r.is_null() { None }
                else { unsafe { Some((*r as *mut BytesTrieMap<V>).as_mut().unwrap_unchecked()) } }
            }
        }
    }

    pub fn items<'a>(&'a self) -> impl Iterator<Item=(Vec<u8>, V)> + 'a {
        BytesTrieMapIter::new(self)
    }

    pub fn contains(&self, k: &[u8]) -> bool {
        self.get(k).is_some()
    }

    pub fn insert(&mut self, k: &[u8], v: V) -> bool {
        assert!(k.len() >= 0);
        let mut node = &mut self.root;

        if k.len() > 1 {
        for i in 0..k.len() - 1 {
            unsafe {
            let cf = node.update(k[i], || CoFree{rec: ptr::null_mut(), value: None});

            node = match cf.rec.as_mut() {
                Some(r) => { r }
                None => {
                    let l = ByteTrieNode::new();
                    let mut rl = Box::new(l);
                    let ptr: *mut ByteTrieNode<CoFree<V>> = rl.as_mut();
                    mem::forget(rl);
                    cf.rec = ptr;
                    ptr.as_mut().unwrap()
                }
            }
            }
        }
        }

        let lk = k[k.len() - 1];
        if node.contains(lk) {
            let cf = node.get_unchecked_mut(lk);
            match cf.value {
                None => {
                    cf.value = Some(v);
                    false
                }
                Some(_) => {
                    true
                }
            }
        } else {
            let cf = CoFree{ rec: ptr::null_mut() , value: Some(v) };
            node.insert(lk, cf)
        }
    }

    // pub fn remove(&mut self, k: u16) -> Option<V> {
    //     let k1 = k as u8;
    //     let k2 = (k >> 8) as u8;
    //     match self.root.get(k1) {
    //         Some(btn) => {
    //             let btnr = unsafe { &mut **btn };
    //             let r = btnr.remove(k2);
    //             if btnr.len() == 0 {
    //                 self.root.remove(k1);
    //                 unsafe { dealloc(ptr::from_mut(btnr).cast(), Layout::new::<ByteTrieNode<V>>()); }
    //             }
    //             r
    //         }
    //         None => None
    //     }
    // }

    // pub fn deepcopy(&self) -> Self {
    //     return self.items().collect();
    // }

    pub fn update<F : FnOnce() -> V>(&mut self, k: &[u8], default: F) -> &mut V {
        assert!(k.len() >= 0);
        let mut node = &mut self.root;

        if k.len() > 1 {
        for i in 0..k.len() - 1 {
            unsafe {
            let cf = node.update(k[i], || CoFree{rec: ptr::null_mut(), value: None});

            node = match cf.rec.as_mut() {
                Some(r) => { r }
                None => {
                    let l = ByteTrieNode::new();
                    let mut rl = Box::new(l);
                    let ptr: *mut ByteTrieNode<CoFree<V>> = rl.as_mut();
                    mem::forget(rl);
                    cf.rec = ptr;
                    ptr.as_mut().unwrap()
                }
            }
            }
        }
        }

        let lk = k[k.len() - 1];
        let cf = node.update(lk, || CoFree{ rec: ptr::null_mut() , value: None });
        cf.value.get_or_insert_with(default)
    }

    pub fn get(&self, k: &[u8]) -> Option<&V> {
        let mut node = &self.root;

        if k.len() > 1 {
            for i in 0..k.len() - 1 {
                match node.get(k[i]) {
                    Some(cf) => {
                        match unsafe { cf.rec.as_ref() } {
                            Some(r) => { node = r }
                            None => { return None }
                        }
                    }
                    None => { return None }
                }
            }
        }

        match node.get(k[k.len() - 1]) {
            None => { None }
            Some(CoFree{ rec: _, value }) => {
                match value {
                    None => { None }
                    Some(v) => { Some(v) }
                }
            }
        }
    }

    fn cofreelen(btn: &ByteTrieNode<CoFree<V>>) -> usize {
        return btn.values.iter().rfold(0, |t, cf| unsafe {
            t + cf.value.is_some() as usize + cf.rec.as_ref().map(|r| Self::cofreelen(r)).unwrap_or(0)
        });
    }

    pub fn len(&self) -> usize {
        return Self::cofreelen(&self.root);
    }
}


pub struct ShortTrieMap<V> {
    pub(crate) root: ByteTrieNode<*mut ByteTrieNode<V>>
}

impl <V : Clone> FromIterator<(u16, V)> for ShortTrieMap<V> {
    fn from_iter<I: IntoIterator<Item=(u16, V)>>(iter: I) -> Self {
        let mut tm = ShortTrieMap::new();
        for (k, v) in iter { tm.insert(k, v); }
        tm
    }
}

impl <V : Clone> ShortTrieMap<V> {
    pub fn new() -> Self {
        Self {
            root: ByteTrieNode::new()
        }
    }

    pub fn items<'a>(&'a self) -> impl Iterator<Item=(u16, V)> + 'a {
        self.root.items().flat_map(move |(k1, l1)| unsafe {
            (*l1).items().map(move |(k2, v)| ((k1 as u16) | ((k2 as u16) << 8), v))
        })
    }

    pub fn contains(&self, k: u16) -> bool {
        let k1 = k as u8;
        let k2 = (k >> 8) as u8;
        if self.root.contains(k1) {
            let l1 = self.root.get_unchecked(k1);
            let rl1 = unsafe { &**l1 };
            rl1.contains(k2)
        } else {
            false
        }
    }

    pub fn insert(&mut self, k: u16, v: V) -> bool {
        let k1 = k as u8;
        let k2 = (k >> 8) as u8;
        if self.root.contains(k1) {
            let l1 = self.root.get_unchecked(k1);
            let rl1 = unsafe { &mut **l1 };
            rl1.insert(k2, v)
        } else {
            let mut l1 = ByteTrieNode::new();
            l1.insert(k2, v);
            let mut rl1 = Box::new(l1);
            self.root.insert(k1, rl1.as_mut());
            mem::forget(rl1);
            false
        }
    }

    pub fn remove(&mut self, k: u16) -> Option<V> {
        let k1 = k as u8;
        let k2 = (k >> 8) as u8;
        match self.root.get(k1) {
            Some(btn) => {
                let btnr = unsafe { &mut **btn };
                let r = btnr.remove(k2);
                if btnr.len() == 0 {
                    self.root.remove(k1);
                    unsafe { dealloc(ptr::from_mut(btnr).cast(), Layout::new::<ByteTrieNode<V>>()); }
                }
                r
            }
            None => None
        }
    }

    pub fn deepcopy(&self) -> Self {
        return self.items().collect();
    }

    pub fn get(&self, k: u16) -> Option<&V> {
        let k1 = k as u8;
        let k2 = (k >> 8) as u8;
        self.root.get(k1).and_then(|l1| {
            let rl1 = unsafe { &**l1 };
            rl1.get(k2)
        })
    }
}

pub struct ByteTrieNodeIter<'a, V> {
    i: u8,
    w: u64,
    btn: &'a ByteTrieNode<V>
}

impl <'a, V> ByteTrieNodeIter<'a, V> {
    fn new(btn: &'a ByteTrieNode<V>) -> Self {
        Self {
            i: 0,
            w: btn.mask.u64(0),
            btn: btn
        }
    }
}

impl <'a, V : Clone> Iterator for ByteTrieNodeIter<'a, V> {
    type Item = (u8, V);

    fn next(&mut self) -> Option<(u8, V)> {
        loop {
            if self.w != 0 {
                let wi = self.w.trailing_zeros() as u8;
                self.w ^= 1u64 << wi;
                let index = self.i*64 + wi;
                // TODO benchmark calculating the value index
                return Some((index, self.btn.get_unchecked(index).clone()))
            } else if self.i < 3 {
                self.i += 1;
                self.w = self.btn.mask.u64(self.i);
            } else {
                return None
            }
        }
    }
}

static ONE: u256 = U256::from_words(!0u128, !0u128);
static ma: [i64; 1024] = [
    0, 0, 0, 0,
    1, 0, 0, 0,
    3, 0, 0, 0,
    7, 0, 0, 0,
    15, 0, 0, 0,
    31, 0, 0, 0,
    63, 0, 0, 0,
    127, 0, 0, 0,
    255, 0, 0, 0,
    511, 0, 0, 0,
    1023, 0, 0, 0,
    2047, 0, 0, 0,
    4095, 0, 0, 0,
    8191, 0, 0, 0,
    16383, 0, 0, 0,
    32767, 0, 0, 0,
    65535, 0, 0, 0,
    131071, 0, 0, 0,
    262143, 0, 0, 0,
    524287, 0, 0, 0,
    1048575, 0, 0, 0,
    2097151, 0, 0, 0,
    4194303, 0, 0, 0,
    8388607, 0, 0, 0,
    16777215, 0, 0, 0,
    33554431, 0, 0, 0,
    67108863, 0, 0, 0,
    134217727, 0, 0, 0,
    268435455, 0, 0, 0,
    536870911, 0, 0, 0,
    1073741823, 0, 0, 0,
    2147483647, 0, 0, 0,
    4294967295, 0, 0, 0,
    8589934591, 0, 0, 0,
    17179869183, 0, 0, 0,
    34359738367, 0, 0, 0,
    68719476735, 0, 0, 0,
    137438953471, 0, 0, 0,
    274877906943, 0, 0, 0,
    549755813887, 0, 0, 0,
    1099511627775, 0, 0, 0,
    2199023255551, 0, 0, 0,
    4398046511103, 0, 0, 0,
    8796093022207, 0, 0, 0,
    17592186044415, 0, 0, 0,
    35184372088831, 0, 0, 0,
    70368744177663, 0, 0, 0,
    140737488355327, 0, 0, 0,
    281474976710655, 0, 0, 0,
    562949953421311, 0, 0, 0,
    1125899906842623, 0, 0, 0,
    2251799813685247, 0, 0, 0,
    4503599627370495, 0, 0, 0,
    9007199254740991, 0, 0, 0,
    18014398509481983, 0, 0, 0,
    36028797018963967, 0, 0, 0,
    72057594037927935, 0, 0, 0,
    144115188075855871, 0, 0, 0,
    288230376151711743, 0, 0, 0,
    576460752303423487, 0, 0, 0,
    1152921504606846975, 0, 0, 0,
    2305843009213693951, 0, 0, 0,
    4611686018427387903, 0, 0, 0,
    9223372036854775807, 0, 0, 0,
    -1, 0, 0, 0,
    -1, 1, 0, 0,
    -1, 3, 0, 0,
    -1, 7, 0, 0,
    -1, 15, 0, 0,
    -1, 31, 0, 0,
    -1, 63, 0, 0,
    -1, 127, 0, 0,
    -1, 255, 0, 0,
    -1, 511, 0, 0,
    -1, 1023, 0, 0,
    -1, 2047, 0, 0,
    -1, 4095, 0, 0,
    -1, 8191, 0, 0,
    -1, 16383, 0, 0,
    -1, 32767, 0, 0,
    -1, 65535, 0, 0,
    -1, 131071, 0, 0,
    -1, 262143, 0, 0,
    -1, 524287, 0, 0,
    -1, 1048575, 0, 0,
    -1, 2097151, 0, 0,
    -1, 4194303, 0, 0,
    -1, 8388607, 0, 0,
    -1, 16777215, 0, 0,
    -1, 33554431, 0, 0,
    -1, 67108863, 0, 0,
    -1, 134217727, 0, 0,
    -1, 268435455, 0, 0,
    -1, 536870911, 0, 0,
    -1, 1073741823, 0, 0,
    -1, 2147483647, 0, 0,
    -1, 4294967295, 0, 0,
    -1, 8589934591, 0, 0,
    -1, 17179869183, 0, 0,
    -1, 34359738367, 0, 0,
    -1, 68719476735, 0, 0,
    -1, 137438953471, 0, 0,
    -1, 274877906943, 0, 0,
    -1, 549755813887, 0, 0,
    -1, 1099511627775, 0, 0,
    -1, 2199023255551, 0, 0,
    -1, 4398046511103, 0, 0,
    -1, 8796093022207, 0, 0,
    -1, 17592186044415, 0, 0,
    -1, 35184372088831, 0, 0,
    -1, 70368744177663, 0, 0,
    -1, 140737488355327, 0, 0,
    -1, 281474976710655, 0, 0,
    -1, 562949953421311, 0, 0,
    -1, 1125899906842623, 0, 0,
    -1, 2251799813685247, 0, 0,
    -1, 4503599627370495, 0, 0,
    -1, 9007199254740991, 0, 0,
    -1, 18014398509481983, 0, 0,
    -1, 36028797018963967, 0, 0,
    -1, 72057594037927935, 0, 0,
    -1, 144115188075855871, 0, 0,
    -1, 288230376151711743, 0, 0,
    -1, 576460752303423487, 0, 0,
    -1, 1152921504606846975, 0, 0,
    -1, 2305843009213693951, 0, 0,
    -1, 4611686018427387903, 0, 0,
    -1, 9223372036854775807, 0, 0,
    -1, -1, 0, 0,
    -1, -1, 1, 0,
    -1, -1, 3, 0,
    -1, -1, 7, 0,
    -1, -1, 15, 0,
    -1, -1, 31, 0,
    -1, -1, 63, 0,
    -1, -1, 127, 0,
    -1, -1, 255, 0,
    -1, -1, 511, 0,
    -1, -1, 1023, 0,
    -1, -1, 2047, 0,
    -1, -1, 4095, 0,
    -1, -1, 8191, 0,
    -1, -1, 16383, 0,
    -1, -1, 32767, 0,
    -1, -1, 65535, 0,
    -1, -1, 131071, 0,
    -1, -1, 262143, 0,
    -1, -1, 524287, 0,
    -1, -1, 1048575, 0,
    -1, -1, 2097151, 0,
    -1, -1, 4194303, 0,
    -1, -1, 8388607, 0,
    -1, -1, 16777215, 0,
    -1, -1, 33554431, 0,
    -1, -1, 67108863, 0,
    -1, -1, 134217727, 0,
    -1, -1, 268435455, 0,
    -1, -1, 536870911, 0,
    -1, -1, 1073741823, 0,
    -1, -1, 2147483647, 0,
    -1, -1, 4294967295, 0,
    -1, -1, 8589934591, 0,
    -1, -1, 17179869183, 0,
    -1, -1, 34359738367, 0,
    -1, -1, 68719476735, 0,
    -1, -1, 137438953471, 0,
    -1, -1, 274877906943, 0,
    -1, -1, 549755813887, 0,
    -1, -1, 1099511627775, 0,
    -1, -1, 2199023255551, 0,
    -1, -1, 4398046511103, 0,
    -1, -1, 8796093022207, 0,
    -1, -1, 17592186044415, 0,
    -1, -1, 35184372088831, 0,
    -1, -1, 70368744177663, 0,
    -1, -1, 140737488355327, 0,
    -1, -1, 281474976710655, 0,
    -1, -1, 562949953421311, 0,
    -1, -1, 1125899906842623, 0,
    -1, -1, 2251799813685247, 0,
    -1, -1, 4503599627370495, 0,
    -1, -1, 9007199254740991, 0,
    -1, -1, 18014398509481983, 0,
    -1, -1, 36028797018963967, 0,
    -1, -1, 72057594037927935, 0,
    -1, -1, 144115188075855871, 0,
    -1, -1, 288230376151711743, 0,
    -1, -1, 576460752303423487, 0,
    -1, -1, 1152921504606846975, 0,
    -1, -1, 2305843009213693951, 0,
    -1, -1, 4611686018427387903, 0,
    -1, -1, 9223372036854775807, 0,
    -1, -1, -1, 0,
    -1, -1, -1, 1,
    -1, -1, -1, 3,
    -1, -1, -1, 7,
    -1, -1, -1, 15,
    -1, -1, -1, 31,
    -1, -1, -1, 63,
    -1, -1, -1, 127,
    -1, -1, -1, 255,
    -1, -1, -1, 511,
    -1, -1, -1, 1023,
    -1, -1, -1, 2047,
    -1, -1, -1, 4095,
    -1, -1, -1, 8191,
    -1, -1, -1, 16383,
    -1, -1, -1, 32767,
    -1, -1, -1, 65535,
    -1, -1, -1, 131071,
    -1, -1, -1, 262143,
    -1, -1, -1, 524287,
    -1, -1, -1, 1048575,
    -1, -1, -1, 2097151,
    -1, -1, -1, 4194303,
    -1, -1, -1, 8388607,
    -1, -1, -1, 16777215,
    -1, -1, -1, 33554431,
    -1, -1, -1, 67108863,
    -1, -1, -1, 134217727,
    -1, -1, -1, 268435455,
    -1, -1, -1, 536870911,
    -1, -1, -1, 1073741823,
    -1, -1, -1, 2147483647,
    -1, -1, -1, 4294967295,
    -1, -1, -1, 8589934591,
    -1, -1, -1, 17179869183,
    -1, -1, -1, 34359738367,
    -1, -1, -1, 68719476735,
    -1, -1, -1, 137438953471,
    -1, -1, -1, 274877906943,
    -1, -1, -1, 549755813887,
    -1, -1, -1, 1099511627775,
    -1, -1, -1, 2199023255551,
    -1, -1, -1, 4398046511103,
    -1, -1, -1, 8796093022207,
    -1, -1, -1, 17592186044415,
    -1, -1, -1, 35184372088831,
    -1, -1, -1, 70368744177663,
    -1, -1, -1, 140737488355327,
    -1, -1, -1, 281474976710655,
    -1, -1, -1, 562949953421311,
    -1, -1, -1, 1125899906842623,
    -1, -1, -1, 2251799813685247,
    -1, -1, -1, 4503599627370495,
    -1, -1, -1, 9007199254740991,
    -1, -1, -1, 18014398509481983,
    -1, -1, -1, 36028797018963967,
    -1, -1, -1, 72057594037927935,
    -1, -1, -1, 144115188075855871,
    -1, -1, -1, 288230376151711743,
    -1, -1, -1, 576460752303423487,
    -1, -1, -1, 1152921504606846975,
    -1, -1, -1, 2305843009213693951,
    -1, -1, -1, 4611686018427387903,
    -1, -1, -1, 9223372036854775807];

impl <V : Clone> ByteTrieNode<V> {
    pub fn new() -> Self {
        Self {
            mask: unsafe { _mm256_set_epi64x(0, 0, 0, 0) },
            // mask: [0, 0, 0, 0],
            values: Vec::new()
        }
    }

    pub fn items<'a>(&'a self) -> ByteTrieNodeIter<'a, V> {
        ByteTrieNodeIter::new(self)
    }

    #[inline]
    pub fn left(&self, pos: u8) -> u8 {
        if pos == 0 { return 0 }
        let mut c = 0u8;
        let m = !0u64 >> (63 - ((pos - 1) & 0b00111111));
        if pos > 0b01000000 { c += self.mask.u64(0).count_ones() as u8; }
        else { return c + (self.mask.u64(0) & m).count_ones() as u8 }
        if pos > 0b10000000 { c += self.mask.u64(1).count_ones() as u8; }
        else { return c + (self.mask.u64(1) & m).count_ones() as u8 }
        if pos > 0b11000000 { c += self.mask.u64(2).count_ones() as u8; }
        else { return c + (self.mask.u64(2) & m).count_ones() as u8 }
        // println!("{} {:b} {} {}", pos, s[3], m.count_ones(), c);
        return c + (self.mask.u64(3) & m).count_ones() as u8;
    }

    #[inline(always)]
    pub unsafe fn ones(x: __m256i) -> usize {
        let c = _mm256_popcnt_epi64(x);
        simd_reduce_add_unordered::<__m256i, i64>(c) as usize
    }

    // #[inline(never)]
    // pub fn left(&self, pos: u8) -> u8 {
    //     unsafe {
    //     let m = _mm256_loadu_si256((ma.as_ptr() as *const __m256i).offset(pos as isize));
    //     let r = _mm256_and_si256(self.mask, m);
    //     let c = _mm256_popcnt_epi64(r);
    //     let f = simd_reduce_add_unordered::<__m256i, i64>(c) as u8;
    //     f
    //     }
    // }

    #[inline]
    pub fn contains(&self, k: u8) -> bool {
        // let m = crate::ring::u64s::u64s(&self.mask);
        0 != (self.mask.u64((k & 0b11000000) >> 6) & (1u64 << (k & 0b00111111)))
    }

    #[inline]
    fn set(&mut self, k: u8) -> () {
        let m = crate::ring::u64s::u64s_mut(&mut self.mask);
        // println!("setting k {} : {} {:b}", k, ((k & 0b11000000) >> 6) as usize, 1u64 << (k & 0b00111111));
        m[((k & 0b11000000) >> 6) as usize] |= 1u64 << (k & 0b00111111);
    }

    #[inline]
    fn clear(&mut self, k: u8) -> () {
        let m = crate::ring::u64s::u64s_mut(&mut self.mask);
        // println!("setting k {} : {} {:b}", k, ((k & 0b11000000) >> 6) as usize, 1u64 << (k & 0b00111111));
        m[((k & 0b11000000) >> 6) as usize] &= !(1u64 << (k & 0b00111111));
    }

    pub fn insert(&mut self, k: u8, v: V) -> bool {
        let ix = self.left(k) as usize;
        // println!("{k} {ix} {:b}", self.mask.0[0]);
        if self.contains(k) {
            unsafe {
                let ptr = self.values.get_unchecked_mut(ix);
                *ptr = v;
            }
            true
        } else {
            self.set(k);
            self.values.insert(ix, v);
            false
        }
    }

    pub fn update<F : FnOnce() -> V>(&mut self, k: u8, default: F) -> &mut V {
        let ix = self.left(k) as usize;
        if !self.contains(k) {
            self.set(k);
            self.values.insert(ix, default());
        }
        unsafe {
            return self.values.get_unchecked_mut(ix);
        }
    }

    pub fn remove(&mut self, k: u8) -> Option<V> {
        if self.contains(k) {
            let v = self.values.remove(k as usize);
            self.clear(k);
            return Some(v);
        }
        None
    }

    pub fn get(&self, k: u8) -> Option<&V> {
        if self.contains(k) {
            let ix = self.left(k) as usize;
            // println!("pos ix {} {} {:b}", pos, ix, self.mask);
            return unsafe { Some(self.values.get_unchecked(ix)) };
        };
        return None;
    }

    #[inline]
    pub fn get_unchecked(&self, k: u8) -> &V {
        let ix = self.left(k) as usize;
        // println!("pos ix {} {} {:b}", pos, ix, self.mask);
        return unsafe { self.values.get_unchecked(ix) };
    }

    #[inline]
    pub fn get_unchecked_mut(&mut self, k: u8) -> &mut V {
        let ix = self.left(k) as usize;
        // println!("pos ix {} {} {:b}", pos, ix, self.mask);
        return unsafe { self.values.get_unchecked_mut(ix) };
    }

    pub fn len(&self) -> usize {
        return unsafe { Self::ones(self.mask) };
    }
}

