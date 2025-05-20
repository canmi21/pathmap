use crate::utils::ByteMask;
use std::sync::atomic::AtomicU32;
use std::mem::MaybeUninit;
use std::ptr::{NonNull, addr_of, addr_of_mut};

macro_rules! const_assert_eq {
    ($a:expr, $b:expr) => {
        const _: [(); 0] = [(); const { if $a == $b { 0 } else { 1 } }];
    }
}

#[derive(Clone, Copy)]
struct PayloadRef(u8);

#[derive(Clone, Copy)]
struct ByteUnsaturated {
    // 3-bits of tag
    // 3 bit integer to indicate the used length of prefix_path
    // has_root_val flag
    // saturated_ff_val_or_child flag.
    common_header: u8,
    val_or_child_bitfield: [u8; 3],
    prefix_path: [u8; 4],
    payloads: [PayloadRef; 24],
    child_mask: ByteMask,
}
const_assert_eq!(64, std::mem::size_of::<ByteUnsaturated>());
const_assert_eq!(8, std::mem::align_of::<ByteUnsaturated>());

#[derive(Clone, Copy)]
struct ByteSaturatedHead {
    // 3-bits of tag
    // 3 bit integer to indicate the used length of prefix_path
    // has_root_val flag
    // saturated_ff_val_or_child flag.
    common_header: u8,
    val_or_child_bitfield: [u8; 3],
    prefix_path: [u8; 4],
    payloads: [PayloadRef; 24],
    child_mask: ByteMask,
}
const_assert_eq!(64, std::mem::size_of::<ByteSaturatedHead>());
const_assert_eq!(8, std::mem::align_of::<ByteSaturatedHead>());

#[derive(Clone, Copy)]
struct ByteSaturatedBody {
    payloads: [NodePtr; 15],
    // Stores a bit indicating whether each payload is a value or a child.
    metadata: u16,
    _usused: [u8; 6],
}
const_assert_eq!(128, std::mem::size_of::<ByteSaturatedBody>());
const_assert_eq!(8, std::mem::align_of::<ByteSaturatedBody>());

#[derive(Clone, Copy)]
struct StraightPath {
     // 3-bits of tag,
     // Bit 3 == end_is_used,
     // Bit 4 == end_val_or_child,
     // Bit 5 == has_root_val (only needed once the changes described in [A.0001_map_root_values.md] are implemented.),
     // Bits 6 & 7 = unused.
    common_header: u8,
    payload: PayloadRef,
    data: [u8; 30],
}

#[derive(Clone, Copy)]
struct LoudsChunk {
    // 3-bits of tag
    // 1 flag = has_root_val
    // 4-bits of unused
    common_header: u8, 
    endpoints_table: [PayloadRef; 8],
    louds_bit_string: u32,
    path_bytes: [u8; 19],
}

#[repr(align(32))]
struct NodeUntagged {
    _unused: [u8; 32],
}
const_assert_eq!(32, std::mem::size_of::<NodeUntagged>());
const_assert_eq!(32, std::mem::align_of::<NodeUntagged>());

