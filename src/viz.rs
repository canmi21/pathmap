use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::{DefaultHasher, Hash, Hasher};
use crate::trie_node::TrieNodeDowncast;
use gxhash::{HashSet, HashSetExt};
use smallvec::{SmallVec, ToSmallVec};
use crate::alloc::Allocator;
use crate::trie_map::BytesTrieMap;
use crate::trie_node::{TaggedNodeRef, TrieNodeODRc, NODE_ITER_FINISHED};
use crate::TrieValue;


pub struct DrawConfig {
    pub ascii: bool,
    pub share_values: bool,
    pub hide_values: bool,
    pub color_mix: bool,
}

struct NodeMeta {
    shared: u64
}

#[derive(Debug)]
enum NodeType {
    Dense, Pair, Tiny, Empty, Bridge, Unknown
}

enum DrawCmd {
    Node(u64, NodeType),
    Edge(u64, u64, SmallVec<[u8; 8]>),
    Value(u64, u64, SmallVec<[u8; 8]>)
}

struct DrawState {
    root: usize,
    values: HashMap<u64, u64>,
    nodes: HashMap<u64, (u64, NodeMeta)>,
    cmds: Vec<DrawCmd>
}


pub unsafe fn viz_btms<V : TrieValue + Debug + Hash>(btms: &[BytesTrieMap<V>], dc: &DrawConfig) {
    println!("flowchart LR");

    let mut ds = DrawState{ root: 0, values: HashMap::new(), nodes: HashMap::new(), cmds: vec![] };
    for btm in btms.iter() {
        unsafe { viz(&btm.root.get().as_ref().unwrap().as_ref().unwrap(), dc, &mut ds) };
        ds.root += 1;
    }

    for cmd in ds.cmds {
        match cmd {
            DrawCmd::Node(address, ntype) => {
                let address_string = format!("{address}");
                let address_str = address_string.as_str();
                // let rc = n.refcount();
                if let Some((_, meta)) = ds.nodes.get(&address) {
                    let color = match meta.shared {
                        0b000 => { "black" }
                        0b100 => { "red" }
                        0b010 => { "green" }
                        0b001 => { "blue" }
                        0b011 => { "#0aa" }
                        0b101 => { "#a0a" }
                        0b110 => { "#aa0" }
                        0b111 => { "gray" }
                        _ => todo!()
                    };
                    println!("g{address_str}@{{ shape: rect, label: \"{ntype:?}\"}}");
                    println!("style g{address_str} fill:{color}");
                } else {
                    println!("g{address_str}@{{ shape: rect, label: \"{ntype:?}\"}}");
                }
            }
            DrawCmd::Edge(src, dst, key_bytes) => {
                let debug_jump = format!("{:?}", key_bytes);
                let jump = if dc.ascii { std::str::from_utf8(&key_bytes[..]).unwrap_or_else(|_| debug_jump.as_str()) }
                else { debug_jump.as_str() };

                println!("g{src} --\"{jump:?}\"--> g{dst}");
            }
            DrawCmd::Value(parent, address, key_bytes) => {
                if dc.hide_values { continue }
                let debug_jump = format!("{:?}", key_bytes);
                let jump = if dc.ascii { std::str::from_utf8(&key_bytes[..]).unwrap_or_else(|_| debug_jump.as_str()) }
                else { debug_jump.as_str() };

                let address_string = format!("{parent}");
                let address_str = address_string.as_str();

                let value_address_string = format!("{address}");
                let value_address_str = value_address_string.as_str();

                let show_v = format!("{:?}", (address as *const V).as_ref().unwrap());

                println!("g{address_str} --\"{jump:?}\"--> v{value_address_str}{address_str}");
                println!("v{value_address_str}{address_str}@{{ shape: rounded, label: \"{show_v}\" }}");
            }
        }
    }
}

