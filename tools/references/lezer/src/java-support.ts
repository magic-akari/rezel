import assert from "node:assert/strict";

import type { NodePropSource } from "@lezer/common";
import { buildParser } from "@lezer/generator";
import { ExternalTokenizer } from "@lezer/lr";

interface BuildJavaParserOptions {
	grammar: string;
	grammarPath: string;
	identifierTables: string;
	warnings: string[];
}

interface TranslatedInput {
	text: string;
	rawOffsets: number[];
}

interface CodePoint {
	value: number;
	width: number;
}

type ScalarRange = readonly [number, number];

export function buildJavaParser(options: BuildJavaParserOptions): ReturnType<typeof buildParser> {
	const identifierStart = parseRanges(options.identifierTables, "JAVA_IDENTIFIER_START");
	const identifierPart = parseRanges(options.identifierTables, "JAVA_IDENTIFIER_PART");
	const emptyProperties: NodePropSource = () => null;

	return buildParser(options.grammar, {
		fileName: options.grammarPath,
		includeNames: true,
		warn(message) {
			options.warnings.push(message);
		},
		externalTokenizer(name, terms) {
			assert.equal(name, "TOKENIZER");
			assert.equal(typeof terms.identifier, "number");
			return javaIdentifiers(terms.identifier, identifierStart, identifierPart);
		},
		externalSpecializer(name, terms) {
			assert.equal(name, "specialize_record");
			assert.equal(typeof terms.record, "number");
			return (value, stack) => (value === "record" && stack.canShift(terms.record) ? terms.record : -1);
		},
		externalPropSource(name) {
			assert.equal(name, "java_highlighting");
			return emptyProperties;
		},
	});
}

export function translateJavaInput(source: string): TranslatedInput {
	const rawByteOffsets = utf8Boundaries(source);
	const cooked: number[] = [];
	const rawOffsets = [0];
	let position = 0;
	let trailingBackslashes = 0;
	let previousWasEscape = false;
	let previousEscape: { value: number; end: number } | null = null;

	while (position < source.length) {
		const eligible = previousWasEscape || trailingBackslashes % 2 === 0;
		if (source.charCodeAt(position) === 0x5c && eligible) {
			const escape = unicodeEscapeAt(source, position);
			if (escape !== null) {
				let lexicalValue = escape.value;
				if (escape.value >= 0xd800 && escape.value <= 0xdbff) {
					const next = unicodeEscapeAt(source, escape.end);
					if (next === null || next.value < 0xdc00 || next.value > 0xdfff) {
						lexicalValue = 0xfffd;
					}
				} else if (
					escape.value >= 0xdc00 &&
					escape.value <= 0xdfff &&
					(previousEscape === null ||
						previousEscape.end !== position ||
						previousEscape.value < 0xd800 ||
						previousEscape.value > 0xdbff)
				) {
					lexicalValue = 0xfffd;
				}
				cooked.push(lexicalValue);
				rawOffsets.push(rawByteOffsets[escape.end]);
				previousWasEscape = true;
				trailingBackslashes = escape.value === 0x5c ? trailingBackslashes + 1 : 0;
				previousEscape = escape;
				position = escape.end;
				continue;
			}
		}

		const first = source.charCodeAt(position);
		let width = 1;
		if (first >= 0xd800 && first <= 0xdbff && position + 1 < source.length) {
			const second = source.charCodeAt(position + 1);
			if (second >= 0xdc00 && second <= 0xdfff) {
				width = 2;
			}
		}
		if (position + width === source.length && first === 0x1a) {
			cooked.push(0x20);
			rawOffsets.push(rawByteOffsets[position + width]);
		} else {
			for (let offset = 0; offset < width; offset += 1) {
				cooked.push(source.charCodeAt(position + offset));
				rawOffsets.push(offset + 1 === width ? rawByteOffsets[position + width] : rawByteOffsets[position]);
			}
		}
		previousWasEscape = false;
		previousEscape = null;
		trailingBackslashes = first === 0x5c ? trailingBackslashes + 1 : 0;
		position += width;
	}

	return {
		text: codeUnitsToString(cooked),
		rawOffsets,
	};
}

function javaIdentifiers(
	term: number,
	identifierStart: ScalarRange[],
	identifierPart: ScalarRange[],
): ExternalTokenizer {
	return new ExternalTokenizer((input) => {
		let character = codePoint(input.peek(0), input.peek(1));
		if (character === null || !inRanges(identifierStart, character.value)) {
			return;
		}
		input.advance(character.width);
		for (;;) {
			character = codePoint(input.peek(0), input.peek(1));
			if (character === null || !inRanges(identifierPart, character.value)) {
				break;
			}
			input.advance(character.width);
		}
		input.acceptToken(term);
	});
}

function codePoint(first: number, second: number): CodePoint | null {
	if (first < 0) {
		return null;
	}
	if (first < 0xd800 || first > 0xdbff || second < 0xdc00 || second > 0xdfff) {
		return { value: first, width: 1 };
	}
	return {
		value: 0x1_0000 + ((first - 0xd800) << 10) + second - 0xdc00,
		width: 2,
	};
}

function inRanges(ranges: ScalarRange[], value: number): boolean {
	let low = 0;
	let high = ranges.length;
	while (low < high) {
		const middle = (low + high) >> 1;
		if (ranges[middle][1] < value) {
			low = middle + 1;
		} else {
			high = middle;
		}
	}
	return low < ranges.length && ranges[low][0] <= value;
}

function parseRanges(source: string, name: string): ScalarRange[] {
	const declaration = new RegExp(`pub(?:\\(super\\))? (?:const|static) ${name}: &\\[\\(u32, u32\\)\\] = &\\[`);
	const match = declaration.exec(source);
	if (match === null) {
		throw new Error(`missing ${name}`);
	}
	const start = match.index + match[0].length;
	const end = source.indexOf("];", start);
	assert.notEqual(end, -1, `unterminated ${name}`);
	return [...source.slice(start, end).matchAll(/\(0x([0-9a-f]+), 0x([0-9a-f]+)\)/g)].map((range) => [
		Number.parseInt(range[1], 16),
		Number.parseInt(range[2], 16),
	]);
}

function unicodeEscapeAt(source: string, offset: number): { value: number; end: number } | null {
	let cursor = offset + 1;
	if (source.charCodeAt(cursor) !== 0x75) {
		return null;
	}
	while (source.charCodeAt(cursor) === 0x75) {
		cursor += 1;
	}
	if (cursor + 4 > source.length) {
		return null;
	}
	const digits = source.slice(cursor, cursor + 4);
	if (!/^[0-9a-fA-F]{4}$/.test(digits)) {
		return null;
	}
	return {
		value: Number.parseInt(digits, 16),
		end: cursor + 4,
	};
}

function codeUnitsToString(units: number[]): string {
	const chunks: string[] = [];
	const chunkSize = 32 * 1024;
	for (let offset = 0; offset < units.length; offset += chunkSize) {
		chunks.push(String.fromCharCode(...units.slice(offset, offset + chunkSize)));
	}
	return chunks.join("");
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
