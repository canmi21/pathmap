use std::io::Write;
use wasm_bindgen::prelude::*;
use js_sys;
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

#[wasm_bindgen]
pub fn object(bts: &BytesTrieSet) -> js_sys::Object {
  let mut s = vec![];
  json_intern(bts.btm.root(), &mut s);
  let r = js_sys::JSON::parse(unsafe { std::str::from_utf8_unchecked(&s[..]) });
  r.unwrap_or_else(|e| e).into()
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
pub fn make_map(r: &Reader) -> BytesTrieSet {
  r.z.make_map().map(|m| BytesTrieSet{ btm: m }).unwrap_or(BytesTrieSet{ btm: BytesTrieMap::new() })
}
