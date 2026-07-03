# Determinism

Which parts of the BRDB/BRZ formats are a byte-for-byte function of their input, and which are
not. This matters for anyone building an independent implementation: it defines what can be
compared by exact bytes versus only semantically, and therefore how two implementations can be
verified against each other.

---

## Payloads are deterministic

The *contents* of the virtual filesystem — the individual `.mps`, `.schema`, and `.json` files,
in their decompressed form — are a deterministic function of the world being saved:

- **`.mps`** — MessagePack-Schema data written in fixed schema-field order, with fixed
  smallest-representation rules for integers and floats (see [overview.md](msgpack-schema/overview.md)
  and [soa.md](msgpack-schema/soa.md)).
- **`.schema`** — always the 3-element form, with structs emitted in dependency (topological)
  order and enums/variants/structs in a stable order.
- **`.json`** — compact, with keys in declaration order; `Bundle.json` uses fixed placeholder
  timestamps (`0001.01.01-00.00.00`) rather than wall-clock time.

Two conformant writers, given the same world, produce byte-identical payloads.

---

## `.brz` — deterministic except per-blob compression

The archive layout (magic, header, parallel index arrays, contiguous blob data — see
[brz/file-tree.md](brz/file-tree.md)) is fully determined by the payload tree:

- Folder, file, and blob IDs are assigned by a breadth-first walk in a fixed order.
- Blobs are deduplicated by BLAKE3 hash and assigned on first occurrence.
- The index is stored **uncompressed** (`index_compression = 0`); its BLAKE3 hash covers the
  uncompressed index bytes.

The **only** non-reproducible part is per-blob zstd output: different zstd implementations may
produce different compressed bytes and sizes. A differing compressed size can even flip the
"store compressed only if smaller" decision, changing a blob's `compression_method` and
`size_compressed` in the index, which changes the index hash and the whole file.

Consequences:

- **Compressed `.brz`: not byte-comparable across implementations.** Compare semantically.
- **Uncompressed `.brz`: fully byte-comparable.** With every blob stored raw, the entire file
  is a deterministic function of the payloads plus the fixed framing.

---

## `.brdb` — not byte-deterministic

A `.brdb` should never be compared byte-for-byte:

- **Wall-clock timestamps.** `revisions`, `folders`, and `files` carry `created_at` /
  `deleted_at` values taken from the system clock at write time; two saves of the same world
  differ.
- **SQLite container.** Page layout, rowids, and physical insertion order are engine-specific
  and not reproducible across SQLite builds or implementations.
- **File ordering.** The order of files within a folder as returned on read is not guaranteed
  stable; any traversal for comparison must sort entries by name.

The *logical* contents are still deterministic (payloads are unaffected by the above), so a
`.brdb` is compared at the payload/semantic level, ignoring timestamps, revision structure, and
SQLite framing.

---

## Comparing independent implementations

The determinism properties dictate how to check two implementations for compatibility:

- **Payload comparison (works for both containers).** Enumerate the virtual filesystem, sorting
  entries by name, and compare each file's **decompressed** content — either byte-for-byte or by
  a hash of the decompressed bytes. This is independent of container framing and of zstd, so it
  is the strictest portable check.
- **Semantic comparison.** Decode each file and compare the decoded structures (JSON with sorted
  keys; schema enums/variants/structs; SoA/`.mps` values with explicit numeric types). This
  tolerates equivalent-but-different encodings (float formatting, integer width, map order).
- **Uncompressed `.brz` byte comparison.** Because an uncompressed `.brz` is fully
  deterministic, two implementations can be compared by raw file bytes — the only check that
  also validates the container framing (magic, header, index arrays).
- **`.brdb`: semantic only.** Never byte-compare; always sort entries and ignore timestamps.

Shared, committed golden files (a small matrix of worlds — a single brick, each orientation,
procedural vs basic assets, multiple owners, multiple chunks) let both implementations assert
against the same fixtures. The most authoritative fixtures are those produced by the game
itself, since they capture any convention a reverse-engineered spec might miss.
