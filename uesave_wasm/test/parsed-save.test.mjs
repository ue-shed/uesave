// Checks the WebAssembly build's JavaScript-facing reads. Build the package first, then run:
//
//   node scripts/build-npm.mjs
//   node --test uesave_wasm/test
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { ParsedSave } from "../pkg/node.js";

const save = new ParsedSave(
	readFileSync(new URL("../../uesave/drg-save-test.sav", import.meta.url)),
	"",
	""
);

// `value` keeps 32-bit floats exact, where JSON text has their shortest form, so floats are
// compared at 32-bit precision.
function sameAtF32(a, b) {
	if (typeof a === "number" && typeof b === "number") {
		return a === b || Math.fround(a) === Math.fround(b);
	}
	if (Array.isArray(a) && Array.isArray(b)) {
		return a.length === b.length && a.every((x, i) => sameAtF32(x, b[i]));
	}
	if (a && b && typeof a === "object" && typeof b === "object") {
		const keys = Object.keys(a);
		return (
			keys.length === Object.keys(b).length && keys.every((k) => sameAtF32(a[k], b[k]))
		);
	}
	return a === b;
}

test("value matches JSON.parse(json) for every top-level property", () => {
	const keys = JSON.parse(save.keys("[]"));
	assert.ok(keys.length > 0);
	for (const { key } of keys) {
		const path = JSON.stringify([key]);
		assert.ok(sameAtF32(save.value(path), JSON.parse(save.json(path))), key);
	}
});

/** Tagged int properties followed by `None`, as a save stores a struct's properties. */
function propertyList(ints) {
	const out = [];
	const string = (s) => {
		out.push(...u32(s.length + 1), ...Buffer.from(s), 0);
	};
	for (const [name, value] of ints) {
		string(name);
		string("IntProperty");
		out.push(...u32(4), ...u32(0), 0, ...u32(value));
	}
	string("None");
	return out;
}

function u32(n) {
	const b = Buffer.alloc(4);
	b.writeInt32LE(n);
	return [...b];
}

test("readProperties reads a property list from bytes", () => {
	const list = propertyList([
		["Level", 140],
		["XP", 9018],
	]);
	const expected = { Level_0: 140, XP_0: 9018 };
	assert.deepEqual(save.readProperties(new Uint8Array(list), "", ""), expected);
	// With the object GUID flag `UObject::Serialize` writes after the properties.
	assert.deepEqual(save.readProperties(new Uint8Array([...list, 0, 0, 0, 0]), "", ""), expected);
});

test("readProperties throws on bytes that aren't a whole property list", () => {
	const list = propertyList([["Level", 140]]);
	for (const bytes of [[...list, 9], list.slice(0, 20), []]) {
		assert.throws(() => save.readProperties(new Uint8Array(bytes), "", ""));
	}
});
