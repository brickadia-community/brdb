# brzip

`brzip` is a command-line tool for unpacking Brickadia save files (`.brdb`) and prefab files (`.brz`). It allows you to inspect the internal structure of these files and optionally convert their binary data schemas and contents into readable JSON format.

## Usage

```bash
brzip <input_file> [output_directory] [options]
```

### Arguments

*   `<input_file>`: The path to the `.brdb` or `.brz` file you want to unpack.
*   `[output_directory]`: (Optional) The directory where the unpacked contents should be saved. If not provided, a directory with the same name as the input file (excluding extension) will be created in the input file's directory.

### Options

*   `--json`: Enables automatic conversion of `.schema` and `.mps` (MessagePack serialized) files to JSON. The JSON files will be created alongside their binary counterparts with `.json` appended to the filename (e.g., `GlobalData.mps.json`).

## Examples

**Unpack a save file:**

```bash
cargo run -p brzip -- MyWorld.brdb
```

**Unpack a prefab to a specific directory:**

```bash
cargo run -p brzip -- MyPrefab.brz ./unpacked_prefab
```

**Unpack and convert data to JSON:**

```bash
cargo run -p brzip -- MyWorld.brdb --json
```

## JSON Output

When the `--json` flag is used:

*   `.schema` files are converted to `.schema.json`, showing the enums and structs defined in the schema.
*   `.mps` files are converted to `.mps.json`, showing the actual data values deserialized according to their corresponding schema.
