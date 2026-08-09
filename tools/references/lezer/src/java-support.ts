import assert from "node:assert/strict";

import type { NodePropSource } from "@lezer/common";
import { buildParser } from "@lezer/generator";
import { ExternalTokenizer } from "@lezer/lr";

interface BuildJavaParserOptions {
	grammar: string;
	grammarPath: string;
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

export function buildJavaParser(options: BuildJavaParserOptions): ReturnType<typeof buildParser> {
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
			return javaIdentifiers(terms.identifier);
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

function javaIdentifiers(term: number): ExternalTokenizer {
	return new ExternalTokenizer((input) => {
		let character = codePoint(input.peek(0), input.peek(1));
		if (character === null || !isIdentifierCandidateStart(character.value)) {
			return;
		}
		input.advance(character.width);
		for (;;) {
			character = codePoint(input.peek(0), input.peek(1));
			if (character === null || !isIdentifierCandidatePart(character.value)) {
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

function isIdentifierCandidateStart(value: number): boolean {
	return (
		value === 0x24 ||
		(value >= 0x41 && value <= 0x5a) ||
		value === 0x5f ||
		(value >= 0x61 && value <= 0x7a) ||
		(value >= 0xa1 && value <= 0x10ffff)
	);
}

function isIdentifierCandidatePart(value: number): boolean {
	return (
		(value >= 0x00 && value <= 0x08) ||
		(value >= 0x0e && value <= 0x1b) ||
		value === 0x24 ||
		(value >= 0x30 && value <= 0x39) ||
		(value >= 0x41 && value <= 0x5a) ||
		value === 0x5f ||
		(value >= 0x61 && value <= 0x7a) ||
		(value >= 0x7f && value <= 0x10ffff)
	);
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
