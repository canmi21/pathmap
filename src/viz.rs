use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::{Hash, DefaultHasher, Hasher};
use std::io::{self, Write};
use smallvec::{SmallVec, ToSmallVec};
use crate::alloc::Allocator;
use crate::trie_map::PathMap;
use crate::trie_node::{TaggedNodeRef, TrieNodeODRc, NODE_ITER_FINISHED};
use crate::zipper::*;
use crate::TrieValue;

/// Configuration settings for rendering a graph of a `pathmap` trie
pub struct DrawConfig {
    /// If `true`, render path substrings as ascii, otherwise render them as strings of
    /// byte-sized numbers
    pub ascii: bool,
    /// If `true`, skips rendering of the paths that terminate in values, otherwise renders
    /// all paths in the trie
    pub hide_value_paths: bool,
    /// If `true`, skips rendering values, but still renders paths leading to values unless
    /// `hide_value_paths` is also `true`
    pub minimize_values: bool,
    /// If `true`, renders the trie irrespective of the pysical (in-memory) representation,
    /// otherwise also renders the nodes that comprise the layout of the trie structure
    pub logical: bool,
}

impl Default for DrawConfig {
    fn default() -> Self {
        Self{
            ascii: false,
            hide_value_paths: false,
            minimize_values: false,
            logical: true,
        }
    }
}

struct NodeMeta {
    /// bit-mask indicating which top-level pathmaps include the node
    shared: u64,
    /// Number of references to this node from upstream
    ref_cnt: usize,
    /// Whether a graph node has already been rendered for this trie node
    taken: bool,
}

#[derive(Debug)]
enum NodeType {
    Dense, Pair, Tiny, Empty, Unknown
}

enum DrawCmd {
    Node(u64, NodeType),
    Edge(u64, u64, SmallVec<[u8; 8]>),
    Value(u64, u64, SmallVec<[u8; 8]>),
    Map(usize, u64),
}

struct DrawState {
    root: usize,
    nodes: HashMap<u64, NodeMeta>,
    cmds: Vec<DrawCmd>
}

