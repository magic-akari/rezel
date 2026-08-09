import assert from "node:assert/strict";

import type { NodePropSource } from "@lezer/common";
import { buildParser } from "@lezer/generator";
import { ContextTracker, ExternalTokenizer, type InputStream, type Stack } from "@lezer/lr";

interface BuildOptions {
	grammar: string;
	grammarPath: string;
	warnings: string[];
}

interface PythonContext {
	parent: PythonContext | null;
	indent: number;
	flags: number;
	hash: number;
}

const LF = 10;
const CR = 13;
const SPACE = 32;
const TAB = 9;
const HASH = 35;
const OPEN_BRACE = 123;
const CLOSE_BRACE = 125;
const SINGLE_QUOTE = 39;
const DOUBLE_QUOTE = 34;
const BACKSLASH = 92;

const BRACKETED = 1;
const STRING = 2;
const DOUBLE = 4;
const LONG = 8;
const RAW = 16;
const FORMAT = 32;

export function buildPythonParser(options: BuildOptions): ReturnType<typeof buildParser> {
	const emptyProperties: NodePropSource = () => null;

	return buildParser(options.grammar, {
		fileName: options.grammarPath,
		includeNames: true,
		warn(message) {
			options.warnings.push(message);
		},
		externalTokenizer(name, terms) {
			switch (name) {
				case "INDENTATION":
					return indentation(requiredTerm(terms, "indent"), requiredTerm(terms, "dedent"));
				case "NEWLINES":
					return newlines(terms);
				case "STRINGS":
					return strings(terms);
				case "TOKENIZER":
					return identifiers(requiredTerm(terms, "identifier"));
				default:
					throw new Error(`unknown Python tokenizer ${name}`);
			}
		},
		contextTracker(terms) {
			const bracketed = new Set(
				[
					"ParenthesizedExpression",
					"parenthesizedWithItems",
					"TupleExpression",
					"ComprehensionExpression",
					"importList",
					"ArgList",
					"ParamList",
					"ArrayExpression",
					"ArrayComprehensionExpression",
					"subscript",
					"SetExpression",
					"SetComprehensionExpression",
					"FormatString",
					"TemplateString",
					"FormatReplacement",
					"TemplateInterpolation",
					"nestedFormatReplacement",
					"DictionaryExpression",
					"DictionaryComprehensionExpression",
					"SequencePattern",
					"MappingPattern",
					"PatternArgList",
					"TypeParamList",
				].map((name) => requiredTerm(terms, name)),
			);
			const starts = stringFlags(terms);
			const top: PythonContext = { parent: null, indent: 0, flags: 0, hash: 0 };
			return new ContextTracker<PythonContext>({
				start: top,
				shift(context, term, stack, input) {
					if (term === requiredTerm(terms, "indent")) {
						const readable = input as InputStream & { read(from: number, to: number): string };
						return child(context, countIndent(readable.read(input.pos, stack.pos)), 0);
					}
					if (term === requiredTerm(terms, "dedent")) {
						return context.parent ?? top;
					}
					if (
						term === requiredTerm(terms, "ParenL") ||
						term === requiredTerm(terms, "BracketL") ||
						term === requiredTerm(terms, "BraceL") ||
						term === requiredTerm(terms, "replacementStart")
					) {
						return child(context, 0, BRACKETED);
					}
					const flags = starts.get(term);
					return flags === undefined ? context : child(context, 0, flags | (context.flags & BRACKETED));
				},
				reduce(context, term) {
					const closesBracket = (context.flags & BRACKETED) !== 0 && bracketed.has(term);
					const closesString =
						(context.flags & STRING) !== 0 &&
						(term === requiredTerm(terms, "String") ||
							term === requiredTerm(terms, "FormatString") ||
							term === requiredTerm(terms, "TemplateString"));
					return closesBracket || closesString ? (context.parent ?? top) : context;
				},
				hash(context) {
					return context.hash;
				},
			});
		},
		externalPropSource(name) {
			assert.equal(name, "python_highlighting");
			return emptyProperties;
		},
	});
}

