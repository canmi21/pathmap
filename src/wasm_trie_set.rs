use std::io::{Read, Write};
use std::ptr;
use flate2::Compression;
use wasm_bindgen::prelude::*;
use js_sys;
use crate::ring::Lattice;
use crate::trie_map::*;
use crate::trie_node::TrieNodeODRc;
use crate::zipper::*;

#[wasm_bindgen]
struct BytesTrieSet {
  btm: BytesTrieMap<()>
}

#[wasm_bindgen]
pub fn empty() -> BytesTrieSet {
  BytesTrieSet { btm: BytesTrieMap::<()>::new() }
}

#[wasm_bindgen]
pub fn range_be_u32(start: u32, stop: u32, step: u32) -> BytesTrieSet {
  BytesTrieSet { btm: BytesTrieMap::<()>::range::<true, u32>(start, stop, step, ()) }
}

#[wasm_bindgen]
pub fn union(x: &BytesTrieSet, y: &BytesTrieSet) -> BytesTrieSet {
  BytesTrieSet { btm: x.btm.join(&y.btm) }
}

#[wasm_bindgen]
pub fn intersection(x: &BytesTrieSet, y: &BytesTrieSet) -> BytesTrieSet {
  BytesTrieSet { btm: x.btm.meet(&y.btm) }
}

#[wasm_bindgen]
pub fn restriction(x: &BytesTrieSet, y: &BytesTrieSet) -> BytesTrieSet {
  BytesTrieSet { btm: x.btm.restrict(&y.btm) }
}

#[wasm_bindgen]
pub fn subtraction(x: &BytesTrieSet, y: &BytesTrieSet) -> BytesTrieSet {
  BytesTrieSet { btm: x.btm.subtract(&y.btm) }
}

#[wasm_bindgen]
pub fn raffination(x: &BytesTrieSet, y: &BytesTrieSet) -> BytesTrieSet {
  // not a performant implementation
  BytesTrieSet { btm: x.btm.subtract(&x.btm.restrict(&y.btm)) }
}

#[wasm_bindgen]
pub fn decapitation(x: &BytesTrieSet, k: usize) -> BytesTrieSet {
  let mut c = x.btm.clone();
  let mut wz = c.write_zipper();
  wz.drop_head(k);
  BytesTrieSet { btm: c }
}

#[wasm_bindgen]
pub fn head(x: &BytesTrieSet, k: usize) -> BytesTrieSet {
  // not a performant implementation
  let mut c = BytesTrieMap::new();
  let mut rz = x.btm.read_zipper();
  for i in 1..k+1 {
    if rz.descend_first_k_path(i) {
      loop {
        c.insert(rz.path(), ());
        if !rz.to_next_k_path(i) { break }
      }
    }
  }
  BytesTrieSet { btm: c }
}

#[wasm_bindgen]
pub fn contains(m: &BytesTrieSet, k: &[u8]) -> bool {
  m.btm.contains(k)
}

#[wasm_bindgen]
pub fn paths(bts: &BytesTrieSet) -> js_sys::Array {
  let mut rz = bts.btm.read_zipper();
  let mut v = js_sys::Array::new();
  while let Some(_) = rz.to_next_val() {
    v.push(&js_sys::Uint8Array::from(rz.path()));
  }
  v
}

fn json_intern(node: &TrieNodeODRc<()>, s: &mut Vec<u8>) {
  let bnode = node.borrow();
  let cm = bnode.node_branches_mask(&[]);
  s.push(b'{');
  let mut n = 0;
  for i in 0..4 {
    let mut lm = cm[i];
    while lm != 0 {
      let index = lm.trailing_zeros();

      let key_byte = 64*(i as u8) + (index as u8);
      match bnode.get_node_at_key(&[key_byte]).into_option() {
        None => {
          if n != 0 { s.push(b',') }
          s.push(b'"');
          s.extend(key_byte.to_string().as_bytes());
          s.push(b'"');
          s.push(b':');
          s.push(b'{');
          s.push(b'}');
          n += 1;
        }
        Some(r) => {
          if n != 0 { s.push(b',') }
          s.push(b'"');
          s.extend(key_byte.to_string().as_bytes());
          s.push(b'"');
          s.push(b':');
          json_intern(&r, s);
          n += 1;
        }
      }

      lm ^= 1u64 << index;
    }
  }
  s.push(b'}');
}

fn serialize_intern(bts: &BytesTrieSet) -> Vec<u8> {
  // serializing a trie as an array of paths is criminal
  let mut buf = vec![];
  let mut rz = bts.btm.read_zipper();
  while let Some(_) = rz.to_next_val() {
    let p = rz.path();
    let l = p.len();
    buf.extend_from_slice(l.to_le_bytes().as_slice());
    buf.extend_from_slice(p);
  }
  buf
}

fn deserialize_intern(sv: &[u8]) -> BytesTrieSet {
  // sue me
  let mut btm = BytesTrieMap::new();
  let mut i = 0;
  while i < sv.len() {
    let l = usize::from_le_bytes((&sv[i..i+size_of::<usize>()]).try_into().unwrap());
    i += size_of::<usize>();
    btm.insert(&sv[i..i+l], ());
    i += l;
  }
  BytesTrieSet { btm }
}

#[wasm_bindgen]
pub fn object(bts: &BytesTrieSet) -> js_sys::Object {
  let mut s = vec![];
  json_intern(bts.btm.root(), &mut s);
  let r = js_sys::JSON::parse(unsafe { std::str::from_utf8_unchecked(&s[..]) });
  r.unwrap_or_else(|e| e).into()
}