/// Output [Mermaid](https://mermaid.js.org) markup commands to render a graph of the trie
/// within the provided [PathMap]s
pub fn viz_maps<V : TrieValue + Debug + Hash, W: Write>(btms: &[PathMap<V>], dc: &DrawConfig, mut out: W) -> io::Result<()> {
    writeln!(out, "flowchart LR")?;
    let mut ds = DrawState{ root: 0, nodes: HashMap::new(), cmds: vec![] };

    if dc.logical {
        for btm in btms.iter() {
            pre_init_node_hashes(btm.root().unwrap(), &mut ds);
            ds.root += 1;
        }
    }

    ds.root = 0;
    for btm in btms.iter() {
        if dc.logical {
            viz_zipper_logical(btm.read_zipper(), dc, &mut ds);
        } else {
            viz_map_physical(btm, dc, &mut ds);
        }
        ds.root += 1;
    }

    for cmd in ds.cmds {
        match cmd {
            DrawCmd::Map(map_idx, root_node_addr) => {
                if dc.logical {
                    //Draw the map *as* its root node
                    writeln!(out, "g{root_node_addr}@{{ shape: cylinder, label: \"PathMap[{map_idx}]\"}}")?;
                } else {
                    //Draw the map connecting to its root node
                    writeln!(out, "m{map_idx}@{{ shape: cylinder, label: \"PathMap[{map_idx}]\"}}")?;
                    writeln!(out, "m{map_idx} --> g{root_node_addr}")?;
                }
            },
            DrawCmd::Node(address, ntype) => {
                if !dc.logical {
                    //Render the node as a box
                    if let Some(meta) = ds.nodes.get(&address) {
                        let color = color_for_bitmask(meta.shared);
                        writeln!(out, "g{address}@{{ shape: rect, label: \"{ntype:?}\"}}")?;
                        writeln!(out, "style g{address} fill:{color}")?;
                    } else {
                        writeln!(out, "g{address}@{{ shape: rect, label: \"{ntype:?}\"}}")?;
                    }
                } else {
                    //Render it as a tiny dot (as small as Mermaid will draw)
                    let color = if let Some(meta) = ds.nodes.get(&address) {
                       color_for_bitmask(meta.shared)
                    } else {
                        "black"
                    };
                    writeln!(out, "g{address}@{{ shape: circle, label: \".\"}}")?;
                    writeln!(out, "style g{address} fill:{color},stroke:none,color:transparent,font-size:0px")?;
                }
            }
            DrawCmd::Edge(src, dst, key_bytes) => {
                let debug_jump = format!("{:?}", key_bytes);
                let jump = if dc.ascii { std::str::from_utf8(&key_bytes[..]).unwrap_or_else(|_| debug_jump.as_str()) }
                else { debug_jump.as_str() };

                if jump.len() > 0 {
                    writeln!(out, "g{src} --\"{jump:?}\"--> g{dst}")?;
                } else {
                    writeln!(out, "g{src} --> g{dst}")?;
                }
            }
            DrawCmd::Value(parent, address, key_bytes) => {
                if dc.hide_value_paths { continue }
                let debug_jump = format!("{:?}", key_bytes);
                let jump = if dc.ascii { std::str::from_utf8(&key_bytes[..]).unwrap_or_else(|_| debug_jump.as_str()) }
                else { debug_jump.as_str() };

                let address_string = format!("{parent}");
                let address_str = address_string.as_str();

                let value_address_string = format!("{address}");
                let value_address_str = value_address_string.as_str();

                let show_v = format!("{:?}", unsafe{ (address as *const V).as_ref().unwrap() });

                if jump.len() > 0 {
                    writeln!(out, "g{address_str} --\"{jump:?}\"--> v{value_address_str}{address_str}")?;
                } else {
                    writeln!(out, "g{address_str} --> v{value_address_str}{address_str}")?;
                }
                if dc.minimize_values {
                    writeln!(out, "v{value_address_str}{address_str}@{{ shape: circle, label: \".\"}}")?;
                    writeln!(out, "style v{value_address_str}{address_str} fill:black,stroke:none,color:transparent,font-size:0px")?;
                } else {
                    writeln!(out, "v{value_address_str}{address_str}@{{ shape: rounded, label: \"{show_v}\" }}")?;
                }
            },
        }
    }
    Ok(())
}

fn color_for_bitmask(mask: u64) -> &'static str {
    match mask {
        0b000 => { "black" }
        0b100 => { "red" }
        0b010 => { "green" }
        0b001 => { "blue" }
        0b011 => { "#0aa" }
        0b101 => { "#a0a" }
        0b110 => { "#aa0" }
        0b111 => { "gray" }
        _ => todo!()
    }
}

fn pre_init_node_hashes<V : TrieValue + Debug + Hash, A : Allocator>(node: &TrieNodeODRc<V, A>, ds: &mut DrawState) {
    if update_node_hash(node, ds) {
        let node_ref = node.as_tagged();
        let mut token = node_ref.new_iter_token();
        while token != NODE_ITER_FINISHED {
            let (new_token, _key_bytes, rec, _value) = node_ref.next_items(token);
            if let Some(child) = rec {
                pre_init_node_hashes(child, ds);
            }
            token = new_token;
        }
    }
}

/// Updates the node hash table for a single node.  Returns `true` if a new hash entry was
/// created, indicating the calling code should descend the subtrie recursively
fn update_node_hash<V : TrieValue + Debug + Hash, A : Allocator>(node: &TrieNodeODRc<V, A>, ds: &mut DrawState) -> bool {
    let node_addr = node.shared_node_id();
    match ds.nodes.get_mut(&node_addr) {
        None => {
            ds.nodes.insert(node_addr, NodeMeta{ shared: 1 << ds.root, ref_cnt: 1, taken: false });
            true
        }
        Some(meta) => {
            meta.shared |= 1 << ds.root;
            meta.ref_cnt += 1;
            false
        }
    }
}

