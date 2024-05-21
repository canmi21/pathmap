use std::alloc::{alloc, dealloc, Layout};
use std::{mem, ptr};
use std::cell::{RefCell, UnsafeCell};
use std::collections::hash_map::Values;
use std::ffi::{c_void, CString};
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::ops::Not;
use std::ptr::{slice_from_raw_parts, slice_from_raw_parts_mut};
use std::thread::Thread;
use libc::exit;


impl <V : Debug> Debug for ByteTrieNodePtr<V> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        unsafe {
        let m = &*load_mask(*self);
        // let l = Utils::len(m);
        write!(f,
               "Node(mask: {:b} {:b} {:b} {:b}, values: {:?}) @ {}-{}",
               m[0], m[1], m[2], m[3],
               load_values(*self),
               self.thread, self.index)
        }
    }
}

pub struct BytesTrieMapIter<'a, V> {
    prefix: Vec<u8>,
    btnis: Vec<ByteTrieNodeIter<'a, CoFree<V>>>,
}

impl <'a, V> BytesTrieMapIter<'a, V> {
    fn new(btm: ByteTrieNodePtr<CoFree<V>>) -> Self {
        Self {
            prefix: vec![],
            btnis: vec![ByteTrieNodeIter::new(btm)],
        }
    }
}

