import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { IterMode, type TreeCursor } from "@lezer/common";

import { buildPythonParser } from "./python-support.ts";

interface ReferenceCase {
	id: string;
	top: string;
	source: string;
}

interface CaseManifest {
	schema: string;
	strict: ReferenceCase[];
	recovering: ReferenceCase[];
}

interface ReferenceNode {
	name: string;
	from: number;
	to: number;
	children?: ReferenceNode[];
}

const CASE_SCHEMA = "rezel.lezer-python-reference-cases.v1";
const SNAPSHOT_SCHEMA = "rezel.lezer-python-reference-snapshot.v1";
const toolDirectory = join(dirname(fileURLToPath(import.meta.url)), "..");
const repository = join(toolDirectory, "..", "..", "..");
const casesPath = join(toolDirectory, "cases", "python.json");
const snapshotPath = join(toolDirectory, "snapshots", "python.json");
const grammarPath = join(repository, "languages", "python", "src", "python.grammar");
const grammar = readFileSync(grammarPath, "utf8");
const cases = JSON.parse(readFileSync(casesPath, "utf8")) as CaseManifest;

assert.equal(cases.schema, CASE_SCHEMA);
const warnings: string[] = [];
const parser = buildPythonParser({ grammar, grammarPath, warnings });
assert.deepEqual(warnings, []);

const snapshot = {
	schema: SNAPSHOT_SCHEMA,
	reference: {
		grammar: identity("languages/python/src/python.grammar", grammar),
	},
	coordinates: "raw-utf8-bytes",
	strict: cases.strict.map((testCase) => {
		const strict = project(
			parser.configure({ top: testCase.top, strict: true }).parse(testCase.source),
			testCase.source,
		);
		const recovering = project(
			parser.configure({ top: testCase.top, strict: false }).parse(testCase.source),
			testCase.source,
		);
		assert.deepEqual(recovering, strict);
		return { id: testCase.id, top: testCase.top, tree: strict };
	}),
	recovering: cases.recovering.map((testCase) => {
		assert.throws(() => parser.configure({ top: testCase.top, strict: true }).parse(testCase.source));
		const configured = parser.configure({ top: testCase.top, strict: false });
		const first = project(configured.parse(testCase.source), testCase.source);
		const second = project(configured.parse(testCase.source), testCase.source);
		assert.deepEqual(first, second);
		return { id: testCase.id, top: testCase.top, tree: first };
	}),
};
const encoded = `${JSON.stringify(snapshot, null, "\t")}\n`;
if (process.argv.includes("--update")) {
	mkdirSync(dirname(snapshotPath), { recursive: true });
	writeFileSync(snapshotPath, encoded);
} else {
	assert.equal(readFileSync(snapshotPath, "utf8"), encoded);
}

function project(tree: { cursor(mode?: IterMode): TreeCursor; length: number }, source: string): ReferenceNode {
	assert.equal(tree.length, source.length);
	const offsets = utf8Boundaries(source);
	const cursor = tree.cursor(IterMode.IncludeAnonymous);
	return projectCursor(cursor, offsets);
}

function projectCursor(cursor: TreeCursor, offsets: number[]): ReferenceNode {
	const children: ReferenceNode[] = [];
	if (cursor.firstChild()) {
		do children.push(projectCursor(cursor, offsets));
		while (cursor.nextSibling());
		assert.equal(cursor.parent(), true);
	}
	const node: ReferenceNode = { name: cursor.name, from: offsets[cursor.from], to: offsets[cursor.to] };
	if (children.length > 0) node.children = children;
	return node;
}

function utf8Boundaries(source: string): number[] {
	const offsets = [0];
	let bytes = 0;
	for (let position = 0; position < source.length; ) {
		const value = source.codePointAt(position);
		if (value === undefined) throw new Error("missing code point");
		const text = String.fromCodePoint(value);
		for (let offset = 1; offset < text.length; offset += 1) offsets.push(bytes);
		bytes += Buffer.byteLength(text, "utf8");
		offsets.push(bytes);
		position += text.length;
	}
	return offsets;
}

function identity(path: string, source: string): { path: string; sha256: string } {
	return { path, sha256: createHash("sha256").update(source).digest("hex") };
}