fn viz_zipper_logical<V : TrieValue + Debug + Hash, Z: zipper_priv::ZipperPriv + ZipperMoving + ZipperIteration + ZipperValues<V>>(mut z: Z, dc: &DrawConfig, ds: &mut DrawState) {
    let root_focus = z.get_focus();
    let root_node = root_focus.borrow().unwrap();
    let root_node_id = hash_pair(root_node.shared_node_id(), &[]);
    ds.cmds.push(DrawCmd::Map(ds.root, root_node_id));

    //We keep two separate stacks.  `trie_stack` is the physical nodes, and `graph_stack` is
    // the nodes that will get rendered.  This is necessary because the correspondence is
    // not straightforward.  For example, many physical trie nodes can end up subsumed under
    // a single graph edge, in the case of a long straight path, but it's also the case that
    // a single physical trie node can produce many graph nodes, when a trie node contains
    // internal graph structure.
    let mut trie_stack = vec![(0, root_node.shared_node_id())];
    let mut graph_stack = vec![(0, root_node_id)];
    while z.to_next_step() {
        let path = z.path();

        //See if we have ascended and therefore need to pop the trie stack
        while path.len() <= trie_stack.last().unwrap().0 {
            trie_stack.pop();
        }

        //See if we have descended into a new node and therefore need to push onto the trie stack
        let new_focus = z.get_focus();
        let mut node_is_shared = false;
        let mut skip_node = false;
        if let Some(node) = new_focus.borrow() {
            let node_addr = node.shared_node_id();
            trie_stack.push((path.len(), node_addr));
            if let Some(meta) = ds.nodes.get_mut(&node_addr) {
                if meta.ref_cnt > 1 {
                    node_is_shared = true;
                }
                skip_node = meta.taken;
                meta.taken = true;
            }
        }

        let node_addr = trie_stack.last().unwrap().1;
        let node_key = &path[trie_stack.last().unwrap().0..];

        //See if we have ascended and therefore need to pop the graph stack
        while path.len() <= graph_stack.last().unwrap().0 {
            graph_stack.pop();
        }

        //See if we have met one of the conditions to push a node onto the graph stack
        if z.child_count() > 1 || (z.is_val() && z.child_count() == 1 && !dc.hide_value_paths) || node_is_shared {
            let parent_node_id = graph_stack.last().unwrap().1;
            let edge_path = &path[graph_stack.last().unwrap().0..];

            let graph_node_id = hash_pair(node_addr, node_key);
            graph_stack.push((z.path().len(), graph_node_id));

            ds.cmds.push(DrawCmd::Edge(parent_node_id, graph_node_id, edge_path.to_smallvec()));
            if !skip_node {
                ds.cmds.push(DrawCmd::Node(graph_node_id, NodeType::Unknown));
            }
        }

        let graph_node_id = graph_stack.last().unwrap().1;
        let edge_path = &path[graph_stack.last().unwrap().0..];

        if let Some(v) = z.val() {
            ds.cmds.push(DrawCmd::Value(graph_node_id, v as *const V as u64, edge_path.to_smallvec()));
        }

        //Skip a whole branch if we've already rendered it elsewhere
        if skip_node {
            while !z.to_next_sibling_byte() {
                if !z.ascend_byte() {
                    return; //We skipped all the way to the root
                }
            }
        }
    }
}

/// A simple function to hash an address with a partial path, to make new node_ids for logical nodes
fn hash_pair(addr: u64, key: &[u8]) -> u64 {
    if key.len() > 0 {
        let mut hasher = DefaultHasher::new();
        addr.hash(&mut hasher);
        key.hash(&mut hasher);
        hasher.finish()
    } else {
        addr
    }
}