impl <'a, V : Clone + 'a> Iterator for BytesTrieMapIter<'a, V> {
    type Item = (Vec<u8>, &'a V);

    fn next(&mut self) -> Option<(Vec<u8>, &'a V)> {
        loop {
            match self.btnis.last_mut() {
                None => { return None }
                Some(mut last) => {
                    let n: Option<(u8, &'a CoFree<V>)> = last.next();
                    match n {
                        None => {
                            self.prefix.pop();
                            self.btnis.pop();
                        }
                        Some((b, cf)) => {
                            let mut k = self.prefix.clone();
                            k.push(b);

                            if cf.rec.index != 0 {
                                self.prefix = k.clone();
                                self.btnis.push(ByteTrieNodeIter::new(cf.rec));
                            }

                            match cf.value.as_ref() {
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
    pub(crate) rec: ByteTrieNodePtr<CoFree<V>>,
    pub(crate) value: Option<V>
}

#[derive(Eq, PartialEq)]
pub struct ByteTrieNodePtr<T> {
    thread: u8,
    size: u8,
    pub index: u32,
    pd: PhantomData<T>
}

impl <T> Clone for ByteTrieNodePtr<T> {
    fn clone(&self) -> Self {
        Self {
            thread: self.thread,
            size: self.size,
            index: self.index,
            pd: PhantomData::default()
        }
    }
}

impl <T> Copy for ByteTrieNodePtr<T> {}

impl <T> ByteTrieNodePtr<T> {
    pub(crate) fn null() -> ByteTrieNodePtr<T> {
        ID_CNT.with(|R| {
            let (id, _) = unsafe { *R.get() };
            return ByteTrieNodePtr{ thread: id, size: 0, index: 0, pd: PhantomData::default() }
        })
    }
}
#[inline(always)]
fn threadid(thread: Thread) -> u64 {
    let name = format!("{:?}", thread.id());
    let relevant = &name[9..name.len()-1];
    let int = relevant.parse();
    int.unwrap()
}


pub fn register() {
    let id = threadid(std::thread::current());
    unsafe {
    let mut l = REGISTRY.lock().unwrap();
    let mut k: u32 = l.not().trailing_zeros();
    *l ^= 1u128 << k;
    println!("alloc {:?} {l} id={k}", id);
    ID_CNT.with(|mut R| {
        *R.get() = (k as u8, 1);
    })
    }
}

pub fn unregister() {
    unsafe {
    ID_CNT.with(|mut R| {
        let (id, CNT) = unsafe { *R.get() };
        // madvise(addr, length, MADV_DONTNEED) != 0
        let mut l = REGISTRY.lock().unwrap();
        *l ^= 1u128 << id;
        println!("dealloc {:?}, id={id},cnt={CNT}", threadid(std::thread::current()));
    });
    }
}

pub static mut MMAP: *mut u8 = ptr::null_mut();
static mut REGISTRY: std::sync::Mutex<u128> = std::sync::Mutex::new(0);

// pub static mut MEM1: *mut u64 = 0u64 as *mut u64;
thread_local!(pub static ID_CNT: UnsafeCell<(u8, u32)> = UnsafeCell::new((1, 1)));

pub fn init(threaded: bool) {
    unsafe {
        MMAP = libc::mmap(if threaded { (1u64 << 48) as *mut c_void } else { ptr::null_mut() },
                          (if threaded { 128 } else { 1 })*(1 << 32)*4096,
                          libc::PROT_READ|libc::PROT_WRITE,
                          libc::MAP_SHARED|libc::MAP_ANONYMOUS|libc::MAP_NORESERVE,
                          -1,
                          0) as *mut u8;
        if MMAP as i64 == -1 {
            let errno = *libc::__errno_location();
            let errstr = CString::from_raw(libc::strerror(errno));
            println!("Failed to create mmap: {}", errstr.to_str().unwrap());
            exit(-1);
        }
    }
    println!("mmap {:?}", unsafe { MMAP });
}

pub fn store_new<T>() -> ByteTrieNodePtr<T> {
    unsafe {
        let s = (256*mem::size_of::<T>()/8) as u32;
        let mut thread = 0;
        let mut i = 0;
        ID_CNT.with(|mut R| {
            let (id, CNT) = unsafe { *R.get() };
            thread = id;
            i = CNT;
            *R.get() = (id, CNT + 2);
        });

        return ByteTrieNodePtr{ thread: thread, size: 0, index: i, pd: PhantomData::default() };
    }
}

pub fn mmap_ptr(id: u8, cnt: u32) -> *mut u8 {
    unsafe { MMAP.offset((((id as isize) << 32) | (cnt as isize)) << 12) }
}

pub fn store_prepared<T>(mask: [u64; 4]) -> ByteTrieNodePtr<T> {
    unsafe {
        let mut thread = 0;
        let mut i = 0;
        let l = Utils::len(&mask);

        ID_CNT.with(|mut R| {
            let (id, CNT) = unsafe { *R.get() };
            let mut MEM: *mut u64 = mmap_ptr(id, CNT) as *mut u64;
            MEM.write(mask[0]);
            MEM.offset(1).write(mask[1]);
            MEM.offset(2).write(mask[2]);
            MEM.offset(3).write(mask[3]);
            thread = id;
            i = CNT as u32;
            *R.get() = (id, CNT + 2);
        });
        return ByteTrieNodePtr{ thread: thread, size: l as u8, index: i, pd: PhantomData::default() };
    }
}

pub fn store<T>(mask: [u64; 4], values: *mut T) -> ByteTrieNodePtr<T> {
    unsafe {
        let mut thread = 0;
        let mut i = 0;
        let l = Utils::len(&mask);
        let s = (l*mem::size_of::<T>()/8) as u32;

        ID_CNT.with(|mut R| {
            let (id, CNT) = unsafe { *R.get() };
            let mut MEM: *mut u64 = mmap_ptr(id, CNT) as *mut u64;
            MEM.write(mask[0]);
            MEM.offset(1).write(mask[1]);
            MEM.offset(2).write(mask[2]);
            MEM.offset(3).write(mask[3]);
            ptr::copy_nonoverlapping::<u64>(values as *mut u64, MEM.offset(4), s as usize);
            thread = id;
            i = CNT as u32;
            *R.get() = (id, CNT + 2);
        });
        // CNT += 4 + s as isize;
        // CNT += 4 + 256*(mem::size_of::<T>()/8) as isize;
        return ByteTrieNodePtr{ thread: thread, size: l as u8, index: i, pd: PhantomData::default() };
    }
}

pub fn load_mask<T>(node: ByteTrieNodePtr<T>) -> *mut [u64; 4] {
    unsafe {
        ID_CNT.with(|mut R| {
            let (id, _) = unsafe { *R.get() };
            mmap_ptr(id, node.index) as *mut [u64; 4]
        })
    }
}

pub fn load_values<T>(node: ByteTrieNodePtr<T>) -> *mut T {
    unsafe {
        ID_CNT.with(|mut R| {
            let (id, _) = unsafe { *R.get() };
            mmap_ptr(id, node.index).byte_offset(32) as *mut T
        })
    }
}


impl <V : Clone + Debug> ByteTrieNodePtr<CoFree<V>> {
    pub fn new() -> Self {
        store_new()
    }

    // pub fn at(self, k: &[u8]) -> Option<ByteTrieNodePtr<V>> {
    //     unsafe {
    //         let mut node = self;
    //
    //         if k.len() > 1 {
    //             for i in 0..k.len() - 1 {
    //                 match Utils::get(&*load_mask(node), load_values(node), k[i]) {
    //                     Some(cf) => {
    //                         if cf.is_null() { return None } else { node = (*cf).rec }
    //                     }
    //                     None => { return None }
    //                 }
    //             }
    //         }
    //
    //         match Utils::get(&*load_mask(node), load_values(node), k[k.len() - 1]) {
    //             None => { None }
    //             Some(CoFree { rec: r, value: _ }) => {
    //                 if r.is_null() { None } else { Some(r) }
    //             }
    //         }
    //     }
    // }

    pub fn items<'a>(&'a self) -> impl Iterator<Item=(Vec<u8>, &'a V)> + 'a {
        BytesTrieMapIter::new(*self)
    }

    pub fn contains(self, k: &[u8]) -> bool {
        self.get(k).is_some()
    }

    pub fn insert(self, k: &[u8], v: V) -> bool {
        unsafe {
            // println!("insert {:?} {:?}", k, v);
            assert!(k.len() >= 0);
            let mut node = self;

            if k.len() > 1 {
                for i in 0..k.len() - 1 {
                    // println!("at key {i} {}", k[i]);
                    let cf = &mut *Utils::update(&mut *load_mask(node), load_values(node), k[i], || CoFree { rec: ByteTrieNodePtr::null(), value: None });

                    node = if cf.rec.index != 0 { cf.rec } else {
                        let ptr = store_new();
                        // println!("created new {} {:?}", ptr.index, ptr);
                        cf.rec = ptr;
                        ptr
                    }
                }
            }

            let lk = k[k.len() - 1];
            let m = &mut *load_mask(node);
            let vs = load_values(node);
            match Utils::get(m, vs, lk) {
                Some(cf) => {
                    match (*cf).value {
                        None => {
                            (*cf).value = Some(v);
                            false
                        }
                        Some(_) => {
                            true
                        }
                    }
                }
                None => {
                    let cf = CoFree { rec: ByteTrieNodePtr::null(), value: Some(v) };
                    Utils::insert(m, vs, lk, cf)
                }
            }
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

        pub fn update<F: FnOnce() -> V>(self, k: &[u8], default: F) -> &mut V {
            unsafe {
            assert!(k.len() >= 0);
            let mut node = self;

            if k.len() > 1 {
                for i in 0..k.len() - 1 {
                    unsafe {
                        let cf = &mut *Utils::update(&mut *load_mask(node), load_values(node), k[i], || CoFree { rec: ByteTrieNodePtr::null(), value: None });

                        node = if cf.rec.index != 0 { cf.rec } else {
                            let ptr = store_new();
                            // println!("created new {} {:?}", ptr.index, ptr);
                            cf.rec = ptr;
                            ptr
                        }
                    }
                }
            }

            let lk = k[k.len() - 1];
            let cf = Utils::update(&mut *load_mask(node), load_values(node), lk, || CoFree { rec: ByteTrieNodePtr::null(), value: None }).as_mut().unwrap();
            cf.value.get_or_insert_with(default)
            }
        }

        pub fn get(self, k: &[u8]) -> Option<&V> {
            unsafe {
            let mut node = self;

            if k.len() > 1 {
                for i in 0..k.len() - 1 {
                    match Utils::get(&*load_mask(node), load_values(node), k[i]) {
                        Some(cf) => {
                            if (*cf).rec.index != 0 { node = (*cf).rec } else { return None }
                        }
                        None => { return None }
                    }
                }
            }

            match Utils::get(&*load_mask(node), load_values(node), k[k.len() - 1]) {
                None => { None }
                Some(cf) => {
                    match cf.as_ref().unwrap().value.as_ref() {
                        None => { None }
                        Some(v) => { Some(v) }
                    }
                }
            }
            }
        }

        pub fn len(self) -> usize {
            unsafe {
            return (*slice_from_raw_parts(load_values(self), Utils::len(&*load_mask(self)))).iter().rfold(0, |t, cf| unsafe {
                t + cf.value.is_some() as usize + (if cf.rec.index == 0 { 0 } else { cf.rec.len() })
            });
            }
        }
    }


// pub struct ShortTrieMap<V> {
//     pub(crate) root: ByteTrieNode<*mut ByteTrieNode<V>>
// }
//
// impl <V : Clone> FromIterator<(u16, V)> for ShortTrieMap<V> {
//     fn from_iter<I: IntoIterator<Item=(u16, V)>>(iter: I) -> Self {
//         let mut tm = ShortTrieMap::new();
//         for (k, v) in iter { tm.insert(k, v); }
//         tm
//     }
// }
//
// impl <V : Clone> ShortTrieMap<V> {
//     pub fn new() -> Self {
//         Self {
//             root: ByteTrieNode::new()
//         }
//     }
//
//     pub fn items<'a>(&'a self) -> impl Iterator<Item=(u16, V)> + 'a {
//         self.root.items().flat_map(move |(k1, l1)| unsafe {
//             (*l1).items().map(move |(k2, v)| ((k1 as u16) | ((k2 as u16) << 8), v))
//         })
//     }
//
//     pub fn contains(&self, k: u16) -> bool {
//         let k1 = k as u8;
//         let k2 = (k >> 8) as u8;
//         if self.root.contains(k1) {
//             let l1 = self.root.get_unchecked(k1);
//             let rl1 = unsafe { &**l1 };
//             rl1.contains(k2)
//         } else {
//             false
//         }
//     }
//
//     pub fn insert(&mut self, k: u16, v: V) -> bool {
//         let k1 = k as u8;
//         let k2 = (k >> 8) as u8;
//         if self.root.contains(k1) {
//             let l1 = self.root.get_unchecked(k1);
//             let rl1 = unsafe { &mut **l1 };
//             rl1.insert(k2, v)
//         } else {
//             let mut l1 = ByteTrieNode::new();
//             l1.insert(k2, v);
//             let mut rl1 = Box::new(l1);
//             self.root.insert(k1, rl1.as_mut());
//             mem::forget(rl1);
//             false
//         }
//     }
//
//     pub fn remove(&mut self, k: u16) -> Option<V> {
//         let k1 = k as u8;
//         let k2 = (k >> 8) as u8;
//         match self.root.get(k1) {
//             Some(btn) => {
//                 let btnr = unsafe { &mut **btn };
//                 let r = btnr.remove(k2);
//                 if btnr.len() == 0 {
//                     self.root.remove(k1);
//                     unsafe { dealloc(ptr::from_mut(btnr).cast(), Layout::new::<ByteTrieNode<V>>()); }
//                 }
//                 r
//             }
//             None => None
//         }
//     }
//
//     pub fn deepcopy(&self) -> Self {
//         return self.items().collect();
//     }
//
//     pub fn get(&self, k: u16) -> Option<&V> {
//         let k1 = k as u8;
//         let k2 = (k >> 8) as u8;
//         self.root.get(k1).and_then(|l1| {
//             let rl1 = unsafe { &**l1 };
//             rl1.get(k2)
//         })
//     }
// }
//
pub struct ByteTrieNodeIter<'a, V> {
    i: u8,
    w: u64,
    m: &'a [u64; 4],
    v: *mut V
}

impl <'a, V> ByteTrieNodeIter<'a, V> {
    fn new(btn: ByteTrieNodePtr<V>) -> Self {
        let m = load_mask(btn);
        Self {
            i: 0,
            w: unsafe { (*m)[0] },
            m: unsafe { &*m },
            v: load_values(btn)
        }
    }
}

impl <'a, V : Clone + 'a> Iterator for ByteTrieNodeIter<'a, V> {
    type Item = (u8, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.w != 0 {
                let wi = self.w.trailing_zeros() as u8;
                self.w ^= 1u64 << wi;
                let index = self.i*64 + wi;
                return Some((index, unsafe { &*Utils::get(self.m, self.v, index).unwrap() }))
            } else if self.i < 3 {
                self.i += 1;
                self.w = unsafe { *self.m.get_unchecked(self.i as usize) };
            } else {
                return None
            }
        }
    }
}

struct Utils {}

impl Utils {
    #[inline]
    pub fn contains(mask: &[u64; 4], k: u8) -> bool {
        0 != (mask[((k & 0b11000000) >> 6) as usize] & (1u64 << (k & 0b00111111)))
    }

    #[inline]
    pub fn left(mask: &[u64; 4], pos: u8) -> u8 {
        if pos == 0 { return 0 }
        let mut c = 0u8;
        let m = !0u64 >> (63 - ((pos - 1) & 0b00111111));
        if pos > 0b01000000 { c += mask[0].count_ones() as u8; }
        else { return c + (mask[0] & m).count_ones() as u8 }
        if pos > 0b10000000 { c += mask[1].count_ones() as u8; }
        else { return c + (mask[1] & m).count_ones() as u8 }
        if pos > 0b11000000 { c += mask[2].count_ones() as u8; }
        else { return c + (mask[2] & m).count_ones() as u8 }
        // println!("{} {:b} {} {}", pos, self.mask[3], m.count_ones(), c);
        return c + (mask[3] & m).count_ones() as u8;
    }

    #[inline]
    pub fn len(mask: &[u64; 4]) -> usize {
        return (mask[0].count_ones() + mask[1].count_ones() + mask[2].count_ones() + mask[3].count_ones()) as usize;
    }

    #[inline]
    fn set(mask: &mut [u64; 4], k: u8) -> () {
        mask[((k & 0b11000000) >> 6) as usize] |= 1u64 << (k & 0b00111111);
    }

    #[inline]
    fn clear(mask: &mut [u64; 4], k: u8) -> () {
        mask[((k & 0b11000000) >> 6) as usize] &= !(1u64 << (k & 0b00111111));
    }

    pub fn insert<V : Clone>(mask: &mut [u64; 4], values: *mut V, k: u8, v: V) -> bool {
        let index = Self::left(mask, k) as usize;
        if Self::contains(mask, k) {
            // println!("overwriting {index}");
            unsafe {
                let ptr = values.add(index);
                *ptr = v;
            }
            true
        } else {
            unsafe {
                let p = values.add(index);
                let len = Self::len(mask);
                // let len = 256;
                // // println!("insert at {k} (index={index},len={len})");
                // if index != len - 1 {
                //     // ptr::copy(p, p.add(1), (len + 1) - index);
                //     let mut i = len;
                //     while i != index {
                //         // println!("mov {} to {}", i - 1, i);
                //         mem::swap(&mut *values.add(i), &mut *values.add(i - 1));
                //         // ptr::write::<V>(values.add(i), (*values.add(i - 1)).clone());
                //         // *values.add(i) = (*values.add(i - 1)).clone();
                //         i -= 1;
                //     }
                // }
                if index < len {
                    // println!("k={} index={} len={}", k, index, len);
                    ptr::copy(p, p.add(1), len - index);
                }
                // println!("write {index}");
                ptr::write(p, v);
            }
            Self::set(mask, k);
            false
        }
    }

    pub fn update<V, F : FnOnce() -> V>(mask: &mut [u64; 4], values: *mut V, k: u8, default: F) -> *mut V {
        let index = Self::left(mask, k) as usize;
        if !Self::contains(mask, k) {
            let len = Self::len(mask);
            unsafe {
                let p = values.add(index);
                if index < len {
                    ptr::copy(p, p.add(1), len - index);
                }
                ptr::write(p, default());
            }
            Self::set(mask, k);
        }
        unsafe {
            values.add(index)
        }
    }

    pub fn get<V>(mask: &[u64; 4], values: *mut V, k: u8) -> Option<*mut V> {
        if Self::contains(mask, k) {
            let ix = Self::left(mask, k) as usize;
            // println!("pos ix {} {} {:b}", pos, ix, self.mask);
            return unsafe { Some(values.add(ix)) };
        };
        return None;
    }
}