function newlines(terms: Record<string, number>): ExternalTokenizer {
	return new ExternalTokenizer(
		(input, stack) => {
			const context = stack.context as PythonContext;
			if (input.next < 0) {
				input.acceptToken(requiredTerm(terms, "eof"));
			} else if ((context.flags & BRACKETED) !== 0) {
				if (isLineBreak(input.next)) input.acceptToken(requiredTerm(terms, "newlineBracketed"), 1);
			} else if (
				(input.peek(-1) < 0 || isLineBreak(input.peek(-1))) &&
				stack.canShift(requiredTerm(terms, "blankLineStart"))
			) {
				let spaces = 0;
				while (input.next === SPACE || input.next === TAB) {
					input.advance();
					spaces += 1;
				}
				if (input.next < 0 || isLineBreak(input.next) || input.next === HASH) {
					input.acceptToken(requiredTerm(terms, "blankLineStart"), -spaces);
				}
			} else if (isLineBreak(input.next)) {
				input.acceptToken(requiredTerm(terms, "newline"), 1);
			}
		},
		{ contextual: true },
	);
}

function indentation(indent: number, dedent: number): ExternalTokenizer {
	return new ExternalTokenizer((input, stack) => {
		const context = stack.context as PythonContext;
		if (context.flags !== 0) return;
		const previous = input.peek(-1);
		if (previous >= 0 && !isLineBreak(previous)) return;
		let depth = 0;
		let characters = 0;
		for (;;) {
			if (input.next === SPACE) depth += 1;
			else if (input.next === TAB) depth += 8 - (depth % 8);
			else break;
			input.advance();
			characters += 1;
		}
		if (depth !== context.indent && input.next >= 0 && !isLineBreak(input.next) && input.next !== HASH) {
			input.acceptToken(depth < context.indent ? dedent : indent, depth < context.indent ? -characters : 0);
		}
	});
}

function strings(terms: Record<string, number>): ExternalTokenizer {
	return new ExternalTokenizer(
		(input, stack) => {
			const flags = (stack.context as PythonContext).flags;
			const quote = (flags & DOUBLE) !== 0 ? DOUBLE_QUOTE : SINGLE_QUOTE;
			const long = (flags & LONG) !== 0;
			const escapes = (flags & RAW) === 0;
			const format = (flags & FORMAT) !== 0;
			const start = input.pos;
			for (;;) {
				if (input.next < 0) break;
				if (format && input.next === OPEN_BRACE) {
					if (input.peek(1) === OPEN_BRACE) input.advance(2);
					else if (input.pos === start) {
						input.acceptToken(requiredTerm(terms, "replacementStart"), 1);
						return;
					} else break;
				} else if (escapes && input.next === BACKSLASH) {
					if (input.pos !== start) break;
					input.advance();
					const escaped = input.next;
					if (escaped >= 0) {
						input.advance();
						skipEscape(input, escaped);
					}
					input.acceptToken(requiredTerm(terms, "Escape"));
					return;
				} else if (input.next === BACKSLASH && input.peek(1) >= 0) {
					input.advance(2);
				} else if (input.next === quote && (!long || (input.peek(1) === quote && input.peek(2) === quote))) {
					if (input.pos === start) {
						input.acceptToken(requiredTerm(terms, "stringEnd"), long ? 3 : 1);
						return;
					}
					break;
				} else if (input.next === LF) {
					if (long) input.advance();
					else if (input.pos === start) {
						input.acceptToken(requiredTerm(terms, "stringEnd"));
						return;
					} else break;
				} else input.advance();
			}
			if (input.pos > start) input.acceptToken(requiredTerm(terms, "stringContent"));
		},
		{ contextual: true },
	);
}

function identifiers(term: number): ExternalTokenizer {
	return new ExternalTokenizer((input) => {
		if (looksLikeStringPrefix(input)) return;
		let character = codePoint(input.peek(0), input.peek(1));
		if (character === null || !isIdentifierCandidateStart(character.value)) return;
		input.advance(character.width);
		for (;;) {
			character = codePoint(input.peek(0), input.peek(1));
			if (character === null || !isIdentifierCandidateContinue(character.value)) break;
			input.advance(character.width);
		}
		input.acceptToken(term);
	});
}

