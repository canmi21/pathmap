use std::fmt::Debug;
use std::ptr;
use crate::bit_sibling;
use crate::bytetrie::{BytesTrieMap, ByteTrieNode, CoFree};

// CZ2 uses a stack machine
// Store({a: 1}) // push

// CZ3 (incomplete) uses register machine

// Store({a: 1}, yym0)

// Content addressed abstract machine

// Store({a: 1}) // "you know what to do with this"

// ZAM (Warren Abstract Machine for triemaps)

// Store({a: 1}, [b, c])


enum Instruction {
    // == DESCEND ==
    Exact(u8),  // jumps to specific child
    Tail(u8),  // jumps to specific child and don't include

    Set([u64; 4]),  // jump to all children in mask
    Tails([u64; 4]),  // jump to all children in mask and don't include

    All(),  // jump to all
    Ignore(),  // jump to all and don't include

    Max(),  // Any
    Min(),  // Any

    // == ASCEND ==
    Head(u8),  // prefixes all with const

    // == SET OPS ==
    Union(),
    Intersection(),
    Subtraction(),

    // == CZ 2 OPS ==
    Restrict()
}

pub struct ReadZipper<'a, V> where V : Clone {
    pub root: &'a BytesTrieMap<V>,
    pub focus: &'a ByteTrieNode<CoFree<V>>,
    pub path: Vec<u8>,
    pub ancestors: Vec<&'a ByteTrieNode<CoFree<V>>>,
}

impl <'a, V : Clone + Debug> ReadZipper<'a, V> {
    pub fn new(btm: &'a BytesTrieMap<V>) -> Self {
        Self {
            root: btm,
            focus: &btm.root,
            path: vec![],
            ancestors: vec![],
        }
    }

    // moves deeper
    pub fn child(&mut self, k: u8) -> bool {
        match self.focus.get(k) {
            None => { false }
            Some(cf) => unsafe {
                match cf.subtree() {
                    None => { false }
                    Some(r) => {
                        self.path.push(k);
                        self.ancestors.push(self.focus);
                        self.focus = r;
                        true
                    }
                }
            }
        }
    }

    pub fn nth_child(&mut self, n: u8, forward: bool) -> bool {
        // println!("n {n}");
        // #iterations can be reduced by popcount(mask[i] & prefix)
        let mut child = -1;
        // this is not DRY but I lost the fight to the Rust compiler
        let pair = if forward { self.focus.values.iter().enumerate().find(|(_, v)| {
            let has_child = v.subtree().is_some();
            if has_child { child += 1; child == (n as i32) } else { false }
        }) } else { self.focus.values.iter().rev().enumerate().find(|(_, v)| {
            let has_child = v.subtree().is_some();
            if has_child { child += 1; child == (n as i32) } else { false }
        }) };
        match pair {
            None => { return false }
            Some((item, cf)) => {
                let mut i = if forward { 0 } else { 3 };
                let mut m = self.focus.mask[i];
                let mut c = 0;
                let mut c_ahead = m.count_ones() as usize;
                loop {
                    if item < c_ahead { break; }
                    if forward { i += 1} else { i -= 1 };
                    if i > 3 || i < 0 { return false }
                    m = self.focus.mask[i];
                    c = c_ahead;
                    c_ahead += m.count_ones() as usize;
                }

                let mut loc= 0;
                if forward {
                    loc = 63 - m.leading_zeros();
                    while c < item {
                        m ^= 1u64 << loc;
                        loc = 63 - m.leading_zeros();
                        c += 1;
                    }
                } else {
                    loc = m.trailing_zeros();
                    while c < item {
                        m ^= 1u64 << loc;
                        loc = m.trailing_zeros();
                        c += 1;
                    }
                }

                let prefix = (i << 6 | (loc as usize)) as u8;
                // println!("{:#066b}", self.focus.mask[i]);
                // println!("{i} {loc} {prefix}");
                debug_assert!(self.focus.contains(prefix));
                self.ancestors.push(self.focus);
                self.path.push(prefix);
                self.focus = cf.subtree().as_ref().unwrap();
                true
            }
        }
    }

