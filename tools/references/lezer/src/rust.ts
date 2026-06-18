import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { IterMode, type Tree, type TreeCursor } from "@lezer/common";

import { buildRustParser, rustSourceInput } from "./rust-support.ts";

interface ReferenceCase {
	id: string;
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

interface PackageManifest {
	devDependencies: Record<string, string>;
}

interface PackageLock {
	packages: Record<string, LockedPackage>;
}

interface LockedPackage {
	version?: string;
	integrity?: string;
}

const CASE_SCHEMA = "rezel.lezer-rust-reference-cases.v1";
const SNAPSHOT_SCHEMA = "rezel.lezer-rust-reference-snapshot.v1";
const REFERENCE_PACKAGES = ["@lezer/common", "@lezer/generator", "@lezer/lr"] as const;

assert.equal(process.versions.unicode, "17.0", "Rust 1.95 identifiers require Unicode 17");

const toolDirectory = join(dirname(fileURLToPath(import.meta.url)), "..");
const repository = join(toolDirectory, "..", "..", "..");
const casesPath = join(toolDirectory, "cases", "rust.json");
const snapshotPath = join(toolDirectory, "snapshots", "rust.json");
const grammarPath = join(repository, "languages", "rust", "grammar", "rust.grammar");
const cases = JSON.parse(readFileSync(casesPath, "utf8")) as CaseManifest;

assert.equal(cases.schema, CASE_SCHEMA);
assertUniqueIds([...cases.strict, ...cases.recovering]);

const grammar = readFileSync(grammarPath, "utf8");
const warnings: string[] = [];
const parser = buildRustParser({
	grammar,
	grammarPath,
	warnings,
});
assert.deepEqual(warnings, [], "the maintained Rust grammar must generate without warnings");

const strictParser = parser.configure({ strict: true });
const recoveringParser = parser.configure({ strict: false });
const snapshot = {
	schema: SNAPSHOT_SCHEMA,
	reference: {
		packages: Object.fromEntries(REFERENCE_PACKAGES.map((name) => [name, packageIdentity(name)])),
		artifacts: {
			grammar: inputIdentity("languages/rust/grammar/rust.grammar", grammar),
		},
	},
	coordinates: "raw-utf8-bytes",
	strict: cases.strict.map((testCase) => {
		const strict = parseCase(strictParser, testCase);
		const recovering = parseCase(recoveringParser, testCase);
		assert.deepEqual(recovering, strict, `${testCase.id} recovered despite being valid`);
		return { id: testCase.id, tree: strict };
	}),
	recovering: cases.recovering.map((testCase) => {
		assert.throws(() => parseCase(strictParser, testCase), `${testCase.id} unexpectedly passed strict parsing`);
		const first = parseCase(recoveringParser, testCase);
		const second = parseCase(recoveringParser, testCase);
		assert.deepEqual(first, second, `${testCase.id} produced a non-deterministic reference tree`);
		return { id: testCase.id, tree: first };
	}),
};
const encoded = `${JSON.stringify(snapshot, null, "\t")}\n`;

if (process.argv.includes("--update")) {
	mkdirSync(dirname(snapshotPath), { recursive: true });
	writeFileSync(snapshotPath, encoded);
	process.stderr.write(`updated ${snapshotPath}\n`);
} else {
	assert.equal(
		readFileSync(snapshotPath, "utf8"),
		encoded,
		"the pinned Lezer Rust reference snapshot drifted; inspect the grammar or pin before updating",
	);
}

function parseCase(activeParser: ReturnType<typeof parser.configure>, testCase: ReferenceCase): ReferenceNode {
	const tree = activeParser.parse(rustSourceInput(testCase.source));
	assert.equal(tree.length, testCase.source.length);
	const projected = project(tree, utf8Boundaries(testCase.source));
	assert.equal(projected.from, 0);
	assert.equal(projected.to, Buffer.byteLength(testCase.source, "utf8"));
	return projected;
}

function project(tree: Tree, rawOffsets: number[]): ReferenceNode {
	const cursor = tree.cursor(IterMode.IncludeAnonymous);
	return projectCursor(cursor, rawOffsets);
}

function projectCursor(cursor: TreeCursor, rawOffsets: number[]): ReferenceNode {
	const children: ReferenceNode[] = [];
	if (cursor.firstChild()) {
		do {
			children.push(projectCursor(cursor, rawOffsets));
		} while (cursor.nextSibling());
		assert.equal(cursor.parent(), true);
	}
	const node: ReferenceNode = {
		name: cursor.name,
		from: rawOffsets[cursor.from],
		to: rawOffsets[cursor.to],
	};
	if (children.length > 0) {
		node.children = children;
	}
	return node;
}

function utf8Boundaries(source: string): number[] {
	const offsets = [0];
	let bytes = 0;
	for (let position = 0; position < source.length; ) {
		const value = source.codePointAt(position);
		if (value === undefined) {
			throw new Error("missing code point inside the source");
		}
		const text = String.fromCodePoint(value);
		for (let offset = 1; offset < text.length; offset += 1) {
			offsets.push(bytes);
		}
		bytes += Buffer.byteLength(text, "utf8");
		offsets.push(bytes);
		position += text.length;
	}
	return offsets;
}

function inputIdentity(path: string, contents: string): { path: string; sha256: string } {
	return {
		path,
		sha256: createHash("sha256").update(contents).digest("hex"),
	};
}

function assertUniqueIds(referenceCases: ReferenceCase[]): void {
	const ids = new Set<string>();
	for (const testCase of referenceCases) {
		assert.notEqual(testCase.id, "");
		assert.equal(ids.has(testCase.id), false, `duplicate reference case ${testCase.id}`);
		ids.add(testCase.id);
	}
}

function packageIdentity(name: (typeof REFERENCE_PACKAGES)[number]): {
	version: string;
	integrity: string;
} {
	const manifest = JSON.parse(readFileSync(join(toolDirectory, "package.json"), "utf8")) as PackageManifest;
	const expectedVersion = manifest.devDependencies[name];
	assert.notEqual(expectedVersion, undefined, `${name} is not pinned`);

	const installed = JSON.parse(readFileSync(join(toolDirectory, "node_modules", name, "package.json"), "utf8")) as {
		name: string;
		version: string;
	};
	assert.equal(installed.name, name);
	assert.equal(installed.version, expectedVersion);

	const lock = JSON.parse(readFileSync(join(toolDirectory, "package-lock.json"), "utf8")) as PackageLock;
	const locked = lock.packages[`node_modules/${name}`];
	assert.notEqual(locked, undefined, `package-lock is missing ${name}`);
	assert.equal(locked.version, expectedVersion);
	const integrity = locked.integrity;
	if (integrity === undefined) {
		throw new Error(`${name} lock entry has no integrity`);
	}
	assert.ok(integrity.startsWith("sha512-"));
	return { version: expectedVersion, integrity };
}
