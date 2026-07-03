# BRDB-RS

This library provides an interface for reading and writing [Brickadia](https://brickadia.com/)'s World files, which are stored in the `.brdb` format.

It also contains code for assisting with parsing msgpack-schema files as defined in [Zeblote's Brickadia msgpac-schema Gist](https://gist.github.com/Zeblote/053d54cc820df3bccad57df676202895). Some undocumented changes to this format are required to fully read/write `.brdb` files.

The `.brz` format is described in [Zeblote's Brickadia brz Gist](https://gist.github.com/Zeblote/0fc682b9df1a3e82942b613ab70d8a04).

## API

See [Examples](./examples/) for complete read/write walkthroughs. The two entry points are
the high-level `World` (build a world in memory, then serialize) and the container readers
(`Brdb` / `Brz`, unified behind `BrFsReader` + `BrReader`).

### Writing

```rust
use brdb::{Brick, World};

let mut world = World::new();
world.meta.bundle.description = "Example World".to_string();
world.bricks.push(Brick {
    position: (0, 0, 6).into(),
    color: (255, 0, 0).into(),
    ..Default::default()
});

world.write_brdb("example.brdb")?; // SQLite container (mutable, revisioned)
world.write_brz("example.brz")?;   // binary archive (read-only snapshot)
```

`World` holds `bricks`, `owners`, `meta`, and (additively) `grids`, `entities`, `wires`, and
component registries. Serialization runs `World::to_unsaved()` (chunk bricks into columnar
Structure-of-Arrays data + build registries) → `UnsavedFs::to_pending()` (a `BrPendingFs`
tree of `.json` / `.schema` / `.mps` files) → the container writer. Bricks carrying
components require `world.register_used_components()` (or `register_all_components()`) first.

### Reading

```rust
use brdb::{Brdb, IntoReader, BrFsReader};

let db = Brdb::new("example.brdb")?.into_reader(); // or Brz::open("example.brz")?.into_reader()

println!("{}", db.get_fs()?.render());              // virtual filesystem tree
let soa = db.brick_chunk_soa(1, (0, 0, 0).into())?; // decoded brick chunk (grid 1, chunk 0,0,0)
let bytes = db.read_file("Meta/World.json")?;       // raw file by path
```

Both containers implement `BrFsReader` (`find_file`, `find_blob`, `get_fs`, path helpers), and
`BrReader<T>` adds cached, schema-aware world reads. `.brdb` selects each chunk's schema by its
revision timestamp (see [Schema Selection by Revision](../../docs/format/msgpack-schema/schema-revisions.md));
`.brz` uses a single embedded schema.

### Format documentation

See [docs/README.md](../../docs/README.md) for the complete on-disk format reference.

## Notes

- Webassembly support requires [rusqlite](https://github.com/rusqlite/rusqlite/pull/1643) to support the `wasm32-unknown-unknown` target. When this is merged, the `brdb` crate should be able to support WebAssembly.
- This library does not contain every in-game asset name (item classes, etc) so a world with those values needs to be parsed to determine their respective values.
- The structs and component data inside worlds may change as Brickadia updates. The game should support migrating old worlds, but newly created worlds may have unexpected fields in them.

## Liability

Use these libraries on your saves at your own risk:

- This library may generate invalid `.brdb` files, which may cause the game (or your computer) to crash or behave unexpectedly. **Report these bugs to the Brickadia team.**
- This library may modify the contents of your `.brdb` files in ways that are not easily recoverable. **Make backups of worlds you plan to modify with this library.**