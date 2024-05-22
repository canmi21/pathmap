use std::fmt::{Debug, Formatter};
use std::ptr::slice_from_raw_parts;

use rclite::Rc;

#[derive(Clone)]
pub struct ByteTrieNode<V> {
    pub(crate) mask: [u64; 4],
    pub(crate) values: Vec<V>,
    pub(crate) parent_val: Option<V>, //TODO, consider packing this into `values`
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

                            match cf.subtree() {
                                None => {}
                                Some(rec) => {
                                    self.prefix = k.clone();
                                    self.btnis.push(ByteTrieNodeIter::new(&rec));
                                }
                            }

                            match &cf.value() {
                                None => {}
                                Some(v) => {
                                    return Some((k, &v))
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum CoFree<V> {
    Branch(Rc<ByteTrieNode<CoFree<V>>>),
    Leaf(V)
}

impl<V: Clone> CoFree<V> {
    #[inline]
    pub fn new_with_subtree(subtree: Rc<ByteTrieNode<CoFree<V>>>) -> Self {
        Self::Branch(subtree)
    }
    #[inline]
    pub fn new_with_val(val: V) -> Self {
        Self::Leaf(val)
    }
    #[inline]
    pub fn subtree(&self) -> Option<&ByteTrieNode<CoFree<V>>> {
        match self {
            Self::Branch(ptr) => Some(ptr),
            Self::Leaf(_) => None
        }
    }
    /// Returns a mutable version of the ByteTrieNode using copy-on-write semantics.
    /// Promotes a Leaf to a Branch if called on a leaf
    #[inline]
    pub fn subtree_mut(&mut self) -> Option<&mut ByteTrieNode<CoFree<V>>> {
        match self {
            Self::Branch(ptr) => Some(Rc::make_mut(ptr)),
            Self::Leaf(_) => {
                let mut new_branch_cf = Self::Branch(Rc::new(ByteTrieNode::new()));
                std::mem::swap(self, &mut new_branch_cf);
                let old_val_cf = new_branch_cf;
                let ptr = self.subtree_mut();
                let btn = ptr.unwrap();
                btn.parent_val = Some(old_val_cf);
                Some(btn)
            }
        }
    }
    #[inline]
    pub fn value(&self) -> Option<&V> {
        match self {
            Self::Leaf(v) => Some(v),
            Self::Branch(ptr) => ptr.parent_val.as_ref().and_then(|cf| cf.value()),
        }
    }
    #[inline]
    pub fn value_mut(&mut self) -> Option<&mut V> {
        match self {
            Self::Leaf(v) => Some(v),
            Self::Branch(ptr) => {
                let ptr = Rc::make_mut(ptr);
                ptr.parent_val.as_mut().and_then(|cf| cf.value_mut())
            },
        }
    }
    #[inline]
    pub fn set_value(&mut self, val: V) -> Option<V> {
        match self {
            Self::Leaf(v) => {
                let mut old_val = val;
                core::mem::swap(&mut old_val, v);
                Some(old_val)
            },
            Self::Branch(ptr) => {
                let ptr = Rc::make_mut(ptr);
                match &mut ptr.parent_val {
                    None => {
                        ptr.parent_val = Some(Self::new_with_val(val));
                        None
                    }
                    Some(old_val_cf) => {
                        old_val_cf.set_value(val)
                    }
                }
            },
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

                            match  cf.subtree() {
                                None => {
                                    self.nopush = true;
                                }
                                Some(rec) => {
                                    self.nopush = false;
                                    self.btnis.push(ByteTrieNodeIter::new(rec));
                                }
                            }

                            match &cf.value() {
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
pub struct BytesTrieMap<V> {
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
    #[allow(invalid_reference_casting)] //TODO: Take this away when the QUESTION is answered
    pub(crate) fn at<K: AsRef<[u8]>>(&self, k: K) -> Option<&mut BytesTrieMap<V>> {
        let k = k.as_ref();
        let mut node = &self.root;

        if k.len() > 1 {
            for i in 0..k.len() - 1 {
                match node.get(k[i]) {
                    Some(cf) => {
                        match cf.subtree() {
                            Some(r) => { node = r }
                            None => { return None }
                        }
                    }
                    None => { return None }
                }
            }
        }

        node.get(k[k.len() - 1]).and_then(|cf| cf.subtree()).map(|subnode| 
            //SAFETY: the type-cast should be ok, because BytesTrieMap<V> is a #[repr(transparent)]
            // wrapper around ByteTrieNode<CoFree<V>>.
            //WARNING.  The cast_mut() is actually UNSAFE!!  See QUESTION above
            unsafe{ &mut *((&*subnode) as *const ByteTrieNode<CoFree<V>>).cast_mut().cast()  }
        )
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
    /// NOTE: The usual Rust convention for insert is to return the old value if a value was replaced.
    /// LP: I changed the behavior to always replace, rather than to do nothing and return true if
    ///   the slot was occupied
    pub fn insert<K: AsRef<[u8]>>(&mut self, k: K, v: V) -> bool {
        let k = k.as_ref();
        let mut node = &mut self.root;

        if k.len() > 1 {
            for i in 0..k.len() - 1 {
                let cf = node.update(k[i], || {
                    let l = ByteTrieNode::new();
                    CoFree::new_with_subtree(Rc::new(l))
                });

                node = cf.subtree_mut().unwrap();
            }
        }

        let lk = k[k.len() - 1];
        if node.contains(lk) {
            let cf = unsafe{ node.get_unchecked_mut(lk) };
            cf.set_value(v).is_some()
        } else {
            let cf = CoFree::new_with_val(v);
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
                let cf = node.update(k[i], || {
                    let l = ByteTrieNode::new();
                    CoFree::new_with_subtree(Rc::new(l))
                });

                node = cf.subtree_mut().unwrap();
            }
        }

        let lk = k[k.len() - 1];
        let cf = node.update(lk, || CoFree::new_with_val(default()));
        cf.value_mut().unwrap()
    }

    pub fn get<K: AsRef<[u8]>>(&self, k: K) -> Option<&V> {
        let k = k.as_ref();
        let mut node = &self.root;

        if k.len() > 1 {
            for i in 0..k.len() - 1 {
                match node.get(k[i]) {
                    Some(cf) => {
                        match cf.subtree() {
                            Some(r) => { node = r }
                            None => { return None }
                        }
                    }
                    None => { return None }
                }
            }
        }

        match node.get(k[k.len() - 1]) {
            None => None,
            Some(cf) => cf.value()
        }
    }

    fn cofreelen(btn: &ByteTrieNode<CoFree<V>>) -> usize {
        return btn.values.iter().rfold(0, |t, cf| {
            t + cf.value().is_some() as usize + cf.subtree().map(|r| Self::cofreelen(r)).unwrap_or(0)
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

#[derive(Clone)]
pub struct ShortTrieMap<V> {
    pub(crate) root: ByteTrieNode<Option<Rc<ByteTrieNode<V>>>>
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
        self.root.items().flat_map(|(k1, l1)| {
            l1.as_ref().unwrap().items().map(move |(k2, v)| ((k1 as u16) | ((k2 as u16) << 8), v))
        })
    }

    pub fn contains(&self, k: u16) -> bool {
        let k1 = k as u8;
        let k2 = (k >> 8) as u8;
        if self.root.contains(k1) {
            let rl1 = unsafe{ self.root.get_unchecked(k1) };
            rl1.as_ref().unwrap().contains(k2)
        } else {
            false
        }
    }

    pub fn insert(&mut self, k: u16, v: V) -> bool {
        let k1 = k as u8;
        let k2 = (k >> 8) as u8;
        if self.root.contains(k1) {
            let rl1 = unsafe{ self.root.get_unchecked_mut(k1) };
            Rc::make_mut(rl1.as_mut().unwrap()).insert(k2, v)
        } else {
            let mut l1 = ByteTrieNode::new();
            l1.insert(k2, v);
            let rl1 = Some(Rc::new(l1));
            self.root.insert(k1, rl1);
            false
        }
    }

    pub fn remove(&mut self, k: u16) -> Option<V> {
        let k1 = k as u8;
        let k2 = (k >> 8) as u8;
        match self.root.get_mut(k1) {
            Some(btn) => {
                let btnr = Rc::make_mut(btn.as_mut().unwrap());
                let r = btnr.remove(k2);
                if btnr.len() == 0 {
                    btnr.remove(k1);
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
            let rl1 = &**l1.as_ref().unwrap();
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
                self.w = *unsafe{ self.btn.mask.get_unchecked(self.i as usize) };
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
            values: Vec::new(),
            parent_val: None,
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
            let node_ref = unsafe { self.values.get_unchecked_mut(ix) };
            *node_ref = v;
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
        unsafe { self.values.get_unchecked_mut(ix) }
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
            unsafe { Some(self.values.get_unchecked(ix)) }
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, k: u8) -> Option<&mut V> {
        if self.contains(k) {
            let ix = self.left(k) as usize;
            unsafe { Some(self.values.get_unchecked_mut(ix)) }
        } else {
            None
        }
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
