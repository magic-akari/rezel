import assert from "node:assert/strict";

import type { NodePropSource } from "@lezer/common";
import { buildParser } from "@lezer/generator";
import { ContextTracker, ExternalTokenizer, type InputStream, type Stack } from "@lezer/lr";

interface BuildGoParserOptions {
	grammar: string;
	grammarPath: string;
	warnings: string[];
}

interface BlockCommentScan {
	after: number;
	containsLineEnd: boolean;
}

const NEWLINE = 10;
const CARRIAGE_RETURN = 13;
const SPACE = 32;
const TAB = 9;
const SLASH = 47;
const ASTERISK = 42;
const CLOSE_PAREN = 41;
const CLOSE_BRACE = 125;
const OPEN_BRACKET = 91;
const LESS_THAN = 60;
const MINUS = 45;

const TRACKED_TERM_NAMES = [
	"IncDecOp",
	"identifier",
	"Rune",
	"String",
	"Number",
	"predeclaredBool",
	"predeclaredNil",
	"break",
	"continue",
	"return",
	"fallthrough",
	"closeParen",
	"closeBracket",
	"closeBrace",
] as const;

export function buildGoParser(options: BuildGoParserOptions): ReturnType<typeof buildParser> {
	const emptyProperties: NodePropSource = () => null;

	return buildParser(options.grammar, {
		fileName: options.grammarPath,
		includeNames: true,
		warn(message) {
			options.warnings.push(message);
		},
		externalTokenizer(name, terms) {
			switch (name) {
				case "SEMICOLON": {
					const insertedSemi = requiredTerm(terms, "insertedSemi");
					return new ExternalTokenizer(
						(input, stack) => {
							scanSemicolon(input, stack, insertedSemi);
						},
						{ contextual: true },
					);
				}
				case "INDEX_TYPE": {
					const indexTypeStart = requiredTerm(terms, "indexTypeStart");
					return new ExternalTokenizer(
						(input, stack) => {
							if (stack.canShift(indexTypeStart) && startsIndexType(input)) {
								input.acceptToken(indexTypeStart, 0);
							}
						},
						{ extend: true },
					);
				}
				default:
					throw new Error(`unknown Go tokenizer ${name}`);
			}
		},
		contextTracker(terms) {
			const space = requiredTerm(terms, "space");
			const trackedTerms = new Set(TRACKED_TERM_NAMES.map((name) => requiredTerm(terms, name)));
			return new ContextTracker<boolean>({
				start: false,
				shift(context, term) {
					return term === space ? context : trackedTerms.has(term);
				},
				hash(context) {
					return Number(context);
				},
			});
		},
		externalPropSource(name) {
			assert.equal(name, "go_highlighting");
			return emptyProperties;
		},
	});
}

function startsIndexType(input: InputStream): boolean {
	const first = input.peek(0);
	if (first === OPEN_BRACKET) return true;
	if (first === LESS_THAN) {
		if (input.peek(1) !== MINUS) return false;
		const start = skipGoTrivia(input, 2);
		return start >= 0 && nextWordIs(input, start, "chan");
	}
	for (const keyword of ["chan", "func", "interface", "map", "struct"]) {
		if (nextWordIs(input, 0, keyword)) return true;
	}
	return false;
}

function nextWordIs(input: InputStream, start: number, expected: string): boolean {
	for (let index = 0; index < expected.length; index += 1) {
		if (input.peek(start + index) !== expected.charCodeAt(index)) return false;
	}
	const next = input.peek(start + expected.length);
	const asciiAlphanumeric = (next >= 48 && next <= 57) || (next >= 65 && next <= 90) || (next >= 97 && next <= 122);
	return next < 0 || (next < 128 && next !== 95 && !asciiAlphanumeric);
}

function skipGoTrivia(input: InputStream, initial: number): number {
	let scan = initial;
	for (;;) {
		while ([SPACE, TAB, NEWLINE, CARRIAGE_RETURN].includes(input.peek(scan))) scan += 1;
		if (input.peek(scan) !== SLASH) return scan;
		const comment = input.peek(scan + 1);
		if (comment === SLASH) {
			scan += 2;
			while (![NEWLINE, CARRIAGE_RETURN, -1].includes(input.peek(scan))) scan += 1;
			continue;
		}
		if (comment !== ASTERISK) return scan;
		scan += 2;
		for (;;) {
			const next = input.peek(scan);
			if (next < 0) return -1;
			if (next === ASTERISK && input.peek(scan + 1) === SLASH) {
				scan += 2;
				break;
			}
			scan += 1;
		}
	}
}

function scanSemicolon(input: InputStream, stack: Stack, insertedSemi: number): void {
	let scan = 0;
	for (;;) {
		const next = input.peek(scan);
		if (next === SPACE || next === TAB) {
			scan += 1;
			continue;
		}

		const lineEnd = next < 0 || next === NEWLINE || next === CARRIAGE_RETURN;
		const lineComment = next === SLASH && input.peek(scan + 1) === SLASH;
		if (stack.context === true && (lineEnd || lineComment)) {
			input.acceptToken(insertedSemi, 0);
			return;
		}

		const blockComment = next === SLASH && input.peek(scan + 1) === ASTERISK;
		if (blockComment) {
			const result = scanBlockComment(input, scan);
			if (result.containsLineEnd) {
				if (stack.context === true) {
					input.acceptToken(insertedSemi, 0);
				}
				return;
			}
			scan = result.after;
			continue;
		}

		if (next === CLOSE_PAREN || next === CLOSE_BRACE) {
			input.acceptToken(insertedSemi, 0);
		}
		return;
	}
}

function scanBlockComment(input: InputStream, start: number): BlockCommentScan {
	let scan = start + 2;
	for (;;) {
		const next = input.peek(scan);
		if (next < 0 || next === NEWLINE || next === CARRIAGE_RETURN) {
			return { after: scan, containsLineEnd: true };
		}
		if (next === ASTERISK && input.peek(scan + 1) === SLASH) {
			return { after: scan + 2, containsLineEnd: false };
		}
		scan += 1;
	}
}

function requiredTerm(terms: Record<string, number>, name: string): number {
	const term = terms[name];
	if (typeof term !== "number") {
		throw new Error(`the maintained Go grammar did not export ${name}`);
	}
	return term;
}