fn viz_map_physical<V : TrieValue + Debug + Hash, A : Allocator>(map: &PathMap<V, A>, dc: &DrawConfig, ds: &mut DrawState) {
    let root_node = map.root().unwrap();
    ds.cmds.push(DrawCmd::Map(ds.root, root_node.shared_node_id()));
    update_node_hash(root_node, ds);
    viz_node_physical(root_node, dc, ds);
}

fn viz_node_physical<V : TrieValue + Debug + Hash, A : Allocator>(n: &TrieNodeODRc<V, A>, dc: &DrawConfig, ds: &mut DrawState) {
    let address = n.shared_node_id();

    let bn = n.as_tagged();
    let ntype = match bn {
        TaggedNodeRef::DenseByteNode(_) => { NodeType::Dense }
        TaggedNodeRef::LineListNode(_) => { NodeType::Pair }
        TaggedNodeRef::TinyRefNode(_) => { NodeType::Tiny }
        // TaggedNodeRef::BridgeNode(_) => { NodeType::Bridge }
        TaggedNodeRef::CellByteNode(_) => { NodeType::Unknown }
        TaggedNodeRef::EmptyNode => { NodeType::Empty }
    };
    ds.cmds.push(DrawCmd::Node(address, ntype));

    let mut token = bn.new_iter_token();
    while token != NODE_ITER_FINISHED {
        let (new_token, key_bytes, rec, value) = bn.next_items(token);

        if let Some(r) = rec {
            let other_address = r.shared_node_id();
            ds.cmds.push(DrawCmd::Edge(address, other_address, key_bytes.to_smallvec()));
            if update_node_hash(r, ds) {
                viz_node_physical(r, dc, ds);
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
    use crate::zipper::{ZipperCreation, ZipperMoving, ZipperWriting};
    use super::*;

    #[test]
    fn small_viz() {
        let mut btm = PathMap::new();
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });

        let mut out_buf = Vec::new();
        viz_maps(&[btm], &DrawConfig{ ascii: true, hide_value_paths: false, minimize_values: false, logical: false }, &mut out_buf).unwrap();
        // println!("{}", String::from_utf8_lossy(&out_buf));
    }

    #[test]
    fn logical_viz_tiny() {
        let mut btm = PathMap::new();
        let rs = ["arrow", "bow", "cannon"];
        rs.iter().for_each(|path| { btm.insert(path.as_bytes(), ()); });

        let mut out_buf = Vec::new();
        viz_maps(&[btm], &DrawConfig{ ascii: true, hide_value_paths: false, minimize_values: true, logical: true }, &mut out_buf).unwrap();
        // println!("{}", String::from_utf8_lossy(&out_buf));
    }

    #[test]
    fn logical_viz_small() {
        let mut btm = PathMap::new();
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });

        let mut out_buf = Vec::new();
        viz_maps(&[btm], &DrawConfig{ ascii: true, hide_value_paths: false, minimize_values: false, logical: true }, &mut out_buf).unwrap();
        // println!("{}", String::from_utf8_lossy(&out_buf));
    }

    #[test]
    fn joined_viz() {
        let mut a = PathMap::<usize>::new();
        let mut b = PathMap::<usize>::new();
        let rs = ["Abbotsford", "Abbottabad", "Abcoude", "Abdul Hakim", "Abdulino", "Abdullahnagar", "Abdurahmoni Jomi", "Abejorral", "Abelardo Luz"];
        for (i, path) in rs.into_iter().enumerate() {
            if i % 2 == 0 {
                a.insert(path, i);
            } else {
                b.insert(path, i);
            }
        }

        let joined = a.join(&b);

        let mut out_buf = Vec::new();
        viz_maps(&[a, b, joined], &DrawConfig{ ascii: true, hide_value_paths: false, minimize_values: false, logical: false }, &mut out_buf).unwrap();
        // println!("{}", String::from_utf8_lossy(&out_buf));
    }

    #[test]
    fn fizzbuzz() {
        let n = 50;

        let mut space = PathMap::<()>::new();
        let zh = space.zipper_head();

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
        m3n5_zipper.subtract_into(&m3_zipper, true);

        let mut m5n3_path = b"[2]".to_vec();
        let m5n3_symbol = "m5n3".as_bytes();
        m5n3_path.extend(b"<4>");
        m5n3_path.extend(m5n3_symbol);
        let mut m5n3_zipper = zh.write_zipper_at_exclusive_path(&m5n3_path[..]).unwrap();
        m5n3_zipper.graft(&m3_zipper);
        m5n3_zipper.subtract_into(&m5_zipper, true);

        let mut m3m5_path = b"[2]".to_vec();
        let m3m5_symbol = "m3m5".as_bytes();
        m3m5_path.extend(b"<4>");
        m3m5_path.extend(m3m5_symbol);
        let mut m3m5_zipper = zh.write_zipper_at_exclusive_path(&m3m5_path[..]).unwrap();
        m3m5_zipper.graft(&m3_zipper);
        m3m5_zipper.join_into(&m5_zipper);

        let mut n3n5_path = b"[2]".to_vec();
        let n3n5_symbol = "n3n5".as_bytes();
        n3n5_path.extend(b"<4>");
        n3n5_path.extend(n3n5_symbol);
        let mut n3n5_zipper = zh.write_zipper_at_exclusive_path(&n3n5_path[..]).unwrap();
        n3n5_zipper.graft(&r_zipper);
        n3n5_zipper.subtract_into(&m3m5_zipper, true);
        drop(m3m5_zipper);

        drop(m3_zipper);
        drop(m5_zipper);
        drop(r_zipper);

        let mut fizzbuzz_path = b"[2]".to_vec();
        let fizzbuzz_symbol = "FizzBuzz".as_bytes();
        fizzbuzz_path.extend(b"<8>");
        fizzbuzz_path.extend(fizzbuzz_symbol);
        let mut fizz_buzz_zipper = zh.write_zipper_at_exclusive_path(fizzbuzz_path).unwrap();
        fizz_buzz_zipper.graft(&m35_zipper);
        drop(fizz_buzz_zipper);
        drop(m35_zipper);

        let mut nothing_path = b"[2]".to_vec();
        let nothing_symbol = "Nothing".as_bytes();
        nothing_path.extend(b"<7>");
        nothing_path.extend(nothing_symbol);
        let mut nothing_zipper = zh.write_zipper_at_exclusive_path(nothing_path).unwrap();
        nothing_zipper.graft(&n3n5_zipper);
        drop(nothing_zipper);
        drop(n3n5_zipper);

        let mut fizz_path = b"[2]".to_vec();
        let fizz_symbol = "Fizz".as_bytes();
        fizz_path.extend(b"<4>");
        fizz_path.extend(fizz_symbol);
        let mut fizz_zipper = zh.write_zipper_at_exclusive_path(fizz_path).unwrap();
        fizz_zipper.graft(&m3n5_zipper);
        drop(fizz_zipper);
        drop(m3n5_zipper);

        let mut buzz_path = b"[2]".to_vec();
        let buzz_symbol = "Buzz".as_bytes();
        buzz_path.extend(b"<4>");
        buzz_path.extend(buzz_symbol);
        let mut buzz_zipper = zh.write_zipper_at_exclusive_path(buzz_path).unwrap();
        buzz_zipper.graft(&m5n3_zipper);
        drop(buzz_zipper);
        drop(m5n3_zipper);

        drop(zh);

        // println!("space size {}", space.val_count());

        let mut out_buf = Vec::new();
        viz_maps(&[space], &DrawConfig{ ascii: false, hide_value_paths: true, minimize_values: true, logical: false }, &mut out_buf).unwrap();
        // println!("{}", String::from_utf8_lossy(&out_buf));
    }

}