#[wasm_bindgen]
pub fn serialize(bts: &BytesTrieSet) -> Box<[u8]> {
  // writing chunk in-tandem with the encoder would be better (and totally doable)
  let serialized = serialize_intern(bts);
  let mut compressor = flate2::write::GzEncoder::new(Vec::new(), Compression::default());
  compressor.write_all(&serialized[..]).unwrap();
  compressor.finish().unwrap().into_boxed_slice()
}

#[wasm_bindgen]
pub fn deserialize(jsbs: &js_sys::Uint8Array) -> BytesTrieSet {
  // reading chunks in-tandem with the decoder would be better (and totally doable)
  let bs = jsbs.to_vec();
  let mut decompressor = flate2::read::GzDecoder::new(&bs[..]);
  let mut buf = vec![];
  decompressor.read_to_end(&mut buf).unwrap();
  deserialize_intern(&buf[..])
}

fn d3_json_intern(node: &TrieNodeODRc<()>, s: &mut Vec<u8>) {
  let bnode = node.borrow();
  let cm = bnode.node_branches_mask(&[]);
  s.push(b'[');
  let mut n = 0;
  for i in 0..4 {
    let mut lm = cm[i];
    while lm != 0 {
      let index = lm.trailing_zeros();

      let key_byte = 64*(i as u8) + (index as u8);
      match bnode.get_node_at_key(&[key_byte]).into_option() {
        None => {
          if n != 0 { s.push(b',') }
          s.extend(b"{\"name\":\"");
          s.extend(key_byte.to_string().as_bytes());
          s.extend(b"\"}");
          n += 1;
        }
        Some(r) => {
          if n != 0 { s.push(b',') }
          s.extend(b"{\"name\":\"");
          s.extend(key_byte.to_string().as_bytes());
          s.extend(b"\",\"children\":");
          d3_json_intern(&r, s);
          s.push(b'}');
          n += 1;
        }
      }

      lm ^= 1u64 << index;
    }
  }
  s.push(b']');
}

#[wasm_bindgen]
pub fn d3_hierarchy(bts: &BytesTrieSet) -> js_sys::Object {
  let mut s = vec![];
  s.extend(b"{\"name\":\"root\",\"children\":");
  d3_json_intern(bts.btm.root(), &mut s);
  s.push(b'}');
  let r = js_sys::JSON::parse(unsafe { std::str::from_utf8_unchecked(&s[..]) });
  r.unwrap_or_else(|e| e).into()
}

#[wasm_bindgen]
struct Reader {
  z: ReadZipperUntracked<'static, 'static, ()>
}

#[wasm_bindgen]
pub fn reader(bts: &BytesTrieSet) -> Reader {
  Reader { z: unsafe { std::mem::transmute(bts.btm.read_zipper()) } }
}

#[wasm_bindgen]
pub fn descend_to(r: &mut Reader, k: &[u8]) -> bool {
  r.z.descend_to(k)
}

#[wasm_bindgen]
pub fn to_next_val(r: &mut Reader) -> bool {
  r.z.to_next_val().is_some()
}

#[wasm_bindgen]
pub fn descend_indexed_byte(r: &mut Reader, i: usize) -> bool {
  r.z.descend_indexed_branch(i)
}

#[wasm_bindgen]
pub fn to_next_sibling_byte(r: &mut Reader) -> bool {
  r.z.to_next_sibling_byte()
}

#[wasm_bindgen]
pub fn to_prev_sibling_byte(r: &mut Reader) -> bool {
  r.z.to_prev_sibling_byte()
}

#[wasm_bindgen]
pub fn ascend(r: &mut Reader, k: usize) -> bool {
  r.z.ascend(k)
}

#[wasm_bindgen]
pub fn exists(r: &Reader) -> bool {
  r.z.path_exists()
}

#[wasm_bindgen]
pub fn children(r: &Reader) -> Box<[u8]> {
  let cm = r.z.child_mask();
  let mut v = Box::<[u8]>::new_uninit_slice((cm[0].count_ones() + cm[1].count_ones() + cm[2].count_ones() + cm[3].count_ones()) as usize);
  let mut n = 0;
  for i in 0..4 {
    let mut lm = cm[i];
    while lm != 0 {
      let index = lm.trailing_zeros();

      let key_byte = 64*(i as u8) + (index as u8);
      v[n] = std::mem::MaybeUninit::new(key_byte);
      n += 1;

      lm ^= 1u64 << index;
    }
  }
  unsafe { v.assume_init() }
}

#[wasm_bindgen]
pub fn path(r: &Reader) -> Box<[u8]> {
  r.z.path().into()
}

#[wasm_bindgen]
pub fn min_path(r: &Reader) -> Box<[u8]> {
  let mut rz = r.z.fork_read_zipper();
  loop {
    let cc = rz.child_count();
    if cc == 0 { break }
    else { rz.descend_first_byte(); }
  }
  rz.path().into()
}

#[wasm_bindgen]
pub fn max_path(r: &Reader) -> Box<[u8]> {
  let mut rz = r.z.fork_read_zipper();
  loop {
    let cc = rz.child_count(); // descend last byte?
    if cc == 0 { break }
    else { rz.descend_indexed_branch(cc - 1); }
  }
  rz.path().into()
}

#[wasm_bindgen]
pub fn make_map(r: &Reader) -> BytesTrieSet {
  r.z.make_map().map(|m| BytesTrieSet{ btm: m }).unwrap_or(BytesTrieSet{ btm: BytesTrieMap::new() })
}

#[wasm_bindgen]
pub fn val_count(r: &Reader) -> usize {
  r.z.val_count()
}

#[wasm_bindgen]
pub fn fork_reader(r: &Reader) -> Reader {
  Reader { z: unsafe { std::mem::transmute(r.z.fork_read_zipper()) } }
}