    // stays on the same level
    pub fn sibling(&mut self, next: bool) -> bool {
        match self.ancestors.last() {
            None => { false }
            Some(parent) => {
                let k = self.path.last().unwrap(); // should be in-sync ancestors and non-empty
                let mut mask_i = ((k & 0b11000000) >> 6) as usize;
                let mut bit_i = k & 0b00111111;
                // println!("k {k}");

                loop {
                    // println!("loop");
                    let mut n = bit_sibling(bit_i, parent.mask[mask_i], !next);
                    // println!("{} {bit_i} {mask_i}", n == bit_i);
                    if n == bit_i { // outside of word
                        loop {
                            if next { mask_i += 1 } else { mask_i -= 1 };
                            if !(0 <= mask_i && mask_i < 4) { return false }
                            if parent.mask[mask_i] == 0 { continue }
                            n = parent.mask[mask_i].trailing_zeros() as u8; break;
                        }
                    }
                    bit_i = n;
                    // println!("{} {bit_i} {mask_i}", n == bit_i);
                    // println!("{:?}", parent.items().map(|(k, _)| k).collect::<Vec<_>>());
                    let sk = n | ((mask_i << 6) as u8);
                    // println!("candidate {}", sk);
                    debug_assert!(parent.contains(sk));
                    match unsafe { parent.get_unchecked(sk).subtree().as_ref() } {
                        None => {
                            // println!("{} {}", k, sk);
                            continue
                        }
                        Some(cs) => {
                            *self.path.last_mut().unwrap() = sk;
                            self.focus = cs;
                            return true
                        }
                    }
                }
            }
        }
    }

    // moves up
    pub fn parent(&mut self) -> bool {
        match self.ancestors.pop() {
            None => { false }
            Some(parent) => {
                self.focus = parent;
                self.path.pop();
                true
            }
        }
    }
}


// pub struct WriteZipper<'a, V> {
//     pub root: *mut BytesTrieMap<V>,
//     pub focus: *mut ByteTrieNode<CoFree<V>>,
//     pub path: Vec<u8>,
//     pub ancestors: Vec<*mut ByteTrieNode<CoFree<V>>>,
// }
//
// impl <'a, V : Clone + Debug> WriteZipper<'a, V> {
//     pub fn remove_children(m: [u64; 4]) {}
//
//     pub fn remove_child(k: u8) {}
//     pub fn remove_nth_child(n: u8) {}
//
//     pub fn remove_value(k: u8) {}
//     pub fn remove_nth_value(n: u8) {}
//
//     pub fn add_child(k: u8) {}
//     pub fn add_nth_child(n: u8) {}
//
//     pub fn add_value(k: u8) {}
//     pub fn add_nth_value(n: u8) {}
// }

trait Engine {
    // fn execute<V>(inp: &BytesTrieMap<V>, k: &[Instruction]) -> BytesTrieMap<V> {
    //     let mut out = BytesTrieMap::new();
    //     let mut inp_loc = &inp.root;
    //     let mut out_loc = &out.root;
    //
    //     match k[0] {
    //         Instruction::Exact(k) => {
    //             node.get()
    //         }
    //         Instruction::Set(_) => {}
    //         Instruction::Ignore() => {}
    //         Instruction::Any() => {}
    //     }
    //     if k.len() > 1 {
    //         for i in 0..k.len() - 1 {
    //             match node.get(k[i]) {
    //                 Some(cf) => {
    //                     match unsafe { cf.rec.as_ref() } {
    //                         Some(r) => { node = r }
    //                         None => { return None }
    //                     }
    //                 }
    //                 None => { return None }
    //             }
    //         }
    //     }
    //
    //     out
    // }
}