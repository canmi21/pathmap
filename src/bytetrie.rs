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
        let rm = &*load_rmask(*self);
        let vm = &*load_vmask(*self);
        // let l = Utils::len(m);
        write!(f,
               "Node(rmask: {:b} {:b} {:b} {:b}, rmask: {:b} {:b} {:b} {:b}, rvalues: {:?}, vvalues: {:?}) @ {}-{}",
               rm[0], rm[1], rm[2], rm[3],
               vm[0], vm[1], vm[2], vm[3],
               load_rvalues(*self),
               load_vvalues(*self),
               self.thread, self.index)
        }
    }
}

pub struct BytesTrieMapIter<'a, V> {
    prefix: Vec<u8>,
    rtrace: Vec<ByteTrieNodeIter<'a, ByteTrieNodePtr<V>>>,
    vtrace: Vec<ByteTrieNodeIter<'a, V>>,
}

impl <'a, V> BytesTrieMapIter<'a, V> {
    fn new(btm: ByteTrieNodePtr<V>) -> Self {
        unsafe {
        Self {
            prefix: vec![],
            rtrace: vec![ByteTrieNodeIter::new(&*load_rmask(btm), load_rvalues(btm))],
            vtrace: vec![ByteTrieNodeIter::new(&*load_vmask(btm), load_vvalues(btm))],
        }
        }
    }
}

// impl <'a, V : Clone + 'a> Iterator for BytesTrieMapIter<'a, V> {
//     type Item = (Vec<u8>, &'a V);
//
//     fn next(&mut self) -> Option<(Vec<u8>, &'a V)> {
//         loop {
//             match self.btnis.last_mut() {
//                 None => { return None }
//                 Some(mut last) => {
//                     let n: Option<(u8, &'a V)> = last.next();
//                     match n {
//                         None => {
//                             self.prefix.pop();
//                             self.btnis.pop();
//                         }
//                         Some((b, cf)) => {
//                             let mut k = self.prefix.clone();
//                             k.push(b);
//
//                             if cf.rec.index != 0 {
//                                 self.prefix = k.clone();
//                                 self.btnis.push(ByteTrieNodeIter::new(cf.rec));
//                             }
//
//                             match cf.value.as_ref() {
//                                 None => {}
//                                 Some(v) => {
//                                     return Some((k, v))
//                                 }
//                             }
//                         }
//                     }
//                 }
//             }
//         }
//     }
// }

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
    // #[inline(never)]
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
    // println!("alloc {:?} {l} id={k}", id);
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
        // println!("dealloc {:?}, id={id},cnt={CNT}", threadid(std::thread::current()));
    });
    }
}

pub static mut MMAP: *mut u8 = ptr::null_mut();
static mut REGISTRY: std::sync::Mutex<u128> = std::sync::Mutex::new(0);

// pub static mut MEM1: *mut u64 = 0u64 as *mut u64;
thread_local!(static ID_CNT: UnsafeCell<(u8, u32)> = const { UnsafeCell::new((1, 1)) });

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

// #[inline(never)]
pub fn store_new<T>() -> ByteTrieNodePtr<T> {
    unsafe {
        let (thread, i) = ID_CNT.with(|mut R| {
            let (id, CNT) = unsafe { *R.get() };
            *R.get() = (id, CNT + 2);
            (id, CNT)
        });

        return ByteTrieNodePtr{ thread: thread, size: 0, index: i, pd: PhantomData::default() };
    }
}

// // #[inline(never)]
pub fn mmap_ptr(id: u8, cnt: u32, offset: u16) -> *mut u8 {
    unsafe { MMAP.offset(((((id as isize) << 32) | (cnt as isize)) << 12) | (offset as isize)) }
}

