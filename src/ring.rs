use crate::bytetrie::*;
use std::collections::HashMap;
use std::hash::Hash;
use std::{mem, ptr};
use std::process::exit;

pub trait Lattice: Sized {
    fn join(&self, other: &Self) -> Self;
    fn meet(&self, other: &Self) -> Self;
    fn bottom() -> Self;
    fn join_all(xs: Vec<&Self>) -> Self {
        xs.iter().rfold(Self::bottom(), |x, y| x.join(y))
    }
}

pub trait MapRing<V> {
    fn join_with(&self, other: &Self, op: fn(&V, &V) -> V) -> Self;
    // fn meet_with<F: Copy + Fn(&V, &V) -> V>(&self, other: &Self, op: F) -> Self;
    // fn subtract_with<F: Copy + Fn(&V, &V) -> Option<V>>(&self, other: &Self, op: F) -> Self;
}

pub trait DistributiveLattice: Lattice {
    fn subtract(&self, other: &Self) -> Self;
}

pub trait PartialDistributiveLattice: Lattice {
    fn psubtract(&self, other: &Self) -> Option<Self> where Self: Sized;
}

impl Lattice for () {
    fn join(&self, other: &Self) -> Self { () }
    fn meet(&self, other: &Self) -> Self { () }
    fn bottom() -> Self { () }
}

impl Lattice for &() {
    fn join(&self, other: &Self) -> Self { &() }
    fn meet(&self, other: &Self) -> Self { &() }
    fn bottom() -> Self { &() }
}

impl <V : Lattice + Clone> Lattice for Option<V> {
    fn join(&self, other: &Option<V>) -> Option<V> {
        match self {
            None => { match other {
                None => { None }
                Some(r) => { Some(r.clone()) }
            } }
            Some(l) => match other {
                None => { Some(l.clone()) }
                Some(r) => { Some(l.join(r)) }
            }
        }
    }

    fn meet(&self, other: &Option<V>) -> Option<V> {
        match self {
            None => { None }
            Some(l) => {
                match other {
                    None => { None }
                    Some(r) => Some(l.meet(r))
                }
            }
        }
    }

    fn bottom() -> Self {
        None
    }
}

impl <V : PartialDistributiveLattice + Clone> DistributiveLattice for Option<V> {
    fn subtract(&self, other: &Self) -> Self {
        match self {
            None => { None }
            Some(s) => { match other {
                None => { Some(s.clone()) }
                Some(o) => { s.psubtract(o) }
            } }
        }
    }
}

impl <V : Clone> MapRing<V> for Option<V> {
    fn join_with(&self, other: &Self, op: fn(&V, &V) -> V) -> Self {
        match self {
            None => { match other {
                None => { None }
                Some(r) => { Some(r.clone()) }
            } }
            Some(l) => match other {
                None => { Some(l.clone()) }
                Some(r) => { Some(op(l, r)) }
            }
        }
    }

    // fn meet_with<F: Copy + Fn(&V, &V) -> V>(&self, other: &Self, op: F) -> Self {
    //     match self {
    //         None => { None }
    //         Some(l) => {
    //             match other {
    //                 None => { None }
    //                 Some(r) => Some(op(l, r))
    //             }
    //         }
    //     }
    // }
    //
    // fn subtract_with<F: Copy + Fn(&V, &V) -> Option<V>>(&self, other: &Self, op: F) -> Self {
    //     match self {
    //         None => { None }
    //         Some(l) => {
    //             match other {
    //                 None => { Some(l.clone()) }
    //                 Some(r) => op(l, r)
    //             }
    //         }
    //     }
    // }
}


impl Lattice for u64 {
    fn join(&self, other: &u64) -> u64 { *self }
    fn meet(&self, other: &u64) -> u64 { *self }
    fn bottom() -> Self { 0 }
}

impl Lattice for &u64 {
    fn join(&self, other: &Self) -> Self { self }
    fn meet(&self, other: &Self) -> Self { self }
    fn bottom() -> Self { &0 }
}

impl PartialDistributiveLattice for u64 {
    fn psubtract(&self, other: &Self) -> Option<Self> where Self: Sized {
        if self == other { None }
        else { Some(*self) }
    }
}

