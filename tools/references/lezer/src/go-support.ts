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
			assert.equal(name, "semicolon");
			const insertedSemi = requiredTerm(terms, "insertedSemi");
			return new ExternalTokenizer(
				(input, stack) => {
					scanSemicolon(input, stack, insertedSemi);
				},
				{ contextual: true },
			);
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
			assert.equal(name, "goHighlighting");
			return emptyProperties;
		},
	});
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
