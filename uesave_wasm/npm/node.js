// Node entry: loads the WebAssembly module from disk, so the exports are ready to call on
// import. (The browser entry's default init fetches the .wasm by URL, which Node can't do
// for file URLs.)
import { readFileSync } from "node:fs";

import { initSync } from "./uesave_wasm.js";

initSync({ module: readFileSync(new URL("./uesave_wasm_bg.wasm", import.meta.url)) });

export * from "./uesave_wasm.js";
// Already initialized above; calling it returns immediately, so code written for the
// browser entry (`await init()`) runs unchanged in Node.
export { default } from "./uesave_wasm.js";
