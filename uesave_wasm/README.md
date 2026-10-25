# @ue-shed/uesave-wasm

Read Unreal Engine save files (GVAS `.sav`) in the browser and Node, compiled to WebAssembly
from the [UE Shed fork](https://github.com/ue-shed/uesave) of
[uesave](https://github.com/trumank/uesave).

uesave is written and maintained by [Truman Kilen](https://github.com/trumank) and
contributors, and is MIT licensed. All credit for the parser goes to them. This package adds
type hints, filtered and compressed reads, a queryable parsed save, and performance and memory
fixes; see the fork's README for the full list.

## Loading

In Node, the module loads itself on import:

```js
import { sav_to_json } from "@ue-shed/uesave-wasm";
```

In the browser, initialize it once with the URL of `uesave_wasm_bg.wasm`, which the package
exports as `@ue-shed/uesave-wasm/uesave_wasm_bg.wasm`:

```js
import init, { sav_to_json } from "@ue-shed/uesave-wasm";
await init({ module_or_path: wasmUrl });
```

`init()` is also exported in Node, where it returns immediately, so the same code runs in both.

The type declarations use `Symbol.dispose` (`ParsedSave` is disposable), which needs TypeScript's
`esnext.disposable` lib unless `skipLibCheck` is on.

## API

Results are JSON strings, or for `ParsedSave.value` JavaScript values, in the shape uesave's JSON output uses: a property is named
`<name>_<index>`, structs are objects, arrays are lists, and maps are lists of `{ key, value }`.

- `sav_to_json(data, hintsJson)` parses a whole save.
- `sav_to_json_only(data, hintsJson, pathsJson)` reads only the properties at the given paths
  (and whatever contains them). Everything else is skipped by its tag size without being parsed,
  so pulling a few values out of a large save takes milliseconds.
- `compressed_sav_to_json(compressed, hintsJson)` and
  `compressed_sav_to_json_only(compressed, hintsJson, pathsJson)` read a save stored as a UE
  compressed archive (zlib chunks, as `FArchiveLoadCompressedProxy` reads) holding a
  length-prefixed byte array. The filtered form only inflates the chunks it reads.
- `sav_property_bytes(data, name)` returns a top-level byte array property, such as a compressed
  payload, without converting the save to JSON.
- `new ParsedSave(data, hintsJson, pathsJson)` and `ParsedSave.fromCompressed(...)` parse once
  and keep the save in WebAssembly memory. Query it with `keys(pathJson)` (a node's children with
  their kind and size), `json(pathJson)` (one node as JSON) or `value(pathJson)` (one node as a
  JavaScript value, built without JSON text in between), or `toJson()` for everything. Paths are
  JSON arrays of segments, such as `["SaveData_0", "players_0", 0, "playerInfo_0"]`. Call
  `free()` when done. Taking a large save one part at a time with `value` keeps memory low:
  neither the whole JSON text nor the whole object tree exists at once. `value` keeps 32-bit
  floats exact (`28812.8671875`), where JSON text has the shortest form (`28812.867`).
- `json_to_sav(json)` writes a save back from full (unfiltered) JSON.

**Hints** (`hintsJson`) map dot-joined property paths to uesave struct type names, for maps and
struct arrays whose element types the save doesn't record, for example
`{ "SaveData.tileSaveDataMap.savedEntryMap.Key": "Struct" }`. Pass `""` for none.

One hint value isn't a type: `"Properties"` on a byte array (`TArray<uint8>`) decodes its bytes as
a property list, for games that store a struct's tagged properties, or a whole object written with
`UObject::Serialize`, in one. It reads as an object of properties instead of `{ "Byte": [...] }`;
bytes that don't parse as a property list stay as they are. Its path is the one the reader is at,
so inside a map's struct values it has no `Value` segment:
`{ "SaveData.dynamicQuestDataMap.Data": "Properties" }`. A save read this way can't be written back
with `json_to_sav`.

**Paths** (`pathsJson`) are a JSON array of dot-joined property paths in the same form, for
example `["SaveData.playerInfo", "SaveData.currentDate"]`. Map entries are transparent: a
property inside a map's values is addressed through the map's name, as in
`SaveData.NPCSaveData.heartPoints`.

## License

MIT. See `LICENSE`, which carries the original uesave copyright.