unsafe fn viz<V : TrieValue + Debug + Hash, A : Allocator>(n: &TrieNodeODRc<V, A>, dc: &DrawConfig, ds: &mut DrawState) {
    let address = n.shared_node_id();

    let bn = n.as_tagged();
    let ntype = match bn {
        TaggedNodeRef::DenseByteNode(_) => { NodeType::Dense }
        TaggedNodeRef::LineListNode(_) => { NodeType::Pair }
        TaggedNodeRef::TinyRefNode(_) => { NodeType::Tiny }
        // TaggedNodeRef::BridgeNode(_) => { NodeType::Bridge }
        TaggedNodeRef::CellByteNode(_) => { NodeType::Unknown }
        TaggedNodeRef::EmptyNode => { NodeType::Empty }
        _ => { panic!() }
    };
    ds.cmds.push(DrawCmd::Node(address, ntype));

    let mut token = bn.new_iter_token();
    while token != NODE_ITER_FINISHED {
        let (new_token, key_bytes, rec, value) = bn.next_items(token);

        if let Some(r) = rec {
            let other_address = r.shared_node_id();
            ds.cmds.push(DrawCmd::Edge(address, other_address, key_bytes.to_smallvec()));
            match ds.nodes.get_mut(&other_address) {
                None => {
                    viz(r, dc, ds);
                    ds.nodes.insert(other_address, (address, NodeMeta{ shared: 1 << ds.root }));
                }
                Some((parent, ref mut meta)) => {
                    meta.shared |= 1 << ds.root;
                }
            }
        }

        if let Some(v) = value {
            ds.cmds.push(DrawCmd::Value(address, v as *const V as u64, key_bytes.to_smallvec()));
        }

        token = new_token;
    }
}

#[cfg(test)]
mod test {
    use crate::zipper::{ZipperCreation, ZipperIteration, ZipperMoving, ZipperWriting};
    use super::*;