// #[inline(never)]
pub fn store_prepared<T>(rmask: [u64; 4], vmask: [u64; 4]) -> ByteTrieNodePtr<T> {
    unsafe {
        // let l = Utils::len(&rmask);
        let (thread, i) = ID_CNT.with(|mut R| {
            let (id, CNT) = unsafe { *R.get() };
            let mut MEM: *mut u64 = mmap_ptr(id, CNT, 0) as *mut u64;
            ptr::write(MEM as *mut [u64; 4], rmask);
            ptr::write(MEM.offset(4) as *mut [u64; 4], vmask);
            *R.get() = (id, CNT + 2);
            (id, CNT)
        });
        return ByteTrieNodePtr{ thread: thread, size: 0, index: i, pd: PhantomData::default() };
    }
}

// #[inline(never)]
pub fn store<T>(rmask: [u64; 4], rvalues: *mut ByteTrieNodePtr<T>, vmask: [u64; 4], vvalues: *mut T) -> ByteTrieNodePtr<T> {
    unsafe {
        let rl = Utils::len(&rmask);
        let vl = Utils::len(&vmask);
        let (thread, i) = ID_CNT.with(|mut R| {
            let (id, CNT) = unsafe { *R.get() };
            let mut MEM: *mut u64 = mmap_ptr(id, CNT, 0) as *mut u64;
            ptr::write(MEM as *mut [u64; 4], rmask);
            ptr::write(MEM.offset(4) as *mut [u64; 4], vmask);
            debug_assert_eq!(mem::size_of::<ByteTrieNodePtr<T>>(), mem::size_of::<u64>());
            ptr::copy_nonoverlapping::<u64>(rvalues as *mut u64, MEM.offset(8), rl);
            ptr::copy_nonoverlapping::<T>(vvalues, MEM.offset(8 + 256) as *mut T, vl);
            *R.get() = (id, CNT + 2);
            (id, CNT)
        });
        return ByteTrieNodePtr{ thread: thread, size: rl as u8, index: i, pd: PhantomData::default() };
    }
}

// #[inline(never)]
pub fn load_rmask<T>(node: ByteTrieNodePtr<T>) -> *mut [u64; 4] {
    unsafe {
        mmap_ptr(node.thread, node.index, 0) as *mut [u64; 4]
    }
}

// #[inline(never)]
pub fn load_vmask<T>(node: ByteTrieNodePtr<T>) -> *mut [u64; 4] {
    unsafe {
        mmap_ptr(node.thread, node.index, 32) as *mut [u64; 4]
    }
}

// #[inline(never)]
pub fn load_rvalues<T>(node: ByteTrieNodePtr<T>) -> *mut ByteTrieNodePtr<T> {
    unsafe {
        mmap_ptr(node.thread, node.index, 64) as *mut ByteTrieNodePtr<T>
    }
}

// #[inline(never)]
pub fn load_vvalues<T>(node: ByteTrieNodePtr<T>) -> *mut T {
    unsafe {
        mmap_ptr(node.thread, node.index, 64 + 8*256) as *mut T
    }
}

impl <V : Clone + Debug> ByteTrieNodePtr<V> {
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

    // pub fn items<'a>(&'a self) -> impl Iterator<Item=(Vec<u8>, &'a V)> + 'a {
    //     BytesTrieMapIter::new(*self)
    // }

    pub fn contains(self, k: &[u8]) -> bool {
        self.get(k).is_some()
    }