impl Lattice for u32 {
    fn join(&self, other: &u32) -> u32 { *self }
    fn meet(&self, other: &u32) -> u32 { *self }
    fn bottom() -> Self { 0 }
}

impl Lattice for &u32 {
    fn join(&self, other: &Self) -> Self { self }
    fn meet(&self, other: &Self) -> Self { self }
    fn bottom() -> Self { &0 }
}

impl Lattice for u16 {
    fn join(&self, other: &u16) -> u16 { *self }
    fn meet(&self, other: &u16) -> u16 { *other }
    fn bottom() -> Self { 0 }
}

impl Lattice for &u16 {
    fn join(&self, other: &Self) -> Self { self }
    fn meet(&self, other: &Self) -> Self { other }
    fn bottom() -> Self { &0 }
}

impl PartialDistributiveLattice for u16 {
    fn psubtract(&self, other: &Self) -> Option<Self> where Self: Sized {
        if self == other { None }
        else { Some(*self) }
    }
}

impl Lattice for u8 {
    fn join(&self, other: &u8) -> u8 { *self }
    fn meet(&self, other: &u8) -> u8 { *self }
    fn bottom() -> Self { 0 }
}

impl Lattice for &u8 {
    fn join(&self, other: &Self) -> Self { self }
    fn meet(&self, other: &Self) -> Self { self }
    fn bottom() -> Self { &0 }
}

impl <K : Copy + Eq + Hash, V : Copy + Lattice> Lattice for HashMap<K, V> {
    fn join(&self, other: &HashMap<K, V>) -> HashMap<K, V> {
        let mut res = HashMap::<K, V>::new();
        for (key, value) in self.iter() {
            if !other.contains_key(key) {
                res.insert(*key, *value);
            }
        }
        for (key, value) in other.iter() {
            if !self.contains_key(key) {
                res.insert(*key, *value);
            }
        }
        for (key, value) in self.iter() {
            if other.contains_key(key) {
                res.insert(*key, value.join(other.get(key).unwrap()));
            }
        }
        return res
    }

    fn meet(&self, other: &HashMap<K, V>) -> HashMap<K, V> {
        let mut res = HashMap::<K, V>::new();
        for (key, value) in self.iter() {
            if other.contains_key(key) {
                res.insert(*key, value.join(other.get(key).unwrap()));
            }
        }
        return res;
    }

    fn bottom() -> Self {
        HashMap::new()
    }
}


