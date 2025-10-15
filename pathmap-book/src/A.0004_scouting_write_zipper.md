# Scouting write zipper

## The problem

Current WriteZipper movement changes the trie more than is necessary.  For example, descending a WZ past a share point will cause the in-memory nodes to become preemptively unique even before any chamges are made through the methods that would be expected to modify the trie.

The reason for this is that the trie keeps a stack of mutable node references, and obtaining one of them requires calling `make_mut` on the `TrieNodeODRc`.

## Solution(s)

My current proposed solution is to essentilly postpone the `make_mut` operation until it's needed.  This would entail traversing the zipper using ordinary `as_tagged` accessors, and maintaining a stack of tagged (read-only) node refs for the top (most descended) portion of the stack.  Then when a trie modification was actually needed, the implementation would need to re-traverse those nodes to obtain unique mutable pointers along the path.

This sounds like twice as much traversal work - and it is, but at least the re-traversal is likely to hit cache.  More importantly, if it saves even a little bit of unnecessary node duplication (and thus breaking sharing) then I feel that it will be a net win.

I'm open to alternative ideas, however.