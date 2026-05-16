import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import type { TreeCursor } from "@lezer/common";
import { parser as jsonParser } from "@lezer/json";

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

const CASE_SCHEMA = "rezel.lezer-json-reference-cases.v1";
const SNAPSHOT_SCHEMA = "rezel.lezer-json-reference-snapshot.v1";
const REFERENCE_PACKAGE = "@lezer/json";

const toolDirectory = join(dirname(fileURLToPath(import.meta.url)), "..");
const casesPath = join(toolDirectory, "cases", "json.json");
const snapshotPath = join(toolDirectory, "snapshots", "json.json");
const cases = JSON.parse(readFileSync(casesPath, "utf8")) as CaseManifest;

assert.equal(cases.schema, CASE_SCHEMA);
assertUniqueIds([...cases.strict, ...cases.recovering]);

const strictParser = jsonParser.configure({ strict: true });
const recoveringParser = jsonParser.configure({ strict: false });
const snapshot = {
	schema: SNAPSHOT_SCHEMA,
	reference: referenceIdentity(),
	strict: cases.strict.map((testCase) => ({
		id: testCase.id,
		tree: project(strictParser.parse(testCase.source), testCase.source),
	})),
	recovering: cases.recovering.map((testCase) => {
		const first = project(recoveringParser.parse(testCase.source), testCase.source);
		const second = project(recoveringParser.parse(testCase.source), testCase.source);
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
		"the pinned Lezer JSON reference snapshot drifted; inspect the pin before updating",
	);
}

function project(tree: ReturnType<typeof jsonParser.parse>, source: string): ReferenceNode {
	assert.equal(tree.length, source.length);
	return projectCursor(tree.cursor(), source);
}

function projectCursor(cursor: TreeCursor, source: string): ReferenceNode {
	const children: ReferenceNode[] = [];
	if (cursor.firstChild()) {
		do {
			children.push(projectCursor(cursor, source));
		} while (cursor.nextSibling());
		assert.equal(cursor.parent(), true);
	}

	const node: ReferenceNode = {
		name: cursor.name,
		from: utf8Offset(source, cursor.from),
		to: utf8Offset(source, cursor.to),
	};
	if (children.length > 0) {
		node.children = children;
	}
	return node;
}

function utf8Offset(source: string, utf16Offset: number): number {
	return Buffer.byteLength(source.slice(0, utf16Offset), "utf8");
}

function assertUniqueIds(cases: ReferenceCase[]): void {
	const ids = new Set<string>();
	for (const testCase of cases) {
		assert.notEqual(testCase.id, "");
		assert.equal(ids.has(testCase.id), false, `duplicate reference case ${testCase.id}`);
		ids.add(testCase.id);
	}
}

function referenceIdentity(): { name: string; version: string; integrity: string } {
	const manifest = JSON.parse(readFileSync(join(toolDirectory, "package.json"), "utf8")) as PackageManifest;
	const expectedVersion = manifest.devDependencies[REFERENCE_PACKAGE];
	assert.notEqual(expectedVersion, undefined, `${REFERENCE_PACKAGE} is not pinned`);

	const installed = JSON.parse(
		readFileSync(join(toolDirectory, "node_modules", REFERENCE_PACKAGE, "package.json"), "utf8"),
	) as { name: string; version: string };
	assert.equal(installed.name, REFERENCE_PACKAGE);
	assert.equal(installed.version, expectedVersion);

	const lock = JSON.parse(readFileSync(join(toolDirectory, "package-lock.json"), "utf8")) as PackageLock;
	const locked = lock.packages[`node_modules/${REFERENCE_PACKAGE}`];
	assert.notEqual(locked, undefined, `package-lock is missing ${REFERENCE_PACKAGE}`);
	assert.equal(locked.version, expectedVersion);
	const integrity = locked.integrity;
	if (integrity === undefined) {
		throw new Error(`${REFERENCE_PACKAGE} lock entry has no integrity`);
	}
	assert.ok(integrity.startsWith("sha512-"));

	return {
		name: REFERENCE_PACKAGE,
		version: expectedVersion,
		integrity,
	};
}
