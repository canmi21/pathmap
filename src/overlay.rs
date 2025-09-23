use crate::utils::{BitMask, ByteMask};
use crate::zipper::{Zipper, ZipperMoving, ZipperIteration, ZipperValues};

pub struct OverlayZipper<VBase, VOverlay, Base, Overlay, Mapping>
    where
        Base: ZipperValues<VBase> + ZipperMoving,
        Overlay: ZipperValues<VOverlay> + ZipperMoving,
        Mapping: Fn(&VBase) -> &VOverlay,
{
    base: Base,
    overlay: Overlay,
    mapping: Mapping,
    _marker: core::marker::PhantomData<(VBase, VOverlay)>,
}

fn identity_ref<V>(x: &V) -> &V { x }

impl<Value, Base, Overlay>
    OverlayZipper<Value, Value, Base, Overlay, fn(&Value) -> &Value>
    where
        Base: ZipperValues<Value> + ZipperMoving,
        Overlay: ZipperValues<Value> + ZipperMoving,
{
    pub fn new(base: Base, overlay: Overlay) -> Self {
        Self {
            base, overlay,
            mapping: identity_ref,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<VBase, VOverlay, Base, Overlay, Mapping>
    OverlayZipper<VBase, VOverlay, Base, Overlay, Mapping>
    where
        Base: ZipperValues<VBase> + ZipperMoving,
        Overlay: ZipperValues<VOverlay> + ZipperMoving,
        Mapping: Fn(&VBase) -> &VOverlay,
{
    pub fn with_mapping(base: Base, overlay: Overlay, mapping: Mapping) -> Self {
        Self {
            base, overlay,
            mapping,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<VBase, VOverlay, Base, Overlay, Mapping>
    OverlayZipper<VBase, VOverlay, Base, Overlay, Mapping>
    where
        Base: ZipperValues<VBase> + ZipperMoving,
        Overlay: ZipperValues<VOverlay> + ZipperMoving,
        Mapping: Fn(&VBase) -> &VOverlay,
{
    fn to_sibling(&mut self, next: bool) -> bool {
        let path = self.path();
        let Some(&last) = path.last() else {
            return false;
        };
        self.ascend(1);
        let child_mask = self.child_mask();
        let maybe_child = if next {
            child_mask.next_bit(last)
        } else {
            child_mask.prev_bit(last)
        };
        let Some(child) = maybe_child else {
            self.descend_to_byte(last);
            return false;
        };
        self.descend_to_byte(child)
    }
}

impl<VBase, VOverlay, Base, Overlay, Mapping> ZipperValues<VOverlay>
    for OverlayZipper<VBase, VOverlay, Base, Overlay, Mapping>
    where
        Base: ZipperValues<VBase> + ZipperMoving,
        Overlay: ZipperValues<VOverlay> + ZipperMoving,
        Mapping: Fn(&VBase) -> &VOverlay,
{
    fn val(&self) -> Option<&VOverlay> {
        if let Some(val) = self.overlay.val() {
            Some(val)
        } else if let Some(val) = self.base.val() {
            Some((self.mapping)(val))
        } else {
            None
        }
    }
}

impl<VBase, VOverlay, Base, Overlay, Mapping> Zipper
    for OverlayZipper<VBase, VOverlay, Base, Overlay, Mapping>
    where
        Base: ZipperValues<VBase> + ZipperMoving,
        Overlay: ZipperValues<VOverlay> + ZipperMoving,
        Mapping: Fn(&VBase) -> &VOverlay,
{
    fn path_exists(&self) -> bool {
        self.overlay.path_exists() || self.base.path_exists()
    }
    fn is_val(&self) -> bool {
        self.overlay.is_val() || self.base.is_val()
    }
    fn child_count(&self) -> usize {
        self.child_mask().count_bits()
    }
    fn child_mask(&self) -> ByteMask {
        self.overlay.child_mask() | self.base.child_mask()
    }
}

impl<VBase, VOverlay, Base, Overlay, Mapping> ZipperMoving
    for OverlayZipper<VBase, VOverlay, Base, Overlay, Mapping>
    where
        Base: ZipperValues<VBase> + ZipperMoving,
        Overlay: ZipperValues<VOverlay> + ZipperMoving,
        Mapping: Fn(&VBase) -> &VOverlay,
{
    fn at_root(&self) -> bool {
        self.overlay.at_root() || self.base.at_root()
    }

    fn reset(&mut self) {
        self.overlay.reset();
        self.base.reset();
    }

    fn path(&self) -> &[u8] {
        self.overlay.path()
    }

    fn val_count(&self) -> usize {
        todo!()
    }

    fn descend_to<P: AsRef<[u8]>>(&mut self, path: P) -> bool {
        let path = path.as_ref();
        self.overlay.descend_to(path) | self.base.descend_to(path)
    }

    fn descend_to_existing<P: AsRef<[u8]>>(&mut self, path: P) -> usize {
        let path = path.as_ref();
        let depth_b = self.base.descend_to_existing(path);
        let depth_o = self.overlay.descend_to_existing(path);
        if depth_b > depth_o {
            self.overlay.descend_to(&path[depth_o..depth_b]);
            depth_b
        } else if depth_o > depth_b {
            self.base.descend_to(&path[depth_b..depth_o]);
            depth_o
        } else {
            depth_b
        }
    }

    #[allow(deprecated)]
    fn descend_to_value<K: AsRef<[u8]>>(&mut self, path: K) -> usize {
        let path = path.as_ref();
        let depth_b = self.base.descend_to_value(path);
        let depth_o = self.overlay.descend_to_value(path);
        if depth_b < depth_o {
            if self.base.is_val() {
                self.overlay.ascend(depth_o - depth_b);
                depth_b
            } else {
                self.base.descend_to(&path[depth_b..depth_o]);
                depth_o
            }
        } else if depth_o < depth_b {
            if self.overlay.is_val() {
                self.base.ascend(depth_b - depth_o);
                depth_o
            } else {
                self.base.descend_to(&path[depth_o..depth_b]);
                depth_b
            }
        } else {
            depth_b
        }
    }

    fn descend_to_byte(&mut self, k: u8) -> bool {
        self.base.descend_to(&[k]) | self.overlay.descend_to(&[k])
    }

    fn descend_first_byte(&mut self) -> bool {
        self.descend_indexed_byte(0)
    }

    fn descend_indexed_byte(&mut self, idx: usize) -> bool {
        let child_mask = self.child_mask();
        let Some(byte) = child_mask.indexed_bit::<true>(idx) else {
            return false;
        };
        self.descend_to_byte(byte)
    }

    /*
    TODO: this implementation seems to be broken, we can use default impl.
    fn descend_until(&mut self) -> bool {
        use crate::utils::find_prefix_overlap;
        let depth = self.overlay.path().len();
        let desc_b = self.base.descend_until();
        let path_b = &self.base.path()[depth..];
        let desc_o = self.overlay.descend_until();
        let path_o = &self.overlay.path()[depth..];
        if !desc_b && !desc_o {
            return false;
        }
        if !desc_b && desc_o {
            self.base.descend_to(path_o);
            return true;
        }
        if desc_b && !desc_o {
            self.overlay.descend_to(path_b);
            return true;
        }
        let overlap = find_prefix_overlap(path_b, path_o);
        if path_b.len() > overlap {
            self.base.ascend(path_b.len() - overlap);
        }
        if path_o.len() > overlap {
            self.overlay.ascend(path_o.len() - overlap);
        }
        overlap > 0
    }
    */

    fn ascend(&mut self, steps: usize) -> bool {
        self.base.ascend(steps) | self.overlay.ascend(steps)
    }

    fn ascend_byte(&mut self) -> bool {
        self.ascend(1)
    }

    fn ascend_until(&mut self) -> bool {
        assert_eq!(self.base.path().len(), self.overlay.path().len());
        // eprintln!("asc_until i {:?} {:?}", self.base.path(), self.overlay.path());
        let asc_b = self.base.ascend_until();
        let path_b = self.base.path();
        let depth_b = path_b.len();
        let asc_o = self.overlay.ascend_until();
        let path_o = self.overlay.path();
        let depth_o = path_o.len();
        if !(asc_o || asc_b) {
            return false;
        }
        // eprintln!("asc_until o {path_b:?} {path_o:?}");
        if depth_o > depth_b {
            self.base.descend_to(&path_o[depth_b..]);
        } else if depth_b > depth_o {
            self.overlay.descend_to(&path_b[depth_o..]);
        }
        // asc_b || asc_o
        true
    }

    fn ascend_until_branch(&mut self) -> bool {
        let asc_b = self.base.ascend_until_branch();
        let path_b = self.base.path();
        let depth_b = path_b.len();
        let asc_o = self.overlay.ascend_until_branch();
        let path_o = self.overlay.path();
        let depth_o = path_o.len();
        if depth_o > depth_b {
            self.base.descend_to(&path_o[depth_b..]);
        } else if depth_b > depth_o {
            self.overlay.descend_to(&path_b[depth_o..]);
        }
        asc_b || asc_o
    }

    fn to_next_sibling_byte(&mut self) -> bool {
        self.to_sibling(true)
    }

    fn to_prev_sibling_byte(&mut self) -> bool {
        self.to_sibling(false)
    }
}

impl<VBase, VOverlay, Base, Overlay, Mapping> ZipperIteration
    for OverlayZipper<VBase, VOverlay, Base, Overlay, Mapping>
    where
        Base: ZipperValues<VBase> + ZipperMoving,
        Overlay: ZipperValues<VOverlay> + ZipperMoving,
        Mapping: Fn(&VBase) -> &VOverlay,
{ }

#[cfg(test)]
mod tests {
    use crate::alloc::GlobalAlloc;
use super::{OverlayZipper};
    use crate::{
        PathMap,
        zipper::{
            ReadZipperUntracked,
            zipper_iteration_tests,
            zipper_moving_tests,
            // ZipperIteration,
            // ZipperMoving,
            // ZipperValues
        },
    };

    // #[test]
    // fn overlay_preserves_keys() {
    //     // base: ACT { "aaa" -> 1, "bbb" -> 3 }
    //     // overlay: PathMap { "aaa" -> 2, "ccc" -> 4 }
    //     // result: Overlay { "aaa" -> 2, "bbb" -> 3, "ccc" -> 4 }
    //     let keys: &[&[u8]] = &[b"a", b"aa", b"ab", b"b", b"ba", b"bb"];
    //     let trie_a = keys[..3].into_iter().map(|k| (k, ())).collect::<PathMap<()>>();
    //     let trie_b = keys[3..].into_iter().map(|k| (k, ())).collect::<PathMap<()>>();
    //     let mut oz = OverlayZipper::new(trie_a.read_zipper(), trie_b.read_zipper());
    //     assert_eq!(oz.keys(), keys);
    // }

    type Mapping = fn(&()) -> &();
    type OZ<'a, V, A=GlobalAlloc> = OverlayZipper<
        V, V,
        ReadZipperUntracked<'a, 'static, V, A>,
        ReadZipperUntracked<'a, 'static, V, A>,
        Mapping
    >;
    zipper_moving_tests::zipper_moving_tests!(overlay_zipper,
        |keys: &[&[u8]]| {
            let cutoff = keys.len() / 3 * 2;
            // eprintln!("keys={:?}", &keys);
            eprintln!("keys={:?}, {:?}", &keys[..cutoff], &keys[cutoff..]);
            let a = keys[..cutoff].into_iter().map(|k| (k, ())).collect::<PathMap<()>>();
            let b = keys[cutoff..].into_iter().map(|k| (k, ())).collect::<PathMap<()>>();
            (a, b)
        },
        |trie: &mut (PathMap<()>, PathMap<()>), path: &[u8]| -> OZ<'_, ()> {
            OverlayZipper::new(
                trie.0.read_zipper_at_path(path),
                trie.1.read_zipper_at_path(path),
            )
        }
    );

    zipper_iteration_tests::zipper_iteration_tests!(arena_compact_zipper,
        |keys: &[&[u8]]| {
            let cutoff = keys.len() / 3 * 2;
            eprintln!("keys={:?}, {:?}", &keys[..cutoff], &keys[cutoff..]);
            let a = keys[..cutoff].into_iter().map(|k| (k, ())).collect::<PathMap<()>>();
            let b = keys[cutoff..].into_iter().map(|k| (k, ())).collect::<PathMap<()>>();
            (a, b)
        },
        |trie: &mut (PathMap<()>, PathMap<()>), path: &[u8]| -> OZ<'_, ()> {
            OverlayZipper::new(
                trie.0.read_zipper_at_path(path),
                trie.1.read_zipper_at_path(path),
            )
        }
    );
}