    #[test]
    fn small_viz() {
        let mut btm = BytesTrieMap::new();
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });

        unsafe { viz_btms(&[btm], &DrawConfig{ ascii: true, share_values: false, hide_values: false, color_mix: false }) };
    }

    #[test]
    fn joined_viz() {
        let mut a = BytesTrieMap::<usize>::new();
        let mut b = BytesTrieMap::<usize>::new();
        let rs = ["Abbotsford", "Abbottabad", "Abcoude", "Abdul Hakim", "Abdulino", "Abdullahnagar", "Abdurahmoni Jomi", "Abejorral", "Abelardo Luz"];
        for (i, path) in rs.into_iter().enumerate() {
            if i % 2 == 0 {
                a.insert(path, i);
            } else {
                b.insert(path, i);
            }
        }

        let joined = a.join(&b);

        unsafe { viz_btms(&[a, b, joined], &DrawConfig{ ascii: true, share_values: false, hide_values: false, color_mix: true }) };
    }

    #[test]
    fn fizzbuzz() {
        let n = 50;

        let mut space = BytesTrieMap::<()>::new();
        let mut zh = space.zipper_head();

        let mut m3_path = b"[2]".to_vec();
        let m3_symbol = "m3".as_bytes();
        m3_path.extend(b"<2>");
        m3_path.extend(m3_symbol);
        let mut m3_zipper = zh.write_zipper_at_exclusive_path(&m3_path[..]).unwrap();
        m3_zipper.descend_to(b"<4>");
        m3_zipper.graft_map(crate::utils::ints::gen_int_range::<(), 4, u32>(3, n as u32, 3, ()));
        m3_zipper.reset();

        let mut m5_path = b"[2]".to_vec();
        let m5_symbol = "m5".as_bytes();
        m5_path.extend(b"<2>");
        m5_path.extend(m5_symbol);
        let mut m5_zipper = zh.write_zipper_at_exclusive_path(&m5_path[..]).unwrap();
        m5_zipper.descend_to(b"<4>");
        m5_zipper.graft_map(crate::utils::ints::gen_int_range::<(), 4, u32>(5, n as u32, 5, ()));
        m5_zipper.reset();

        let mut r_path = b"[2]".to_vec();
        let r_symbol = "r".as_bytes();
        r_path.extend(b"<1>");
        r_path.extend(r_symbol);
        let mut r_zipper = zh.write_zipper_at_exclusive_path(&r_path[..]).unwrap();
        r_zipper.descend_to(b"<4>");
        r_zipper.graft_map(crate::utils::ints::gen_int_range::<(), 4, u32>(1, n as u32, 1, ()));
        r_zipper.reset();

        let mut m35_path = b"[2]".to_vec();
        let m35_symbol = "m35".as_bytes();
        m35_path.extend(b"<3>");
        m35_path.extend(m35_symbol);
        let mut m35_zipper = zh.write_zipper_at_exclusive_path(&m35_path[..]).unwrap();
        m35_zipper.meet_2(&m3_zipper, &m5_zipper);

        let mut m3n5_path = b"[2]".to_vec();
        let m3n5_symbol = "m3n5".as_bytes();
        m3n5_path.extend(b"<4>");
        m3n5_path.extend(m3n5_symbol);
        let mut m3n5_zipper = zh.write_zipper_at_exclusive_path(&m3n5_path[..]).unwrap();
        m3n5_zipper.graft(&m5_zipper);
        m3n5_zipper.subtract(&m3_zipper);

        let mut m5n3_path = b"[2]".to_vec();
        let m5n3_symbol = "m5n3".as_bytes();
        m5n3_path.extend(b"<4>");
        m5n3_path.extend(m5n3_symbol);
        let mut m5n3_zipper = zh.write_zipper_at_exclusive_path(&m5n3_path[..]).unwrap();
        m5n3_zipper.graft(&m3_zipper);
        m5n3_zipper.subtract(&m5_zipper);

        let mut m3m5_path = b"[2]".to_vec();
        let m3m5_symbol = "m3m5".as_bytes();
        m3m5_path.extend(b"<4>");
        m3m5_path.extend(m3m5_symbol);
        let mut m3m5_zipper = zh.write_zipper_at_exclusive_path(&m3m5_path[..]).unwrap();
        m3m5_zipper.graft(&m3_zipper);
        m3m5_zipper.join(&m5_zipper);

        let mut n3n5_path = b"[2]".to_vec();
        let n3n5_symbol = "n3n5".as_bytes();
        n3n5_path.extend(b"<4>");
        n3n5_path.extend(n3n5_symbol);
        let mut n3n5_zipper = zh.write_zipper_at_exclusive_path(&n3n5_path[..]).unwrap();
        n3n5_zipper.graft(&r_zipper);
        n3n5_zipper.subtract(&m3m5_zipper);
        drop(m3m5_zipper);

        drop(m3_zipper);
        drop(m5_zipper);
        drop(r_zipper);

        let mut FizzBuzz_path = b"[2]".to_vec();
        let FizzBuzz_symbol = "FizzBuzz".as_bytes();
        FizzBuzz_path.extend(b"<8>");
        FizzBuzz_path.extend(FizzBuzz_symbol);
        let mut fizz_buzz_zipper = zh.write_zipper_at_exclusive_path(&FizzBuzz_path[..]).unwrap();
        fizz_buzz_zipper.graft(&m35_zipper);
        drop(fizz_buzz_zipper);
        drop(m35_zipper);

        let mut Nothing_path = b"[2]".to_vec();
        let Nothing_symbol = "Nothing".as_bytes();
        Nothing_path.extend(b"<7>");
        Nothing_path.extend(Nothing_symbol);
        let mut nothing_zipper = zh.write_zipper_at_exclusive_path(&Nothing_path[..]).unwrap();
        nothing_zipper.graft(&n3n5_zipper);
        drop(nothing_zipper);
        drop(n3n5_zipper);

        let mut Fizz_path = b"[2]".to_vec();
        let Fizz_symbol = "Fizz".as_bytes();
        Fizz_path.extend(b"<4>");
        Fizz_path.extend(Fizz_symbol);
        let mut fizz_zipper = zh.write_zipper_at_exclusive_path(&Fizz_path[..]).unwrap();
        fizz_zipper.graft(&m3n5_zipper);
        drop(fizz_zipper);
        drop(m3n5_zipper);

        let mut Buzz_path = b"[2]".to_vec();
        let Buzz_symbol = "Buzz".as_bytes();
        Buzz_path.extend(b"<4>");
        Buzz_path.extend(Buzz_symbol);
        let mut buzz_zipper = zh.write_zipper_at_exclusive_path(&Buzz_symbol[..]).unwrap();
        buzz_zipper.graft(&m5n3_zipper);
        drop(buzz_zipper);
        drop(m5n3_zipper);

        drop(zh);

        println!("space size {}", space.val_count());

        unsafe { viz_btms(&[space], &DrawConfig{ ascii: false, share_values: false, hide_values: true, color_mix: true }) };
    }

}