impl<V : Copy + Lattice> Lattice for ByteTrieNodePtr<V> {
    // #[inline(never)]
    fn join(&self, other: &Self) -> Self {
        if self.index == 0 { return *other }
        if other.index == 0 { return *self }
        let srm = unsafe { *load_rmask(*self) };
        let svm = unsafe { *load_vmask(*self) };
        let srv = load_rvalues(*self);
        let svv = load_vvalues(*self);
        let orm = unsafe { *load_rmask(*other) };
        let ovm = unsafe { *load_vmask(*other) };
        let orv = load_rvalues(*other);
        let ovv = load_vvalues(*other);
        let jrm: [u64; 4] = [srm[0] | orm[0],
            srm[1] | orm[1],
            srm[2] | orm[2],
            srm[3] | orm[3]];
        let jvm: [u64; 4] = [svm[0] | ovm[0],
            svm[1] | ovm[1],
            svm[2] | ovm[2],
            svm[3] | ovm[3]];
        let mrm: [u64; 4] = [srm[0] & orm[0],
            srm[1] & orm[1],
            srm[2] & orm[2],
            srm[3] & orm[3]];
        let mvm: [u64; 4] = [svm[0] & ovm[0],
            svm[1] & ovm[1],
            svm[2] & ovm[2],
            svm[3] & ovm[3]];
        // let jmc = [jm[0].count_ones(), jm[1].count_ones(), jm[2].count_ones(), jm[3].count_ones()];

        // let l = (jmc[0] + jmc[1] + jmc[2] + jmc[3]) as usize;
        let res = store_prepared(jrm, jvm);

        let mut rv: *mut ByteTrieNodePtr<V> = load_rvalues(res);
        let mut vv: *mut V = load_vvalues(res);

        {
            for i in 0..4 {
                let mut lm = jrm[i];

                while lm != 0 {
                    // this body runs at most 256 times, in the case there is 100% overlap between full nodes
                    let index = lm.trailing_zeros() as usize;
                    // println!("{}", index);
                    if ((1u64 << index) & mrm[i]) != 0 {
                        let left_v = unsafe { ptr::read(srv.add(64*i + index)) };
                        let right_v = unsafe { ptr::read(orv.add(64*i + index)) };
                        let joined_v = left_v.join(&right_v);
                        // println!("pushing lv rv j {:?} {:?} {:?}", lv, rv, jv);
                        unsafe { ptr::write(rv.add(64*i + index), joined_v); }

                    }
                    else if ((1u64 << index) & srm[i]) != 0 {
                        let left_v = unsafe { ptr::read(srv.add(64*i + index)) };
                        // println!("pushing lv {:?}", lv);
                        unsafe { ptr::write(rv.add(64*i + index), left_v); }
                    } else {
                        let right_v = unsafe { ptr::read(orv.add(64*i + index)) };
                        // println!("pushing rv {:?}", rv);
                        unsafe { ptr::write(rv.add(64*i + index),  right_v) };
                    }
                    lm ^= 1u64 << index;
                }
            }
        }

        {
            for i in 0..4 {
                let mut lm = jvm[i];

                while lm != 0 {
                    // this body runs at most 256 times, in the case there is 100% overlap between full nodes
                    let index = lm.trailing_zeros() as usize;
                    // println!("{}", index);
                    if ((1u64 << index) & mvm[i]) != 0 {
                        let left_v = unsafe { ptr::read(svv.add(64*i + index)) };
                        let right_v = unsafe { ptr::read(ovv.add(64*i + index)) };
                        let joined_v = left_v.join(&right_v);
                        // println!("pushing lv rv j {:?} {:?} {:?}", lv, rv, jv);
                        unsafe { ptr::write(vv.add(64*i + index), joined_v); }

                    }
                    else if ((1u64 << index) & svm[i]) != 0 {
                        let left_v = unsafe { ptr::read(svv.add(64*i + index)) };
                        // println!("pushing lv {:?}", lv);
                        unsafe { ptr::write(vv.add(64*i + index), left_v); }
                    } else {
                        let right_v = unsafe { ptr::read(ovv.add(64*i + index)) };
                        // println!("pushing rv {:?}", rv);
                        unsafe { ptr::write(vv.add(64*i + index),  right_v) };
                    }
                    lm ^= 1u64 << index;
                }
            }
        }

        return res;
    }

    fn meet(&self, other: &Self) -> Self {
        todo!()
        // if self.index == 0 { return ByteTrieNodePtr::null() }
        // if other.index == 0 { return ByteTrieNodePtr::null() }
        // // TODO this technically doesn't need to calculate and iterate over jm
        // // iterating over mm and calculating m such that the following suffices
        // // c_{self,other} += popcnt(m & {self,other})
        // let sm = unsafe { *load_mask(*self) };
        // let sv = load_values(*self);
        // let om = unsafe { *load_mask(*other) };
        // let ov = load_values(*other);
        // let jm: [u64; 4] = [sm[0] | om[0],
        //     sm[1] | om[1],
        //     sm[2] | om[2],
        //     sm[3] | om[3]];
        // let mm: [u64; 4] = [sm[0] & om[0],
        //     sm[1] & om[1],
        //     sm[2] & om[2],
        //     sm[3] & om[3]];
        //
        // // let mmc = [mm[0].count_ones(), mm[1].count_ones(), mm[2].count_ones(), mm[3].count_ones()];
        //
        // // let l = (mmc[0] + mmc[1] + mmc[2] + mmc[3]) as usize;
        // // let mut v = Vec::with_capacity(l);
        // // unsafe { v.set_len(l) }
        //
        // let res = store_prepared(mm);
        // let mut v: *mut V = load_values(res);
        //
        //
        // let mut l = 0;
        // let mut r = 0;
        // let mut c = 0;
        //
        // for i in 0..4 {
        //     let mut lm = jm[i];
        //     while lm != 0 {
        //         let index = lm.trailing_zeros();
        //
        //         if ((1u64 << index) & mm[i]) != 0 {
        //             let lv = unsafe { &*sv.offset(l) };
        //             let rv = unsafe { &*ov.offset(r) };
        //             let jv = lv.meet(rv);
        //             unsafe { *v.offset(c) = jv; }
        //             l += 1;
        //             r += 1;
        //             c += 1;
        //         } else if ((1u64 << index) & sm[i]) != 0 {
        //             l += 1;
        //         } else {
        //             r += 1;
        //         }
        //         lm ^= 1u64 << index;
        //     }
        // }
        //
        // return res;
    }