function stringFlags(terms: Record<string, number>): Map<number, number> {
	const result = new Map<number, number>();
	for (const [name, flags] of [
		["stringStart", 0],
		["stringStartD", DOUBLE],
		["stringStartL", LONG],
		["stringStartLD", LONG | DOUBLE],
		["stringStartR", RAW],
		["stringStartRD", RAW | DOUBLE],
		["stringStartRL", RAW | LONG],
		["stringStartRLD", RAW | LONG | DOUBLE],
		["stringStartF", FORMAT],
		["stringStartFD", FORMAT | DOUBLE],
		["stringStartFL", FORMAT | LONG],
		["stringStartFLD", FORMAT | LONG | DOUBLE],
		["stringStartFR", FORMAT | RAW],
		["stringStartFRD", FORMAT | RAW | DOUBLE],
		["stringStartFRL", FORMAT | RAW | LONG],
		["stringStartFRLD", FORMAT | RAW | LONG | DOUBLE],
		["stringStartT", FORMAT],
		["stringStartTD", FORMAT | DOUBLE],
		["stringStartTL", FORMAT | LONG],
		["stringStartTLD", FORMAT | LONG | DOUBLE],
		["stringStartTR", FORMAT | RAW],
		["stringStartTRD", FORMAT | RAW | DOUBLE],
		["stringStartTRL", FORMAT | RAW | LONG],
		["stringStartTRLD", FORMAT | RAW | LONG | DOUBLE],
	] as const) {
		result.set(requiredTerm(terms, name), flags | STRING);
	}
	return result;
}

function child(parent: PythonContext, indent: number, flags: number): PythonContext {
	return {
		parent,
		indent,
		flags,
		hash: (((parent.hash * 257 + indent) * 67 + flags) | 0) >>> 0,
	};
}

function countIndent(value: string): number {
	let depth = 0;
	for (const character of value) {
		if (character === "\t") depth += 8 - (depth % 8);
		else if (character === "\f") depth = 0;
		else depth += 1;
	}
	return depth;
}

function skipEscape(input: InputStream, escaped: number): void {
	const limits = new Map([
		[111, 2],
		[120, 2],
		[117, 4],
		[85, 8],
	]);
	const limit = limits.get(escaped);
	if (limit !== undefined) {
		for (let index = 0; index < limit && isHex(input.next); index += 1) input.advance();
	} else if (escaped === 78 && input.next === OPEN_BRACE) {
		input.advance();
		for (;;) {
			const next = input.next as number;
			if (next < 0 || [CLOSE_BRACE, SINGLE_QUOTE, DOUBLE_QUOTE, LF].includes(next)) break;
			input.advance();
		}
		if ((input.next as number) === CLOSE_BRACE) input.advance();
	}
}

function looksLikeStringPrefix(input: InputStream): boolean {
	const first = String.fromCharCode(input.peek(0)).toLowerCase();
	const second = input.peek(1);
	if (second === SINGLE_QUOTE || second === DOUBLE_QUOTE) return "bfrtu".includes(first);
	const pair = first + String.fromCharCode(second).toLowerCase();
	return (
		["br", "rb", "fr", "rf", "tr", "rt"].includes(pair) &&
		(input.peek(2) === SINGLE_QUOTE || input.peek(2) === DOUBLE_QUOTE)
	);
}

function codePoint(first: number, second: number): { value: number; width: number } | null {
	if (first < 0) return null;
	if (first < 0xd800 || first > 0xdbff || second < 0xdc00 || second > 0xdfff) {
		return { value: first, width: 1 };
	}
	return { value: 0x1_0000 + ((first - 0xd800) << 10) + second - 0xdc00, width: 2 };
}

function isIdentifierCandidateStart(value: number): boolean {
	return (
		(value >= 65 && value <= 90) ||
		value === 95 ||
		(value >= 97 && value <= 122) ||
		(value >= 0xa1 && value <= 0x10_ffff)
	);
}

function isIdentifierCandidateContinue(value: number): boolean {
	return (value >= 48 && value <= 57) || isIdentifierCandidateStart(value);
}

function isLineBreak(value: number): boolean {
	return value === LF || value === CR;
}

function isHex(value: number): boolean {
	return (value >= 48 && value <= 57) || (value >= 65 && value <= 70) || (value >= 97 && value <= 102);
}

function requiredTerm(terms: Record<string, number>, name: string): number {
	const term = terms[name];
	if (typeof term !== "number") throw new Error(`the maintained Python grammar did not export ${name}`);
	return term;
}
