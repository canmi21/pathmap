use std::alloc::{dealloc, Layout};
use std::{mem, ptr};
use std::fmt::{Debug, Formatter};
use std::ptr::slice_from_raw_parts;


#[derive(Clone)]
pub struct ByteTrieNode<V> {
    pub(crate) mask: [u64; 4],
    pub(crate) values: Vec<V>
}

impl <V : Debug> Debug for ByteTrieNode<V> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f,
               "Node(mask: {:b} {:b} {:b} {:b}, values: {:?})",
               self.mask[0], self.mask[1], self.mask[2], self.mask[3],
               self.values)
    }
}

pub struct BytesTrieMapIter<'a, V> where V : Clone {
    prefix: Vec<u8>,
    btnis: Vec<ByteTrieNodeIter<'a, CoFree<V>>>,
}

impl <'a, V : Clone> BytesTrieMapIter<'a, V> {
    fn new(btm: &'a BytesTrieMap<V>) -> Self {
        Self {
            prefix: vec![],
            btnis: vec![ByteTrieNodeIter::new(&btm.root)],
        }
    }
}

impl <'a, V : Clone> Iterator for BytesTrieMapIter<'a, V> {
    type Item = (Vec<u8>, &'a V);

    fn next(&mut self) -> Option<(Vec<u8>, &'a V)> {
        loop {
            match self.btnis.last_mut() {
                None => { return None }
                Some(last) => {
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

                            match &cf.value {
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

/// An iterator-like object that traverses key-value pairs in a [BytesTrieMap], however only one
/// returned reference may exist at a given time
pub struct BytesTrieMapCursor<'a, V> where V : Clone {
    prefix: Vec<u8>,
    btnis: Vec<ByteTrieNodeIter<'a, CoFree<V>>>,
    nopush: bool
}

impl <'a, V : Clone> BytesTrieMapCursor<'a, V> {
    fn new(btm: &'a BytesTrieMap<V>) -> Self {
        Self {
            prefix: vec![],
            btnis: vec![ByteTrieNodeIter::new(&btm.root)],
            nopush: false
        }
    }
}

impl <'a, V : Clone> BytesTrieMapCursor<'a, V> {
    pub fn next(&mut self) -> Option<(&[u8], &'a V)> {
        loop {
            match self.btnis.last_mut() {
                None => { return None }
                Some(last) => {
                    match last.next() {
                        None => {
                            // decrease view len with one
                            self.prefix.pop();
                            self.btnis.pop();
                        }
                        Some((b, cf)) => {
                            if self.nopush {
                                *self.prefix.last_mut().unwrap() = b;
                                self.nopush = false;
                            } else {
                                self.prefix.push(b);
                            }

                            match unsafe { cf.rec.as_ref() } {
                                None => {
                                    self.nopush = true;
                                }
                                Some(rec) => {
                                    self.nopush = false;
                                    self.btnis.push(ByteTrieNodeIter::new(rec));
                                }
                            }

                            match &cf.value {
                                None => {}
                                Some(v) => {
                                    return Some((&self.prefix, v))
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
pub struct CoFree<V> where V : Clone {
    pub(crate) rec: *mut ByteTrieNode<CoFree<V>>,
    pub(crate) value: Option<V>
}

// BOTH_EMPTY(, )
// FIRST_EMPTY(, ab)
// SECOND_EMPTY(pq, )
// DISJOINT(abcd, xyz), DISJOINT(abc, xyzw), DISJOINT(abc, xyz)
// PREFIX_OF(ab, abcd)
// PREFIXED_BY(abcd, a)
// SHARING(abc, abpq)
// EQUALS(abcd, abcd)

enum PrefixComparison {
    BothEmpty,
    FirstEmpty,
    SecondEmpty,
    Disjoint,
    PrefixOf,
    PrefixedBy,
    Sharing,
    Equals,
}

fn compare_prefix(left: &[u8], right: &[u8], n: &mut usize) -> PrefixComparison {
    use PrefixComparison::*;
    let left_len = left.len();
    let right_len = right.len();
    if left_len == 0 && right_len == 0 { return BothEmpty }
    if left_len == 0 { return FirstEmpty }
    if right_len == 0 { return SecondEmpty }
    if unsafe { left.get_unchecked(0) != right.get_unchecked(0) } { return Disjoint }

    *n = 1;
    loop {
        let left_finished = *n == left_len;
        let right_finished = *n == right_len;
        if left_finished && right_finished { return Equals }
        if left_finished { return PrefixOf }
        if right_finished { return PrefixedBy }
        if unsafe { left.get_unchecked(*n) != right.get_unchecked(*n) } { return Sharing }
        *n += 1;
    }
}

pub fn insert_tail<K: AsRef<[u8]>>(btm: &mut BytesTrieMap<Box<[u8]>>, k: K) -> bool {
    let k = k.as_ref();
    let mut node = &mut btm.root;

    for i in 0..k.len() - 1 {
        let cf = node.update(k[i], || CoFree{rec: ptr::null_mut(), value: None});

        node = match unsafe{ cf.rec.as_mut() } {
            Some(r) => { r }
            None => {
                match cf.value.as_ref() {
                    None => {
                        cf.value = Some(k[i+1..].into());
                    }
                    Some(ab) => {
                        use PrefixComparison::*;
                        let existing = &ab[..];
                        let inserting = &k[i+1..];
                        let mut shared = 0;
                        match compare_prefix(existing, inserting, &mut shared) {
                            BothEmpty => { /* nothing needs to be done */ }
                            FirstEmpty => { panic!("FirstEmpty not implemented yet") }
                            SecondEmpty => { panic!("SecondEmpty not implemented yet") }
                            Disjoint => {
                                // add a node with the two diverging bytes added to it
                                // as values, use the respective continuations
                                let mut l = ByteTrieNode::new();
                                l.insert(existing[0], CoFree{ rec: ptr::null_mut(), value: Some(existing[1..].into()) });
                                l.insert(inserting[0], CoFree{ rec: ptr::null_mut(), value: Some(inserting[1..].into()) });

                                let lbox =  Box::new(l);
                                let ptr = Box::leak(lbox);
                                cf.rec = ptr;
                                cf.value = None;
                            }
                            PrefixOf => { panic!("PrefixOf not implemented yet") }
                            PrefixedBy => { panic!("PrefixedBy not implemented yet") }
                            Sharing => {
                                // create a chain of length `shared`
                                // add a node with the two diverging bytes added to it
                                // as values, use the respective continuations
                                let k_ = &existing[..shared];
                                let mut l = ByteTrieNode::new();
                                let mut node_ = &mut l;

                                for i in 0..k_.len() {
                                    let cf_ = node_.update(k_[i], || CoFree{rec: ptr::null_mut(), value: None});

                                    node_ = match unsafe{ cf_.rec.as_mut() } {
                                        Some(r) => { r }
                                        None => {
                                            let l_ = ByteTrieNode::new();
                                            let ptr_ = Box::leak(Box::new(l_));
                                            cf_.rec = ptr_;
                                            unsafe{ &mut *ptr_ }
                                        }
                                    }
                                }

                                node_.insert(existing[shared], CoFree{ rec: ptr::null_mut(), value: Some(existing[shared + 1..].into()) });
                                node_.insert(inserting[shared], CoFree{ rec: ptr::null_mut(), value: Some(inserting[shared + 1..].into()) });

                                let lbox =  Box::new(l);
                                let ptr = Box::leak(lbox);
                                cf.rec = ptr;
                                cf.value = None;
                            }
                            Equals => { /* nothing needs to be done */ }
                        }
                    }
                }
                return false
            }
        }
    }

    // todo if prefix
    true
}


/// A map type that uses byte slices `&[u8]` as keys
///
/// This type is implemented using some of the approaches explained in the
/// ["Bitwise trie with bitmap" Wikipedia article](https://en.wikipedia.org/wiki/Bitwise_trie_with_bitmap).
///
/// ```
/// # use ringmap::bytetrie::BytesTrieMap;
/// let mut map = BytesTrieMap::<String>::new();
/// map.insert("one", "1".to_string());
/// map.insert("two", "2".to_string());
///
/// assert!(map.contains("one"));
/// assert_eq!(map.get("two"), Some(&"2".to_string()));
/// assert!(!map.contains("three"));
/// ```
#[repr(transparent)]
#[derive(Clone)]
pub struct BytesTrieMap<V> where V : Clone {
    pub(crate) root: ByteTrieNode<CoFree<V>>
}

impl <V : Clone> BytesTrieMap<V> {
    pub fn new() -> Self {
        Self {
            root: ByteTrieNode::new()
        }
    }

    //QUESTION: who is the intended user of this method?  This interface is fundamentally unsafe
    // because it exposes a mutable reference inside an immutable structure
    pub(crate) fn at<K: AsRef<[u8]>>(&self, k: K) -> Option<&mut BytesTrieMap<V>> {
        let k = k.as_ref();
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

    pub fn items<'a>(&'a self) -> impl Iterator<Item=(Vec<u8>, &'a V)> + 'a {
        BytesTrieMapIter::new(self)
    }

    pub fn item_cursor<'a>(&'a self) -> BytesTrieMapCursor<'a, V> {
        BytesTrieMapCursor::new(self)
    }

    pub fn contains<K: AsRef<[u8]>>(&self, k: K) -> bool {
        self.get(k).is_some()
    }

    /// Inserts `v` at into the map at `k`.  Panics if `k` has a zero length
    pub fn insert<K: AsRef<[u8]>>(&mut self, k: K, v: V) -> bool {
        let k = k.as_ref();
        let mut node = &mut self.root;

        if k.len() > 1 {
            for i in 0..k.len() - 1 {
                let cf = node.update(k[i], || CoFree{rec: ptr::null_mut(), value: None});

                node = match unsafe{ cf.rec.as_mut() } {
                    Some(r) => { r }
                    None => {
                        let l = ByteTrieNode::new();
                        let ptr = Box::into_raw(Box::new(l));
                        cf.rec = ptr;
                        unsafe{ &mut *ptr }
                    }
                }
            }
        }

        let lk = k[k.len() - 1];
        if node.contains(lk) {
            let cf = unsafe{ node.get_unchecked_mut(lk) };
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

    pub fn update<K: AsRef<[u8]>, F : FnOnce() -> V>(&mut self, k: K, default: F) -> &mut V {
        let k = k.as_ref();
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

    pub fn get<K: AsRef<[u8]>>(&self, k: K) -> Option<&V> {
        let k = k.as_ref();
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

impl<V: Clone, K: AsRef<[u8]>> FromIterator<(K, V)> for BytesTrieMap<V> {
    fn from_iter<I: IntoIterator<Item=(K, V)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (key, val) in iter {
            map.insert(key, val);
        }
        map
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

    pub fn items<'a>(&'a self) -> impl Iterator<Item=(u16, &'a V)> + 'a {
        self.root.items().flat_map(move |(k1, l1)| unsafe {
            unsafe { &**l1 }.items().map(move |(k2, v)| ((k1 as u16) | ((k2 as u16) << 8), v))
        })
    }

    pub fn contains(&self, k: u16) -> bool {
        let k1 = k as u8;
        let k2 = (k >> 8) as u8;
        if self.root.contains(k1) {
            let rl1 = self.root.get_child(k1);
            rl1.contains(k2)
        } else {
            false
        }
    }

    pub fn insert(&mut self, k: u16, v: V) -> bool {
        let k1 = k as u8;
        let k2 = (k >> 8) as u8;
        if self.root.contains(k1) {
            let rl1 = self.root.get_child_mut(k1);
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

    // pub fn deepcopy(&self) -> Self {
    //     return self.items().collect();
    // }

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
            w: btn.mask[0],
            btn: btn
        }
    }
}

impl <'a, V : Clone> Iterator for ByteTrieNodeIter<'a, V> {
    type Item = (u8, &'a V);

    fn next(&mut self) -> Option<(u8, &'a V)> {
        loop {
            if self.w != 0 {
                let wi = self.w.trailing_zeros() as u8;
                self.w ^= 1u64 << wi;
                let index = self.i*64 + wi;
                return Some((index, unsafe{ self.btn.get_unchecked(index) } ))
            } else if self.i < 3 {
                self.i += 1;
                self.w = unsafe { *self.btn.mask.get_unchecked(self.i as usize) };
            } else {
                return None
            }
        }
    }
}

impl <V : Clone> ByteTrieNode<V> {
    pub fn new() -> Self {
        Self {
            mask: [0u64; 4],
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
        if pos > 0b01000000 { c += self.mask[0].count_ones() as u8; }
        else { return c + (self.mask[0] & m).count_ones() as u8 }
        if pos > 0b10000000 { c += self.mask[1].count_ones() as u8; }
        else { return c + (self.mask[1] & m).count_ones() as u8 }
        if pos > 0b11000000 { c += self.mask[2].count_ones() as u8; }
        else { return c + (self.mask[2] & m).count_ones() as u8 }
        // println!("{} {:b} {} {}", pos, self.mask[3], m.count_ones(), c);
        return c + (self.mask[3] & m).count_ones() as u8;
    }

    #[inline]
    pub fn contains(&self, k: u8) -> bool {
        0 != (self.mask[((k & 0b11000000) >> 6) as usize] & (1u64 << (k & 0b00111111)))
    }

    #[inline]
    fn set(&mut self, k: u8) -> () {
        // println!("setting k {} : {} {:b}", k, ((k & 0b11000000) >> 6) as usize, 1u64 << (k & 0b00111111));
        self.mask[((k & 0b11000000) >> 6) as usize] |= 1u64 << (k & 0b00111111);
    }

    #[inline]
    fn clear(&mut self, k: u8) -> () {
        // println!("setting k {} : {} {:b}", k, ((k & 0b11000000) >> 6) as usize, 1u64 << (k & 0b00111111));
        self.mask[((k & 0b11000000) >> 6) as usize] &= !(1u64 << (k & 0b00111111));
    }

    pub fn insert(&mut self, k: u8, v: V) -> bool {
        let ix = self.left(k) as usize;
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
    pub unsafe fn get_unchecked(&self, k: u8) -> &V {
        let ix = self.left(k) as usize;
        // println!("pos ix {} {} {:b}", pos, ix, self.mask);
        self.values.get_unchecked(ix)
    }

    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, k: u8) -> &mut V {
        let ix = self.left(k) as usize;
        // println!("pos ix {} {} {:b}", pos, ix, self.mask);
        self.values.get_unchecked_mut(ix)
    }

    pub fn len(&self) -> usize {
        return (self.mask[0].count_ones() + self.mask[1].count_ones() + self.mask[2].count_ones() + self.mask[3].count_ones()) as usize;
    }
}

impl<V> ByteTrieNode<*mut ByteTrieNode<V>> {
    #[inline(always)] 
    fn get_child(&self, k: u8) -> &ByteTrieNode<V> {
        unsafe{ &**self.get_unchecked(k) }
    }
    #[inline(always)] 
    fn get_child_mut(&mut self, k: u8) -> &mut ByteTrieNode<V> {
        unsafe{ &mut **self.get_unchecked_mut(k) }
    }
}