    fn bottom() -> Self {
        store_new()
    }

    // fn join_all(xs: Vec<&Self>) -> Self {
    //     let mut jm: [u64; 4] = [0, 0, 0, 0];
    //     for x in xs.iter() {
    //         jm[0] |= x.mask[0];
    //         jm[1] |= x.mask[1];
    //         jm[2] |= x.mask[2];
    //         jm[3] |= x.mask[3];
    //     }
    //
    //     let jmc = [jm[0].count_ones(), jm[1].count_ones(), jm[2].count_ones(), jm[3].count_ones()];
    //
    //     let l = (jmc[0] + jmc[1] + jmc[2] + jmc[3]) as usize;
    //     let mut v = Vec::with_capacity(l);
    //     unsafe { v.set_len(l) }
    //
    //     let mut c = 0;
    //
    //     for i in 0..4 {
    //         let mut lm = jm[i];
    //         while lm != 0 {
    //             // this body runs at most 256 times, in the case there is 100% overlap between full nodes
    //             let index = lm.trailing_zeros();
    //
    //             let to_join: Vec<&V> = xs.iter().enumerate().filter_map(|(i, x)| x.get(i as u8)).collect();
    //             unsafe { *v.get_unchecked_mut(c) = Lattice::join_all(to_join); }
    //
    //             lm ^= 1u64 << index;
    //             c += 1;
    //         }
    //     }
    //
    //     return ByteTrieNode::<V>{ mask: jm, values: v };
    // }
}

// impl <V : Copy + PartialDistributiveLattice> DistributiveLattice for ByteTrieNodePtr<V> {
//     fn subtract(&self, other: &Self) -> Self {
//         let sm = unsafe { *load_mask(*self) };
//         let sv = load_values(*self);
//         let om = unsafe { *load_mask(*other) };
//         let ov = load_values(*other);
//         let mut btn = self.clone();
//
//         let mut l = 0;
//         let mut r = 0;
//         let mut c = 0;
//
//         for i in 0..4 {
//             let mut lm = sm[i];
//             while lm != 0 {
//                 let index = lm.trailing_zeros();
//
//                 if ((1u64 << index) & om[i]) != 0 {
//                     let lv = unsafe { &*sv.offset(l) };
//                     let rv = unsafe { &*ov.offset(r) };
//                     match lv.psubtract(rv) {
//                         None => { btn.remove(64*(i as u8)); }
//                         Some(jv) => unsafe { *sv.offset(l) = jv; }
//                     }
//                 }
//
//                 lm ^= 1u64 << index;
//                 l += 1;
//             }
//         }
//
//         return btn;
//     }
// }

// impl <V : Copy + PartialDistributiveLattice> PartialDistributiveLattice for ByteTrieNode<V> {
//     fn psubtract(&self, other: &Self) -> Option<Self> where Self: Sized {
//         let r = self.subtract(other);
//         if r.len() == 0 { return None }
//         else { return Some(r) }
//     }
// }

// impl <V : Copy + Lattice> Lattice for *mut ByteTrieNode<V> {
//     fn join(&self, other: &Self) -> Self {
//         unsafe {
//             match self.as_ref() {
//                 None => { *other }
//                 Some(sptr) => {
//                     match other.as_ref() {
//                         None => { ptr::null_mut() }
//                         Some(optr) => {
//                             let v = unsafe { sptr.join(optr) };
//                             let mut vb = Box::new(v);
//                             let p = vb.as_mut() as Self;
//                             mem::forget(vb);
//                             p
//                         }
//                     }
//                 }
//             }
//         }
//     }
//
//     fn meet(&self, other: &Self) -> Self {
//         unsafe {
//             match self.as_ref() {
//                 None => { ptr::null_mut() }
//                 Some(sptr) => {
//                     match other.as_ref() {
//                         None => { ptr::null_mut() }
//                         Some(optr) => {
//                             let v = unsafe { sptr.meet(optr) };
//                             let mut vb = Box::new(v);
//                             let p = vb.as_mut() as Self;
//                             mem::forget(vb);
//                             p
//                         }
//                     }
//                 }
//             }
//         }
//     }
//
//     fn bottom() -> Self {
//         ptr::null_mut()
//     }
// }

