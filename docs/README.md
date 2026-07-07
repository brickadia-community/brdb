# BRDB Documentation

## Format Reference

### BRDB (SQLite Database)
- [File Tree Structure](format/brdb/file-tree.md): SQLite tables, indexes, virtual filesystem hierarchy
- [Revisions](format/brdb/revisions.md): Revision mechanism, timestamps, soft-delete
- [Blobs](format/brdb/blobs.md): Content storage, zstd compression, BLAKE3 hashing

### BRZ (Binary Archive)
- [File Tree Structure](format/brz/file-tree.md): Binary layout, index format, blob data
- [BRDB vs BRZ](format/brz/brdb-vs-brz.md): Side-by-side comparison of the two formats
- [Prefab Bundles](format/brz/prefabs.md): Prefab file tree, embedded `Prefabs/Uploads` archives, `bundle_path_ref` component references

### MessagePack Schema
- [Overview](format/msgpack-schema/overview.md): Schema format, types, deviations from Zeblote's gist
- [Structure of Arrays](format/msgpack-schema/soa.md): SoA format for bricks, components, entities
- [Flat Arrays](format/msgpack-schema/flat-arrays.md): Binary flat array encoding and limitations
- [Shared Schemas](format/msgpack-schema/shared-schemas.md): Global data, asset references, interning
- [Entity & Component Files](format/msgpack-schema/entity-components.md): .mps file structure, extra data after SoA
- [Schema Selection by Revision](format/msgpack-schema/schema-revisions.md): How a chunk's `created_at` picks the schema that decodes it

### Format Properties
- [Determinism](format/determinism.md): Which parts of each format are byte-reproducible, and how to compare independent implementations

## External References
- [Zeblote's msgpack-schema Gist](https://gist.github.com/Zeblote/053d54cc820df3bccad57df676202895)
- [Zeblote's BRZ Gist](https://gist.github.com/Zeblote/0fc682b9df1a3e82942b613ab70d8a04)
