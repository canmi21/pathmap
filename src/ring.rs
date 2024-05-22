
use std::collections::HashMap;
use std::hash::Hash;

use rclite::Rc;

use crate::bytetrie::{BytesTrieMap, ByteTrieNode, ShortTrieMap, CoFree};
use crate::bytetrie::NODE_CNT;

pub trait Lattice: Sized {
    fn join(&self, other: &Self) -> Self;
    fn join_into(&mut self, other: Self) {
        *self = self.join(&other);
    }
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
    fn join_into(&mut self, other: Self) {
        match self {
            None => { match other {
                None => { }
                Some(r) => { *self = Some(r) }
            } }
            Some(l) => match other {
                None => { }
                Some(r) => { l.join_into(r) }
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
impl <V : Lattice> Lattice for Box<V> {
    fn join(&self, other: &Self) -> Self {
        Box::new(self.as_ref().join(other.as_ref()))
    }

    fn meet(&self, other: &Self) -> Self {
        Box::new(self.as_ref().meet(other.as_ref()))
    }

    fn bottom() -> Self {
        Box::new(V::bottom())
    }
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

impl Lattice for u64 {
    fn join(&self, _other: &u64) -> u64 { *self }
    fn meet(&self, _other: &u64) -> u64 { *self }
    fn bottom() -> Self { 0 }
}

impl Lattice for &u64 {
    fn join(&self, _other: &Self) -> Self { self }
    fn meet(&self, _other: &Self) -> Self { self }
    fn bottom() -> Self { &0 }
}

impl PartialDistributiveLattice for u64 {
    fn psubtract(&self, other: &Self) -> Option<Self> where Self: Sized {
        if self == other { None }
        else { Some(*self) }
    }
}

impl Lattice for u32 {
    fn join(&self, _other: &u32) -> u32 { *self }
    fn meet(&self, _other: &u32) -> u32 { *self }
    fn bottom() -> Self { 0 }
}

impl Lattice for &u32 {
    fn join(&self, _other: &Self) -> Self { self }
    fn meet(&self, _other: &Self) -> Self { self }
    fn bottom() -> Self { &0 }
}

impl Lattice for u16 {
    fn join(&self, _other: &u16) -> u16 { *self }
    fn meet(&self, other: &u16) -> u16 { *other }
    fn bottom() -> Self { 0 }
}

impl Lattice for &u16 {
    fn join(&self, _other: &Self) -> Self { self }
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
    fn join(&self, _other: &u8) -> u8 { *self }
    fn meet(&self, _other: &u8) -> u8 { *self }
    fn bottom() -> Self { 0 }
}

impl Lattice for &u8 {
    fn join(&self, _other: &Self) -> Self { self }
    fn meet(&self, _other: &Self) -> Self { self }
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


impl<V : Lattice + Clone> Lattice for ByteTrieNode<V> {
    // #[inline(never)]
    fn join(&self, other: &Self) -> Self {
        let jm: [u64; 4] = [self.mask[0] | other.mask[0],
            self.mask[1] | other.mask[1],
            self.mask[2] | other.mask[2],
            self.mask[3] | other.mask[3]];
        let mm: [u64; 4] = [self.mask[0] & other.mask[0],
            self.mask[1] & other.mask[1],
            self.mask[2] & other.mask[2],
            self.mask[3] & other.mask[3]];

        let jmc = [jm[0].count_ones(), jm[1].count_ones(), jm[2].count_ones(), jm[3].count_ones()];

        let len = (jmc[0] + jmc[1] + jmc[2] + jmc[3]) as usize;
        let mut v: Vec<V> = Vec::with_capacity(len);

        let mut l = 0;
        let mut r = 0;
        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm[i];
            while lm != 0 {
                // this body runs at most 256 times, in the case there is 100% overlap between full nodes
                let index = lm.trailing_zeros();
                // println!("{}", index);
                if ((1u64 << index) & mm[i]) != 0 {
                    let lv = unsafe { self.values.get_unchecked(l) };
                    let rv = unsafe { other.values.get_unchecked(r) };
                    let jv = lv.join(rv);
                    // println!("pushing lv rv j {:?} {:?} {:?}", lv, rv, jv);
                    unsafe { std::ptr::write(v.get_unchecked_mut(c), jv) };
                    l += 1;
                    r += 1;
                } else if ((1u64 << index) & self.mask[i]) != 0 {
                    let lv = unsafe { self.values.get_unchecked(l) };
                    // println!("pushing lv {:?}", lv);
                    unsafe { std::ptr::write(v.get_unchecked_mut(c), lv.clone()) };
                    l += 1;
                } else {
                    let rv = unsafe { other.values.get_unchecked(r) };
                    // println!("pushing rv {:?}", rv);
                    unsafe { std::ptr::write(v.get_unchecked_mut(c), rv.clone()) };
                    r += 1;
                }
                lm ^= 1u64 << index;
                c += 1;
            }
        }

        unsafe{ v.set_len(c); }
        NODE_CNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        return ByteTrieNode::<V>{ mask: jm, values: v };
    }

    fn join_into(&mut self, mut other: Self) {
        let jm: [u64; 4] = [self.mask[0] | other.mask[0],
            self.mask[1] | other.mask[1],
            self.mask[2] | other.mask[2],
            self.mask[3] | other.mask[3]];
        let mm: [u64; 4] = [self.mask[0] & other.mask[0],
            self.mask[1] & other.mask[1],
            self.mask[2] & other.mask[2],
            self.mask[3] & other.mask[3]];

        let jmc = [jm[0].count_ones(), jm[1].count_ones(), jm[2].count_ones(), jm[3].count_ones()];

        let l = (jmc[0] + jmc[1] + jmc[2] + jmc[3]) as usize;
        let mut v = Vec::with_capacity(l);

        let mut l = 0;
        let mut r = 0;
        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm[i];
            while lm != 0 {
                // this body runs at most 256 times, in the case there is 100% overlap between full nodes
                let index = lm.trailing_zeros();
                // println!("{}", index);
                if ((1u64 << index) & mm[i]) != 0 {
                    let mut lv = unsafe { std::ptr::read(self.values.get_unchecked_mut(l)) };
                    let rv = unsafe { std::ptr::read(other.values.get_unchecked_mut(r)) };
                    lv.join_into(rv);
                    unsafe { std::ptr::write(v.get_unchecked_mut(c), lv) };
                    l += 1;
                    r += 1;
                } else if ((1u64 << index) & self.mask[i]) != 0 {
                    let lv = unsafe { std::ptr::read(self.values.get_unchecked_mut(l)) };
                    unsafe { std::ptr::write(v.get_unchecked_mut(c), lv) };
                    l += 1;
                } else {
                    let rv = unsafe { std::ptr::read(other.values.get_unchecked_mut(r)) };
                    unsafe { std::ptr::write(v.get_unchecked_mut(c), rv) };
                    r += 1;
                }
                lm ^= 1u64 << index;
                c += 1;
            }
        }

        unsafe { self.values.set_len(0) }
        unsafe { other.values.set_len(0) }
        unsafe { v.set_len(c) }
        self.mask = jm;
        self.values = v;
    }

    fn meet(&self, other: &Self) -> Self {
        // TODO this technically doesn't need to calculate and iterate over jm
        // iterating over mm and calculating m such that the following suffices
        // c_{self,other} += popcnt(m & {self,other})
        let jm: [u64; 4] = [self.mask[0] | other.mask[0],
            self.mask[1] | other.mask[1],
            self.mask[2] | other.mask[2],
            self.mask[3] | other.mask[3]];
        let mm: [u64; 4] = [self.mask[0] & other.mask[0],
            self.mask[1] & other.mask[1],
            self.mask[2] & other.mask[2],
            self.mask[3] & other.mask[3]];

        let mmc = [mm[0].count_ones(), mm[1].count_ones(), mm[2].count_ones(), mm[3].count_ones()];

        let len = (mmc[0] + mmc[1] + mmc[2] + mmc[3]) as usize;
        let mut v = Vec::with_capacity(len);

        let mut l = 0;
        let mut r = 0;
        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm[i];
            while lm != 0 {
                let index = lm.trailing_zeros();

                if ((1u64 << index) & mm[i]) != 0 {
                    let lv = unsafe { self.values.get_unchecked(l) };
                    let rv = unsafe { other.values.get_unchecked(r) };
                    let jv = lv.meet(rv);
                    unsafe { std::ptr::write(v.get_unchecked_mut(c), jv) };
                    l += 1;
                    r += 1;
                    c += 1;
                } else if ((1u64 << index) & self.mask[i]) != 0 {
                    l += 1;
                } else {
                    r += 1;
                }
                lm ^= 1u64 << index;
            }
        }

        unsafe{ v.set_len(c); }
        NODE_CNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        return ByteTrieNode::<V>{ mask: mm, values: v };
    }

    fn bottom() -> Self {
        ByteTrieNode::new()
    }

    fn join_all(xs: Vec<&Self>) -> Self {
        let mut jm: [u64; 4] = [0, 0, 0, 0];
        for x in xs.iter() {
            jm[0] |= x.mask[0];
            jm[1] |= x.mask[1];
            jm[2] |= x.mask[2];
            jm[3] |= x.mask[3];
        }

        let jmc = [jm[0].count_ones(), jm[1].count_ones(), jm[2].count_ones(), jm[3].count_ones()];

        let len = (jmc[0] + jmc[1] + jmc[2] + jmc[3]) as usize;
        let mut v = Vec::with_capacity(len);

        let mut c = 0;

        for i in 0..4 {
            let mut lm = jm[i];
            while lm != 0 {
                // this body runs at most 256 times, in the case there is 100% overlap between full nodes
                let index = lm.trailing_zeros();

                let to_join: Vec<&V> = xs.iter().enumerate().filter_map(|(i, x)| x.get(i as u8)).collect();
                let joined = Lattice::join_all(to_join);
                unsafe { std::ptr::write(v.get_unchecked_mut(c), joined) };

                lm ^= 1u64 << index;
                c += 1;
            }
        }

        unsafe{ v.set_len(c); }
        NODE_CNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        return ByteTrieNode::<V>{ mask: jm, values: v };
    }
}

impl <V : PartialDistributiveLattice + Clone> DistributiveLattice for ByteTrieNode<V> {
    fn subtract(&self, other: &Self) -> Self {
        let mut btn = self.clone();

        for i in 0..4 {
            let mut lm = self.mask[i];
            while lm != 0 {
                let index = lm.trailing_zeros();

                if ((1u64 << index) & other.mask[i]) != 0 {
                    let lv = unsafe { self.get_unchecked(64*(i as u8)) };
                    let rv = unsafe { other.get_unchecked(64*(i as u8) + (index as u8)) };
                    match lv.psubtract(rv) {
                        None => { btn.remove(64*(i as u8)); }
                        Some(jv) => {
                            let dst = unsafe { btn.get_unchecked_mut(64*(i as u8)) };
                            *dst = jv;
                        }
                    }
                }

                lm ^= 1u64 << index;
            }
        }

        return btn;
    }
}

impl <V : PartialDistributiveLattice + Clone> PartialDistributiveLattice for ByteTrieNode<V> {
    fn psubtract(&self, other: &Self) -> Option<Self> where Self: Sized {
        let r = self.subtract(other);
        if r.len() == 0 { return None }
        else { return Some(r) }
    }
}

// impl<V: Lattice + Clone> Lattice for Box<ByteTrieNode<V>> {
//     fn join(&self, other: &Self) -> Self {
//         Box::new((&**self).join(&**other))
//     }
//     fn meet(&self, other: &Self) -> Self {
//         Box::new((&**self).meet(&**other))
//     }
//     fn bottom() -> Self {
//         unreachable!()
//     }
// }

// impl<V: PartialDistributiveLattice + Clone> PartialDistributiveLattice for Box<ByteTrieNode<V>> {
//     fn psubtract(&self, other: &Self) -> Option<Self> {
//         (&**self).psubtract(&**other).map(|btn| Box::new(btn))
//     }
// }

impl<V: Lattice + Clone> Lattice for Option<Rc<ByteTrieNode<V>>> {
    fn join(&self, other: &Self) -> Self {
        match self {
            None => { other.clone() }
            Some(sptr) => {
                match other {
                    None => { None }
                    Some(optr) => {
                        let v = sptr.join(optr);
                        Some(Rc::new(v))
                    }
                }
            }
        }
    }

    fn join_into(&mut self, other: Self) {
        match self {
            None => { *self = other }
            Some(sptr) => {
                match other {
                    None => { }
                    Some(optr) => {
                        let raw_other = Rc::unwrap_or_clone(optr);
                        Rc::make_mut(sptr).join_into(raw_other);
                    }
                }
            }
        }
    }

    fn meet(&self, other: &Self) -> Self {
        match self {
            None => { None }
            Some(sptr) => {
                match other {
                    None => { None }
                    Some(optr) => {
                        let v = sptr.meet(optr);
                        Some(Rc::new(v))
                    }
                }
            }
        }
    }

    fn bottom() -> Self {
        None
    }
}

impl<V: PartialDistributiveLattice + Clone> PartialDistributiveLattice for Option<Rc<ByteTrieNode<V>>> {
    fn psubtract(&self, other: &Self) -> Option<Self> {
        match self {
            None => { None }
            Some(sr) => {
                match other {
                    None => { Some(self.clone()) }
                    Some(or) => {
                        let v = sr.subtract(or);
                        if v.len() == 0 { None }
                        else {
                            let vb = Rc::new(v);
                            Some(Some(vb))
                        }
                    }
                }
            }
        }
    }
}

impl<V : Clone + Lattice> Lattice for ShortTrieMap<V> {
    fn join(&self, other: &Self) -> Self {
        Self {
            root: self.root.join(&other.root),
        }
    }

    fn join_into(&mut self, other: Self) {
        self.root.join_into(other.root)
    }

    fn meet(&self, other: &Self) -> Self {
        Self {
            root: self.root.meet(&other.root),
        }
    }

    fn bottom() -> Self {
        ShortTrieMap::new()
    }
}

impl<V : Clone + PartialDistributiveLattice> DistributiveLattice for ShortTrieMap<V> {
    fn subtract(&self, other: &Self) -> Self {
        Self {
            root: self.root.subtract(&other.root),
        }
    }
}

impl<V : Clone + Lattice> Lattice for CoFree<V> {
    fn join(&self, other: &Self) -> Self {
        CoFree {
            rec: self.rec.join(&other.rec),
            value: self.value.join(&other.value)
        }
    }

    fn join_into(&mut self, other: Self) {
        self.rec.join_into(other.rec);
        self.value.join_into(other.value);
    }

    fn meet(&self, other: &Self) -> Self {
        CoFree {
            rec: self.rec.meet(&other.rec),
            value: self.value.meet(&other.value)
        }
    }

    fn bottom() -> Self {
        CoFree {
            rec: None,
            value: None
        }
    }
}

impl<V : Clone + PartialDistributiveLattice> DistributiveLattice for CoFree<V> {
    fn subtract(&self, other: &Self) -> Self {
        CoFree {
            rec: self.rec.psubtract(&other.rec).unwrap_or(None),
            value: self.value.subtract(&other.value)
        }
    }
}

impl<V : Clone + PartialDistributiveLattice> PartialDistributiveLattice for CoFree<V> {
    fn psubtract(&self, other: &Self) -> Option<Self> where Self: Sized {
        // .unwrap_or(ptr::null_mut())
        let r = self.rec.psubtract(&other.rec);
        let v = self.value.subtract(&other.value);
        match r {
            None => { if v.is_none() { None } else { Some(CoFree{ rec: None, value: v }) } }
            Some(sr) => { Some(CoFree{ rec: sr, value: v }) }
        }
    }
}

impl<V : Clone + Lattice> Lattice for BytesTrieMap<V> {
    fn join(&self, other: &Self) -> Self {
        Self {
            root: self.root.join(&other.root),
        }
    }

    fn join_into(&mut self, other: Self) {
        self.root.join_into(other.root)
    }

    fn meet(&self, other: &Self) -> Self {
        Self {
            root: self.root.meet(&other.root),
        }
    }

    fn bottom() -> Self {
        BytesTrieMap::new()
    }
}

impl<V : Clone + PartialDistributiveLattice> DistributiveLattice for BytesTrieMap<V> {
    fn subtract(&self, other: &Self) -> Self {
        Self {
            root: self.root.subtract(&other.root),
        }
    }
}

impl<V : Clone + PartialDistributiveLattice> PartialDistributiveLattice for BytesTrieMap<V> {
    fn psubtract(&self, other: &Self) -> Option<Self> {
        let s = self.root.subtract(&other.root);
        if s.len() == 0 { None }
        else { Some(Self { root: s }) }
    }
}
