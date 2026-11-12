# kdl-tree

A fast KDL v2 parser into a borrowed document tree with byte spans on every
node, name, type annotation, entry, property key, value and children block.
No dependencies beyond `thiserror`; the optional `kdl` feature converts to
and from `kdl::KdlDocument`.

It passes all 338 cases of the official kdl-org/kdl test suite and is ~100×
faster than `kdl` on large documents. The parser is derived from
[club-kdl](https://github.com/chronista-club/club-kdl) (MIT OR Apache-2.0);
see the workspace [NOTICE](../../NOTICE).

See the crate docs (`cargo doc -p kdl-tree --open`) and the
[workspace README](../../README.md).
