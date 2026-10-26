# uesave

> **This is the UE Shed fork of [trumank/uesave](https://github.com/trumank/uesave).**
> uesave is written and maintained by [Truman Kilen](https://github.com/trumank) and
> contributors, and is MIT licensed (see [LICENSE](LICENSE)). All credit for the parser
> goes to them. Please report parser bugs and send general improvements upstream.
>
> What this fork changes:
>
> - `uesave_wasm`'s `sav_to_json(data, hints_json)` takes a second argument: a JSON
>   object mapping dot-joined property paths to `StructType` names, the same hints the
>   CLI takes with `-t path=Type`. It lets browser callers parse game-specific `TMap`
>   keys and struct arrays.
> - `sav_to_json` returns compact JSON instead of pretty-printed JSON.
> - Faster parsing: the buffering reader reads ahead in chunks instead of one byte
>   per refill, scope paths and schema records no longer allocate per property, and
>   byte arrays and strings avoid extra copies, and type names are matched without
>   allocating. Release builds use LTO.
> - `SaveReader::only(paths)` reads just the properties at those paths (and whatever
>   contains them), skipping the rest by their tag size without parsing them. For
>   pulling a few values out of a large save. WASM: `sav_to_json_only`.
> - `SaveReader::read_seekable` reads from a stream that can already seek, without the
>   buffering copy `read` makes.
> - `CompressedReader` reads UE's zlib-chunked compressed archives
>   (`FArchiveLoadCompressedProxy`) as a seekable stream, inflating only the chunks
>   that are read. Combined with `only`, a few values come out of a compressed save
>   without decompressing most of it. WASM: `compressed_sav_to_json`,
>   `compressed_sav_to_json_only`, and `sav_property_bytes` to get a byte array
>   property (such as a compressed payload) without converting the save to JSON.
> - WASM `ParsedSave` parses once and keeps the save in WASM memory, to query by path
>   (`keys`, `json`) instead of converting all of it to JSON.
> - Lengths read from a save no longer reserve memory up front. A misparsed property
>   could reserve gigabytes before falling back to raw bytes, which aborts 32-bit WASM.
> - A property that reads more or fewer bytes than its tag declares counts as failed
>   where it is. A wrong type hint used to misalign everything after it, so the raw
>   fallback took a whole enclosing property instead.
> - `SaveReader::nested(paths)` decodes byte arrays that hold a property list (a struct's
>   tagged properties, or an object written with `UObject::Serialize`) in place. WASM:
>   the `"Properties"` hint.
> - `SaveReader::read_properties` reads such a list from bytes with a save's versions
>   and hints, for byte arrays a game wraps in a header of its own: strip the header,
>   then pass the rest. WASM: `ParsedSave.readProperties`.
> - WASM `ParsedSave.value(path)` returns one node as a JavaScript value, with no JSON
>   text in between.
>
> The WASM build is published to npm as
> [`@ue-shed/uesave-wasm`](https://www.npmjs.com/package/@ue-shed/uesave-wasm), versioned
> with the Cargo workspace and tagged `ue-shed-v<version>` (upstream uses `v<version>`).
> `node scripts/build-npm.mjs` builds the package into `uesave_wasm/pkg`, and
> `node --test "uesave_wasm/test/*.test.mjs"` then tests it; pack and publish from there,
> from the exact release commit.
>
> The tag `tools-wasm-2026-04-21` is the exact source of the WASM build vendored in
> Stairway Games' Tools until then: upstream `ab53b2c` plus the hints change. Rebuild it
> with `wasm-pack build uesave_wasm --target web --release`.
>
> The crates.io and docs.rs badges below refer to upstream's published `uesave` crate.
> This fork does not publish to crates.io.

[![docs.rs](https://img.shields.io/docsrs/uesave)](https://docs.rs/uesave)
[![Crates.io](https://img.shields.io/crates/v/uesave)](https://crates.io/crates/uesave)

A library for reading and writing Unreal Engine save files (commonly referred to
as GVAS).

It has been tested on an extensive set of object structures and can fully read
and write Deep Rock Galactic save files (and likely a lot more).

There is a small binary utility to quickly convert saves to and from a plain
text JSON format which can be used for manual save editing.

## Usage

```console
$ cargo install --git https://github.com/trumank/uesave.git
$ uesave --help
Usage: uesave <COMMAND>

Commands:
  to-json    Convert binary save to plain text JSON
  from-json  Convert JSON back to binary save
  edit       Launch $EDITOR to edit a save file as JSON in place
  help       Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help information
  -V, --version  Print version information

```
![edit](https://user-images.githubusercontent.com/1144160/210157064-234da188-ad20-416f-9ea5-7d2956168a20.svg)

## Measuring performance

Run `cargo run -p uesave --release --example profile-read -- path/to/save.sav --json`
to measure parsing and JSON serialization separately. The file is loaded before timing;
parsing includes dropping the result. Each measurement reports the median and range of
seven batches of at least 250 ms. `--compressed` reads a UE compressed archive containing
a length-prefixed save, `--hints hints.json` accepts the WASM hint format (including
`"Properties"`), and `--paths '["SaveData.playerInfo"]'` selects properties.

For phase durations, add `--features tracing` before `--` and `--trace` after it. This
runs once and prints span busy/idle times to stderr, filtered to the `uesave::perf` target.
Spans cover the complete read, header, properties, compressed chunk indexing and cache-miss
inflation, nested property decoding, and raw fallback. Nested durations are inclusive;
they should not be added to their parents. Measure speed without tracing enabled.
The CLI's existing `test-resave --trace` output records byte provenance rather than timing.

The WASM crate also has a `tracing` feature, forwarding parser spans and adding parse,
query, JSON serialization, JS serialization, and key enumeration spans. Embedders must
install a subscriber to collect them; default builds contain no tracing instrumentation.

## Alternative projects
in no particular order:
- https://github.com/rob0rt/drg-save-parser
- https://github.com/ch1pset/UESaveTool
- https://github.com/13xforever/gvas-converter
- https://github.com/RagingLightning/gvas-converter
- https://github.com/SparkyTD/UnrealEngine.Gvas
- https://github.com/localcc/gvas
- https://github.com/oberien/gvas-rs
- https://github.com/scottanderson/railroad.studio
- https://github.com/CrystalFerrai/UeSaveGame
- https://github.com/agc93/unsave