enum NodeRef<'a> {
    ByteUnsaturated(&'a ByteUnsaturated),
    ByteSaturatedHead(&'a ByteSaturatedHead),
    ByteSaturatedBody(&'a ByteSaturatedBody),
    StraightPath(&'a StraightPath),
    LoudsChunk(&'a LoudsChunk),
}

#[derive(Clone, Copy)]
pub struct NodePtr(NonNull<NodeUntagged>);

pub struct NodePtrArc {
    tagged_ptr: NodePtr,
}
const_assert_eq!(8, std::mem::size_of::<NodePtrArc>());
const_assert_eq!(8, std::mem::size_of::<Option<NodePtrArc>>());

impl NodePtrArc {
    const UNSATURATED: usize = 1;
    const SATURATED_HEAD: usize = 2;
    const SATURATED_BODY: usize = 3;
    const STRAIGHT_PATH: usize = 4;
    const LOUDS_CHUNK: usize = 5;
    fn write(mut self, node: NodeRef) -> Self {
        let prev_tag = self.tagged_ptr.0.addr().get() & 0xF;
        unsafe {
            let ptr = self.tagged_ptr.0.sub(prev_tag);
            let new_tag = match node {
                NodeRef::ByteUnsaturated(v) => {
                    ptr.as_ptr().cast::<ByteUnsaturated>().copy_from(v, 1);
                    Self::UNSATURATED
                },
                NodeRef::ByteSaturatedHead(v) => {
                    ptr.as_ptr().cast::<ByteSaturatedHead>().copy_from(v, 1);
                    Self::SATURATED_HEAD
                },
                NodeRef::ByteSaturatedBody(v) => {
                    ptr.as_ptr().cast::<ByteSaturatedBody>().copy_from(v, 1);
                    Self::SATURATED_BODY
                },
                NodeRef::StraightPath(v) => {
                    ptr.as_ptr().cast::<StraightPath>().copy_from(v, 1);
                    Self::STRAIGHT_PATH
                },
                NodeRef::LoudsChunk(v) => {
                    ptr.as_ptr().cast::<LoudsChunk>().copy_from(v, 1);
                    Self::LOUDS_CHUNK
                },
            };
            self.tagged_ptr = NodePtr(ptr.add(new_tag));
            self
        }
    }
    fn get(&self) -> NodeRef {
        let tag = self.tagged_ptr.0.addr().get() & 0xF;
        unsafe {
            let ptr = self.tagged_ptr.0.sub(tag);
            match tag {
                Self::UNSATURATED => NodeRef::ByteUnsaturated(&*ptr.as_ptr().cast()),
                Self::SATURATED_HEAD => NodeRef::ByteSaturatedHead(&*ptr.as_ptr().cast()),
                Self::SATURATED_BODY => NodeRef::ByteSaturatedBody(&*ptr.as_ptr().cast()),
                Self::STRAIGHT_PATH => NodeRef::StraightPath(&*ptr.as_ptr().cast()),
                Self::LOUDS_CHUNK => NodeRef::LoudsChunk(&*ptr.as_ptr().cast()),
                _ => panic!()
            }
        }
    }
}

const CHUNKS_PER_BLOCK: usize = 84;

#[repr(align(4096))]
pub struct NodeBlock {
    prev: NonNull<NodeBlock>,
    next: NonNull<NodeBlock>,
    occupancy: u128,
    ref_counts_table: [AtomicU32; 92],
    payloads_table: [Option<NodePtrArc>; 126],
    chunks_table: [MaybeUninit<NodeUntagged>; CHUNKS_PER_BLOCK],
}
const_assert_eq!(4096, std::mem::size_of::<NodeBlock>());
const_assert_eq!(4096, std::mem::align_of::<NodeBlock>());

impl NodeBlock {
    unsafe fn init(node: NonNull<NodeBlock>) {
        *addr_of_mut!((*node.as_ptr()).prev) = node;
        *addr_of_mut!((*node.as_ptr()).next) = node;
    }

    unsafe fn insert_after(prev: NonNull<NodeBlock>, node: NonNull<NodeBlock>) {
        *addr_of_mut!((*node.as_ptr()).prev) = prev;
        *addr_of_mut!((*node.as_ptr()).next) = *addr_of!((*prev.as_ptr()).next);
        *addr_of_mut!((*prev.as_ptr()).next) = node;
    }

    unsafe fn remove(node: NonNull<NodeBlock>) -> bool {
        let prev = node.as_ref().prev;
        if prev == node {
            // we're the last node on the freelist
            return false;
        }
        let next = node.as_ref().next;
        *addr_of_mut!((*prev.as_ptr()).next) = next;
        *addr_of_mut!((*next.as_ptr()).prev) = prev;
        true
    }

    unsafe fn move_after(prev: NonNull<NodeBlock>, node: NonNull<NodeBlock>) {
        if !Self::remove(node) {
            return;
        }
        Self::insert_after(prev, node);
    }

    unsafe fn nth_node(block: NonNull<NodeBlock>, index: usize) -> NonNull<MaybeUninit<NodeUntagged>> {
        assert!(index < 84, "chunks_table index must be < 84");
        let table = addr_of_mut!((*block.as_ptr()).chunks_table);
        let node = table.cast::<MaybeUninit<NodeUntagged>>().add(index);
        NonNull::new(node).
            expect("This pointer was constructed from non-null pointer and can't be null. QED")
    }
}

const BLOCKS_IN_VEC: usize = 4096 / std::mem::size_of::<NodePtr>();
pub struct Trie {
    blocks: Vec<Vec<NonNull<NodeBlock>>>,
    last_free: NonNull<NodeBlock>,
    root: Option<NodePtr>,
}

impl Trie {
    pub fn new() -> Self {
        let first_block = Self::alloc_raw();
        unsafe { NodeBlock::init(first_block) };
        let mut first_vec = Vec::with_capacity(BLOCKS_IN_VEC);
        first_vec.push(first_block);
        Self {
            blocks: vec![first_vec],
            last_free: first_block,
            root: None,
        }
    }

    fn alloc_raw() -> NonNull<NodeBlock> {
        let layout = std::alloc::Layout::new::<NodeBlock>();
        let ptr = unsafe {
            std::alloc::alloc_zeroed(layout).cast::<NodeBlock>()
        };
        NonNull::new(ptr)
            .expect("We're out of memory. Big sad.")
    }

    fn alloc_block(&mut self) -> NonNull<NodeBlock> {
        let last_vec = match self.blocks.last_mut() {
            Some(vec) if vec.len() < BLOCKS_IN_VEC => {
                vec
            },
            _ => {
                self.blocks.push(Vec::with_capacity(BLOCKS_IN_VEC));
                self.blocks.last_mut()
                    .expect("We just pushed a value, the vec cannot be empty. QED")
            },
        };
        let ptr = Self::alloc_raw();
        const INIT_OCCUPANCY: u128 = (!0) << CHUNKS_PER_BLOCK as u32;
        unsafe {
            *addr_of_mut!((*ptr.as_ptr()).occupancy) = INIT_OCCUPANCY;
            NodeBlock::insert_after(self.last_free, ptr);
        }
        self.last_free = ptr;
        last_vec.push(ptr);
        ptr
    }

    // ByteUnsaturated / ByteSaturatedHead takes 2 chunks
    #[inline(always)]
    fn extend_occupancy_2(occupancy: u128) -> u128 {
        occupancy | occupancy >> 1
    }

    // ByteSaturatedBody takes 4 chunks
    #[inline(always)]
    fn extend_occupancy_4(mut occupancy: u128) -> u128 {
        occupancy = occupancy | occupancy >> 1;
        occupancy | occupancy >> 2
    }

    fn alloc_node(&mut self) -> NonNull<MaybeUninit<NodeUntagged>> {
        unsafe {
            let mut block = self.last_free;
            let mut occupancy = *addr_of!((*block.as_ptr()).occupancy);
            if occupancy.trailing_ones() >= 84 {
                block = self.alloc_block();
            }
            occupancy = *addr_of!((*block.as_ptr()).occupancy);
            let index = occupancy.trailing_ones();
            let ptr = NodeBlock::nth_node(block, index as usize);
            occupancy |= 1 << index;
            *addr_of_mut!((*block.as_mut()).occupancy) = occupancy;
            if occupancy.trailing_ones() >= 84 {
                let prev = *addr_of!((*self.last_free.as_ptr()).prev);
                self.last_free = prev;
            }
            ptr
        }
    }

    pub fn insert_node(&mut self) -> NodePtrArc {
        NodePtrArc {
            tagged_ptr: NodePtr(self.alloc_node().cast::<NodeUntagged>()),
        }
    }
}