    pub fn insert(self, k: &[u8], v: V) -> bool {
        // TODO break loop into "traversing known" and "creating new"
        unsafe {
            debug_assert!(k.len() >= 0);
            let mut node = self;

            if k.len() > 1 {
                for i in 0..k.len() - 1 {
                    node = *Utils::update(&mut *load_rmask(node), load_rvalues(node), k[i], store_new);
                }
            }

            let lk = k[k.len() - 1];
            let m = &mut *load_vmask(node);
            let vs = load_vvalues(node);
            Utils::insert(m, vs, lk, v)
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

        // #[inline(never)]
        pub fn update<F: FnOnce() -> V>(self, k: &[u8], default: F) -> &mut V {
            unsafe {
            assert!(k.len() >= 0);
            let mut node = self;

            if k.len() > 1 {
                for i in 0..k.len() - 1 {
                    unsafe {
                        let cf = &mut *Utils::update(&mut *load_rmask(node), load_rvalues(node), k[i], ByteTrieNodePtr::null);

                        node = if cf.index != 0 { *cf } else {
                            let ptr = store_new();
                            // println!("created new {} {:?}", ptr.index, ptr);
                            *cf = ptr;
                            ptr
                        }
                    }
                }
            }

            let lk = k[k.len() - 1];
            &mut *Utils::update(&mut *load_vmask(node), load_vvalues(node), lk, default)
            }
        }

        pub fn get(self, k: &[u8]) -> Option<&V> {
            unsafe {
            let mut node = self;

            if k.len() > 1 {
                for i in 0..k.len() - 1 {
                    match Utils::get(&*load_rmask(node), load_rvalues(node), k[i]) {
                        Some(cf) => {
                            if (&*cf).index != 0 { node = *cf } else { return None }
                        }
                        None => { return None }
                    }
                }
            }

            match Utils::get(&*load_vmask(node), load_vvalues(node), k[k.len() - 1]) {
                None => { None }
                Some(v) => {
                    Some(v.as_ref().unwrap())
                }
            }
            }
        }

        pub fn len(self) -> usize {
            unsafe {
            return (*slice_from_raw_parts(load_rvalues(self), Utils::len(&*load_rmask(self)))).iter().rfold(Utils::len(&*load_vmask(self)), |t, btnp| unsafe {
                t + (if btnp.index == 0 { 0 } else { btnp.len() })
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
    fn new(m: &'a [u64; 4], values: *mut V) -> Self {
        Self {
            i: 0,
            w: m[0],
            m: m,
            v: values
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
    #[inline(always)]
    pub fn contains_set(mask: &mut [u64; 4], k: u8) -> bool {
        let mut w = &mut mask[((k & 0b11000000) >> 6) as usize];
        let bit = 1u64 << (k & 0b00111111);
        let r = 0 != (*w & bit);
        *w |= bit;
        r
    }

    // #[inline(never)]
    pub fn contains(mask: &[u64; 4], k: u8) -> bool {
        0 != (mask[((k & 0b11000000) >> 6) as usize] & (1u64 << (k & 0b00111111)))
    }

    // #[inline(never)]
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

    // #[inline(never)]
    pub fn len(mask: &[u64; 4]) -> usize {
        return (mask[0].count_ones() + mask[1].count_ones() + mask[2].count_ones() + mask[3].count_ones()) as usize;
    }

    // #[inline(never)]
    fn set(mask: &mut [u64; 4], k: u8) -> () {
        mask[((k & 0b11000000) >> 6) as usize] |= 1u64 << (k & 0b00111111);
    }

    // #[inline(never)]
    fn clear(mask: &mut [u64; 4], k: u8) -> () {
        mask[((k & 0b11000000) >> 6) as usize] &= !(1u64 << (k & 0b00111111));
    }

    // #[inline(never)]
    pub fn insert<V : Clone>(mask: &mut [u64; 4], values: *mut V, k: u8, v: V) -> bool {
        // let index = Self::left(mask, k) as usize;
        if Self::contains_set(mask, k) {
            // println!("overwriting {index}");
            true
        } else {
            unsafe {
                ptr::write(values.add(k as usize), v)
            }
            false
        }
    }

    // #[inline(never)]
    pub fn update<V, F : FnOnce() -> V>(mask: &mut [u64; 4], values: *mut V, k: u8, default: F) -> *mut V {
        if !Self::contains_set(mask, k) {
            // let len = Self::len(mask);
            unsafe { ptr::write(values.add(k as usize), default()); }
        }
        unsafe {
            values.add(k as usize)
        }
    }

    // #[inline(never)]
    pub fn get<V>(mask: &[u64; 4], values: *mut V, k: u8) -> Option<*mut V> {
        if Self::contains(mask, k) {
            // let ix = Self::left(mask, k) as usize;
            // println!("pos ix {} {} {:b}", pos, ix, self.mask);
            return unsafe { Some(values.add(k as usize)) };
        };
        return None;
    }
}
