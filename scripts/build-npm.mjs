// Builds the @ue-shed/uesave-wasm npm package into uesave_wasm/pkg.
//
//   node scripts/build-npm.mjs
//
// Then inspect, pack and publish from that directory. The version comes from the Cargo
// workspace version.
import { spawnSync } from "node:child_process";
import { copyFileSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const crate = join(root, "uesave_wasm");
const out = join(crate, "pkg");

const version = /^\[workspace\.package\][^[]*?^version = "([^"]+)"/ms.exec(
	readFileSync(join(root, "Cargo.toml"), "utf8")
)?.[1];
if (!version) throw new Error("Could not read the workspace version from Cargo.toml");

rmSync(out, { recursive: true, force: true });
const build = spawnSync(
	"wasm-pack",
	["build", crate, "--target", "web", "--release", "--out-dir", out],
	{ stdio: "inherit" }
);
if (build.status !== 0) throw new Error(`wasm-pack failed (${build.status ?? build.error})`);

copyFileSync(join(crate, "npm", "node.js"), join(out, "node.js"));
copyFileSync(join(crate, "npm", "node.d.ts"), join(out, "node.d.ts"));
copyFileSync(join(crate, "README.md"), join(out, "README.md"));
copyFileSync(join(root, "LICENSE"), join(out, "LICENSE"));
rmSync(join(out, ".gitignore"), { force: true });
rmSync(join(out, "package.json"), { force: true });

const pkg = {
	name: "@ue-shed/uesave-wasm",
	version,
	description:
		"Read Unreal Engine save files (GVAS) in the browser and Node: WebAssembly build of the UE Shed fork of uesave",
	license: "MIT",
	author: "Truman Kilen (https://github.com/trumank)",
	contributors: ["UE Shed (https://github.com/ue-shed)"],
	type: "module",
	main: "./node.js",
	types: "./uesave_wasm.d.ts",
	exports: {
		".": {
			node: { types: "./node.d.ts", default: "./node.js" },
			types: "./uesave_wasm.d.ts",
			default: "./uesave_wasm.js"
		},
		"./uesave_wasm_bg.wasm": "./uesave_wasm_bg.wasm",
		"./package.json": "./package.json"
	},
	files: [
		"uesave_wasm_bg.wasm",
		"uesave_wasm_bg.wasm.d.ts",
		"uesave_wasm.js",
		"uesave_wasm.d.ts",
		"node.js",
		"node.d.ts",
		"README.md",
		"LICENSE"
	],
	sideEffects: ["./node.js"],
	keywords: ["unreal-engine", "save", "gvas", "uesave", "wasm"],
	repository: { type: "git", url: "git+https://github.com/ue-shed/uesave.git", directory: "uesave_wasm" },
	homepage: "https://github.com/ue-shed/uesave#readme",
	bugs: { url: "https://github.com/ue-shed/uesave/issues" },
	publishConfig: { access: "public" }
};
writeFileSync(join(out, "package.json"), `${JSON.stringify(pkg, null, "\t")}\n`);
console.log(`Built ${pkg.name}@${version} in ${out}`);