// impl<V : Copy + PartialDistributiveLattice> PartialDistributiveLattice for *mut ByteTrieNode<V> {
//     fn psubtract(&self, other: &Self) -> Option<Self> {
//         unsafe {
//             match self.as_ref() {
//                 None => { None }
//                 Some(sr) => {
//                     match other.as_ref() {
//                         None => { Some(*self) }
//                         Some(or) => {
//                             let v = sr.subtract(or);
//                             if v.len() == 0 { None }
//                             else {
//                                 let mut vb = Box::new(v);
//                                 let p = vb.as_mut() as Self;
//                                 mem::forget(vb);
//                                 Some(p)
//                             }
//                         }
//                     }
//                 }
//             }
//         }
//     }
// }

// impl<V : Copy + Lattice> Lattice for ShortTrieMap<V> {
//     fn join(&self, other: &Self) -> Self {
//         Self {
//             root: self.root.join(&other.root),
//         }
//     }
//
//     fn meet(&self, other: &Self) -> Self {
//         Self {
//             root: self.root.meet(&other.root),
//         }
//     }
//
//     fn bottom() -> Self {
//         ShortTrieMap::new()
//     }
// }
//
// impl<V : Copy + PartialDistributiveLattice> DistributiveLattice for ShortTrieMap<V> {
//     fn subtract(&self, other: &Self) -> Self {
//         Self {
//             root: self.root.subtract(&other.root),
//         }
//     }
// }

impl<V : Copy + Lattice> Lattice for CoFree<V> {
    fn join(&self, other: &Self) -> Self {
        CoFree {
            rec: self.rec.join(&other.rec),
            value: self.value.join(&other.value)
        }
    }

    fn meet(&self, other: &Self) -> Self {
        CoFree {
            rec: self.rec.meet(&other.rec),
            value: self.value.meet(&other.value)
        }
    }

    fn bottom() -> Self {
        CoFree {
            rec: ByteTrieNodePtr::null(),
            value: None
        }
    }
}

// impl<V : Copy + PartialDistributiveLattice> DistributiveLattice for CoFree<V> {
//     fn subtract(&self, other: &Self) -> Self {
//         CoFree {
//             rec: self.rec.psubtract(&other.rec).unwrap_or(ptr::null_mut()),
//             value: self.value.subtract(&other.value)
//         }
//     }
// }

// impl<V : Copy + PartialDistributiveLattice> PartialDistributiveLattice for CoFree<V> {
//     fn psubtract(&self, other: &Self) -> Option<Self> where Self: Sized {
//         // .unwrap_or(ptr::null_mut())
//         let r = self.rec.psubtract(&other.rec);
//         let v = self.value.subtract(&other.value);
//         match r {
//             None => { if v.is_none() { None } else { Some(CoFree{ rec: ByteTrieNodePtr::null(), value: v }) } }
//             Some(sr) => { Some(CoFree{ rec: sr, value: v }) }
//         }
//     }
// }

// impl<V : Copy + Lattice> Lattice for BytesTrieMap<V> {
//     fn join(&self, other: &Self) -> Self {
//         Self {
//             root: self.root.join(&other.root),
//         }
//     }
//
//     fn meet(&self, other: &Self) -> Self {
//         Self {
//             root: self.root.meet(&other.root),
//         }
//     }
//
//     fn bottom() -> Self {
//         BytesTrieMap::new()
//     }
// }

// impl<V : Copy + PartialDistributiveLattice> DistributiveLattice for BytesTrieMap<V> {
//     fn subtract(&self, other: &Self) -> Self {
//         Self {
//             root: self.root.subtract(&other.root),
//         }
//     }
// }

// impl<V : Copy + PartialDistributiveLattice> PartialDistributiveLattice for BytesTrieMap<V> {
//     fn psubtract(&self, other: &Self) -> Option<Self> {
//         let s = self.root.subtract(&other.root);
//         if s.len() == 0 { None }
//         else { Some(Self { root: s }) }
//     }
// }
