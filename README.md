# BRDB

This repo provides interfaces for reading and writing [Brickadia](https://brickadia.com/)'s World files, which are stored in the `.brdb` and `.brz` formats.

It also contains code for assisting with parsing msgpack-schema files as defined in [Zeblote's Brickadia msgpac-schema Gist](https://gist.github.com/Zeblote/053d54cc820df3bccad57df676202895). Some undocumented changes to this format are required to fully read/write `.brdb` files.

The `.brz` format is described in [Zeblote's Brickadia brz Gist](https://gist.github.com/Zeblote/0fc682b9df1a3e82942b613ab70d8a04).

## Implementations

- Rust [brdb](./crates/brdb)
- JS/TS: TODO

## Format

See [docs/README.md](docs/README.md) for comprehensive format documentation.

## Liability

Use these libraries on your saves at your own risk:

- This library may generate invalid `.brdb` files, which may cause the game (or your computer) to crash or behave unexpectedly. **Report these bugs to the Brickadia team.**
- This library may modify the contents of your `.brdb` files in ways that are not easily recoverable. **Make backups of worlds you plan to modify with this library.**