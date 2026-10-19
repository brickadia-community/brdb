# BRDB Documentation

## Format Reference

### BRDB (SQLite Database)
- [File Tree Structure](format/brdb/file-tree.md): SQLite tables, indexes, virtual filesystem hierarchy
- [Revisions](format/brdb/revisions.md): Revision mechanism, timestamps, soft-delete
- [Blobs](format/brdb/blobs.md): Content storage, zstd compression, BLAKE3 hashing

### BRZ (Binary Archive)
- [File Tree Structure](format/brz/file-tree.md): Binary layout, index format, blob data
- [BRDB vs BRZ](format/brz/brdb-vs-brz.md): Side-by-side comparison of the two formats
- [Prefab Bundles](format/brz/prefabs.md): Prefab file tree, embedded `Prefabs/Uploads` archives, `Prefab` path component references

### MessagePack Schema
- [Overview](format/msgpack-schema/overview.md): Schema container, value encodings, variants and `WireValue`, the `types` table and dynamic containers, differences from Zeblote's spec
- [Structure of Arrays](format/msgpack-schema/soa.md): SoA format for bricks, components, wires, entities
- [Flat Arrays](format/msgpack-schema/flat-arrays.md): Binary flat array encoding, C layout vs EA3 packing, limitations
- [Shared Schemas](format/msgpack-schema/shared-schemas.md): Global data, asset references, interning
- [Entity & Component Files](format/msgpack-schema/entity-components.md): .mps file structure, extra data after SoA
- [Schema Selection by Revision](format/msgpack-schema/schema-revisions.md): How a chunk's `created_at` picks the schema that decodes it
- [Older Format Versions](format/msgpack-schema/older-versions.md): EA3 and earlier saves: how to recognise them, their encodings, and renamed names

### Format Properties
- [Determinism](format/determinism.md): Which parts of each format are byte-reproducible, and how to compare independent implementations
- [Migrating Older Versions](format/migrations.md): Upgrading saves in an older version to the Latest format, the `migrations.json` step format, and writing older versions

## External References
- [Zeblote's msgpack-schema draft spec](https://gist.github.com/Zeblote/053d54cc820df3bccad57df676202895)
- [Zeblote's BRZ Gist](https://gist.github.com/Zeblote/0fc682b9df1a3e82942b613ab70d8a04)
