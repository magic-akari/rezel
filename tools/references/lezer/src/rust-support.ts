import assert from "node:assert/strict";

import type { NodePropSource } from "@lezer/common";
import { buildParser } from "@lezer/generator";
import { ExternalTokenizer, type InputStream } from "@lezer/lr";

interface BuildRustParserOptions {
	grammar: string;
	grammarPath: string;
	warnings: string[];
}

interface LiteralTerms {
	float: number;
	rawString: number;
}

interface CodePoint {
	value: number;
	width: number;
}

const LOWER_B = 98;
const LOWER_C = 99;
const LOWER_E = 101;
const LOWER_F = 102;
const LOWER_M = 109;
const LOWER_R = 114;
const UPPER_E = 69;
const ZERO = 48;
const DOT = 46;
const PLUS = 43;
const MINUS = 45;
const HASH = 35;
const QUOTE = 34;
const SINGLE_QUOTE = 39;
const DOLLAR = 36;
const UNDERSCORE = 95;
const PIPE = 124;
const LESS_THAN = 60;
const GREATER_THAN = 62;
const EQUAL = 61;
const BANG = 33;
const SLASH = 47;
const STAR = 42;
const LEFT_BRACKET = 91;
const LINE_FEED = 10;
const BYTE_ORDER_MARK = 0xfeff;
const XID_START = /^\p{XID_Start}$/u;
const XID_CONTINUE = /^\p{XID_Continue}$/u;
const MACRO_RULES = "macro_rules";
const RESERVED_RAW_NAMES = new Set(["_", "crate", "self", "Self", "super"]);

export function rustSourceInput(source: string): string {
	const hiddenRanges: Array<readonly [number, number]> = [];
	let shebangStart = 0;
	const first = sourceCodePoint(source, shebangStart);
	if (first?.value === BYTE_ORDER_MARK) {
		shebangStart += first.width;
		hiddenRanges.push([0, shebangStart]);
	}

	const shebangEnd = sourceShebangEnd(source, shebangStart);
	if (shebangEnd !== null) {
		hiddenRanges.push([shebangStart, shebangEnd]);
	}
	if (hiddenRanges.length === 0) {
		return source;
	}

	let result = "";
	let position = 0;
	for (const [start, end] of hiddenRanges) {
		result += source.slice(position, start);
		for (const character of source.slice(start, end)) {
			result += " ".repeat(character.length);
		}
		position = end;
	}
	result += source.slice(position);
	return result;
}

function sourceShebangEnd(source: string, start: number): number | null {
	if (source.codePointAt(start) !== HASH || source.codePointAt(start + 1) !== BANG) {
		return null;
	}
	if (nextSourceTokenIsLeftBracket(source, start + 2)) {
		return null;
	}
	const lineFeed = source.indexOf("\n", start);
	return lineFeed === -1 ? source.length : lineFeed;
}

function nextSourceTokenIsLeftBracket(source: string, start: number): boolean {
	let position = start;
	for (;;) {
		let character = sourceCodePoint(source, position);
		while (character !== null && isWhitespace(character.value)) {
			position += character.width;
			character = sourceCodePoint(source, position);
		}
		if (character?.value !== SLASH) {
			return character?.value === LEFT_BRACKET;
		}

		const comment = sourceCodePoint(source, position + character.width);
		if (comment?.value === SLASH) {
			position += character.width + comment.width;
			for (;;) {
				character = sourceCodePoint(source, position);
				if (character === null || character.value === LINE_FEED) {
					break;
				}
				position += character.width;
			}
		} else if (comment?.value === STAR) {
			position += character.width + comment.width;
			const end = sourceBlockCommentEnd(source, position);
			if (end === null) {
				return false;
			}
			position = end;
		} else {
			return false;
		}
	}
}

function sourceBlockCommentEnd(source: string, start: number): number | null {
	let depth = 1;
	let position = start;
	for (;;) {
		const character = sourceCodePoint(source, position);
		if (character === null) {
			return null;
		}
		const nextPosition = position + character.width;
		const nextCharacter = sourceCodePoint(source, nextPosition);
		if (character.value === SLASH && nextCharacter?.value === STAR) {
			depth += 1;
			position = nextPosition + nextCharacter.width;
		} else if (character.value === STAR && nextCharacter?.value === SLASH) {
			depth -= 1;
			position = nextPosition + nextCharacter.width;
			if (depth === 0) {
				return position;
			}
		} else {
			position = nextPosition;
		}
	}
}

function sourceCodePoint(source: string, position: number): CodePoint | null {
	const value = source.codePointAt(position);
	if (value === undefined) {
		return null;
	}
	return {
		value,
		width: value > 0xffff ? 2 : 1,
	};
}

export function buildRustParser(options: BuildRustParserOptions): ReturnType<typeof buildParser> {
	const emptyProperties: NodePropSource = () => null;

	return buildParser(options.grammar, {
		fileName: options.grammarPath,
		includeNames: true,
		warn(message) {
			options.warnings.push(message);
		},
		externalTokenizer(name, terms) {
			switch (name) {
				case "CLOSURE_PARAM":
					return closureParam(requiredTerm(terms, "closureParamDelim"));
				case "TYPE_PARAMETER_DELIMITERS":
					return typeParameterDelimiters(requiredTerm(terms, "tpOpen"), requiredTerm(terms, "tpClose"));
				case "LITERALS":
					return literalTokens({
						float: requiredTerm(terms, "Float"),
						rawString: requiredTerm(terms, "RawString"),
					});
				case "MACRO_RULES_TOKENIZER":
					return rustMacroRules(requiredTerm(terms, "macroRulesKeyword"));
				case "TOKEN_IDENTIFIER_TOKENIZER":
					return rustIdentifiers(requiredTerm(terms, "tokenIdentifier"));
				case "IDENTIFIER_TOKENIZER":
					return rustIdentifiers(requiredTerm(terms, "identifier"));
				case "LIFETIME_TOKENIZER":
					return rustLifetimes(requiredTerm(terms, "quoteIdentifier"));
				case "METAVARIABLE_TOKENIZER":
					return rustMetavariables(requiredTerm(terms, "Metavariable"));
				default:
					throw new Error(`unexpected Rust external tokenizer ${name}`);
			}
		},
		externalPropSource(name) {
			assert.equal(name, "rust_highlighting");
			return emptyProperties;
		},
	});
}

function literalTokens(terms: LiteralTerms): ExternalTokenizer {
	return new ExternalTokenizer((input) => {
		const character = next(input);
		if (isNumber(character)) {
			scanNumber(input, terms.float);
		} else if (character === LOWER_B || character === LOWER_C || character === LOWER_R) {
			scanRawString(input, terms.rawString);
		}
	});
}

function rustMacroRules(term: number): ExternalTokenizer {
	return new ExternalTokenizer((input) => {
		if (next(input) !== LOWER_M) {
			return;
		}
		const name = scanIdentifierBody(input, true);
		if (name === MACRO_RULES && !isReservedPrefixDelimiter(next(input)) && macroRulesDefinitionFollows(input)) {
			input.acceptToken(term);
		}
	});
}

function rustIdentifiers(term: number): ExternalTokenizer {
	return new ExternalTokenizer((input) => scanIdentifier(input, term));
}

function rustLifetimes(term: number): ExternalTokenizer {
	return new ExternalTokenizer((input) => {
		if (next(input) === SINGLE_QUOTE) {
			scanLifetime(input, term);
		}
	});
}

function rustMetavariables(term: number): ExternalTokenizer {
	return new ExternalTokenizer((input) => {
		if (next(input) === DOLLAR) {
			scanMetavariable(input, term);
		}
	});
}

function scanIdentifier(input: InputStream, term: number): void {
	const raw = next(input) === LOWER_R && input.peek(1) === HASH;
	if (raw) {
		input.advance(2);
	}

	const name = scanIdentifierBody(input, raw);
	if (name === undefined) {
		return;
	}
	if (raw ? name !== null && RESERVED_RAW_NAMES.has(name) : isReservedPrefixDelimiter(next(input))) {
		return;
	}

	input.acceptToken(term);
}

function macroRulesDefinitionFollows(input: InputStream): boolean {
	// Keep the reference tokenizer aligned with rustc's `is_macro_rules_item`
	// decision and the native tokenizer's deterministic LR boundary.
	const lookahead = { input, offset: 0 };
	if (!skipTrivia(lookahead) || advanceLookahead(lookahead) !== BANG) {
		return false;
	}
	return skipTrivia(lookahead) && nextLookaheadTokenIsIdentifier(lookahead);
}

interface Lookahead {
	input: InputStream;
	offset: number;
}

function peekLookahead(lookahead: Lookahead): CodePoint | null {
	return codePoint(lookahead.input.peek(lookahead.offset), lookahead.input.peek(lookahead.offset + 1));
}

function advanceLookahead(lookahead: Lookahead): number | null {
	const character = peekLookahead(lookahead);
	if (character === null) {
		return null;
	}
	lookahead.offset += character.width;
	return character.value;
}

function skipTrivia(lookahead: Lookahead): boolean {
	for (;;) {
		let character = peekLookahead(lookahead);
		while (character !== null && isWhitespace(character.value)) {
			advanceLookahead(lookahead);
			character = peekLookahead(lookahead);
		}
		if (character?.value !== SLASH) {
			return true;
		}

		advanceLookahead(lookahead);
		character = peekLookahead(lookahead);
		if (character?.value === SLASH) {
			do {
				advanceLookahead(lookahead);
				character = peekLookahead(lookahead);
			} while (character !== null && character.value !== LINE_FEED);
		} else if (character?.value === STAR) {
			advanceLookahead(lookahead);
			if (!skipBlockComment(lookahead)) {
				return false;
			}
		} else {
			return false;
		}
	}
}

function skipBlockComment(lookahead: Lookahead): boolean {
	let depth = 1;
	for (;;) {
		const character = advanceLookahead(lookahead);
		if (character === null) {
			return false;
		}
		const nextCharacter = peekLookahead(lookahead)?.value;
		if (character === SLASH && nextCharacter === STAR) {
			advanceLookahead(lookahead);
			depth += 1;
		} else if (character === STAR && nextCharacter === SLASH) {
			advanceLookahead(lookahead);
			depth -= 1;
			if (depth === 0) {
				return true;
			}
		}
	}
}

function nextLookaheadTokenIsIdentifier(lookahead: Lookahead): boolean {
	let first = advanceLookahead(lookahead);
	if (first === null) {
		return false;
	}
	const raw = first === LOWER_R && peekLookahead(lookahead)?.value === HASH;
	if (raw) {
		advanceLookahead(lookahead);
		first = advanceLookahead(lookahead);
	}
	if (first === null || (first !== UNDERSCORE && !isXidStart(first))) {
		return false;
	}

	let spelling: string | null = raw ? "" : null;
	if (spelling !== null) {
		spelling += String.fromCodePoint(first);
	}
	let character = peekLookahead(lookahead);
	while (character !== null && isXidContinue(character.value)) {
		advanceLookahead(lookahead);
		if (spelling !== null) {
			spelling = character.value > 0x7f ? null : spelling + String.fromCodePoint(character.value);
		}
		character = peekLookahead(lookahead);
	}

	return raw
		? spelling === null || !RESERVED_RAW_NAMES.has(spelling)
		: character === null || !isReservedPrefixDelimiter(character.value);
}

function scanMetavariable(input: InputStream, term: number): void {
	input.advance();
	if (scanIdentifierBody(input, false) !== undefined) {
		input.acceptToken(term);
	}
}

function scanLifetime(input: InputStream, term: number): void {
	input.advance();
	const raw = next(input) === LOWER_R && input.peek(1) === HASH;
	if (raw) {
		input.advance(2);
	}

	const name = scanIdentifierBody(input, raw);
	if (name === undefined || next(input) === SINGLE_QUOTE) {
		return;
	}
	if (raw ? name !== null && RESERVED_RAW_NAMES.has(name) : next(input) === HASH) {
		return;
	}
	input.acceptToken(term);
}

function scanIdentifierBody(input: InputStream, captureAscii: boolean): string | null | undefined {
	let character = codePoint(input.peek(0), input.peek(1));
	if (character === null || (character.value !== UNDERSCORE && !isXidStart(character.value))) {
		return undefined;
	}

	let spelling: string | null = captureAscii ? "" : null;
	for (;;) {
		if (spelling !== null) {
			if (character.value > 0x7f) {
				spelling = null;
			} else {
				spelling += String.fromCodePoint(character.value);
			}
		}
		input.advance(character.width);
		character = codePoint(input.peek(0), input.peek(1));
		if (character === null || !isXidContinue(character.value)) {
			return spelling;
		}
	}
}

function scanNumber(input: InputStream, float: number): void {
	let isFloat = false;
	do {
		input.advance();
	} while (isNumberOrUnderscore(next(input)));

	if (next(input) === DOT) {
		isFloat = true;
		input.advance();
		if (isNumber(next(input))) {
			do {
				input.advance();
			} while (isNumberOrUnderscore(next(input)));
		} else if (next(input) === DOT || next(input) > 0x7f || /\w/u.test(String.fromCharCode(next(input)))) {
			return;
		}
	}

	if (next(input) === LOWER_E || next(input) === UPPER_E) {
		isFloat = true;
		input.advance();
		if (next(input) === PLUS || next(input) === MINUS) {
			input.advance();
		}
		if (!isNumberOrUnderscore(next(input))) {
			return;
		}
		do {
			input.advance();
		} while (isNumberOrUnderscore(next(input)));
	}

	if (next(input) === LOWER_F) {
		const after = input.peek(1);
		const f16 = after === ZERO + 1 && input.peek(2) === ZERO + 6;
		const f32 = after === ZERO + 3 && input.peek(2) === ZERO + 2;
		const f64 = after === ZERO + 6 && input.peek(2) === ZERO + 4;
		const f128 = after === ZERO + 1 && input.peek(2) === ZERO + 2 && input.peek(3) === ZERO + 8;
		if (!f16 && !f32 && !f64 && !f128) {
			return;
		}
		input.advance(f128 ? 4 : 3);
		isFloat = true;
	}

	if (isFloat) {
		input.acceptToken(float);
	}
}

function scanRawString(input: InputStream, rawString: number): void {
	if (next(input) === LOWER_B || next(input) === LOWER_C) {
		input.advance();
	}
	if (next(input) !== LOWER_R) {
		return;
	}
	input.advance();

	let hashes = 0;
	while (next(input) === HASH) {
		hashes += 1;
		input.advance();
	}
	if (next(input) !== QUOTE) {
		return;
	}
	input.advance();

	content: for (;;) {
		if (next(input) < 0) {
			return;
		}
		const isQuote = next(input) === QUOTE;
		input.advance();
		if (!isQuote) {
			continue;
		}
		for (let index = 0; index < hashes; index += 1) {
			if (next(input) !== HASH) {
				continue content;
			}
			input.advance();
		}
		input.acceptToken(rawString);
		return;
	}
}

function closureParam(term: number): ExternalTokenizer {
	return new ExternalTokenizer((input) => {
		if (next(input) === PIPE) {
			input.acceptToken(term, 1);
		}
	});
}

function typeParameterDelimiters(open: number, close: number): ExternalTokenizer {
	return new ExternalTokenizer((input) => {
		if (next(input) === LESS_THAN && input.peek(1) !== EQUAL) {
			input.acceptToken(open, 1);
		} else if (next(input) === GREATER_THAN) {
			input.acceptToken(close, 1);
		}
	});
}

function next(input: InputStream): number {
	return input.next;
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

function isXidStart(character: number): boolean {
	return XID_START.test(String.fromCodePoint(character));
}

function isXidContinue(character: number): boolean {
	return XID_CONTINUE.test(String.fromCodePoint(character));
}

function isReservedPrefixDelimiter(character: number): boolean {
	return character === HASH || character === SINGLE_QUOTE || character === QUOTE;
}

function isWhitespace(character: number): boolean {
	return (
		(character >= 0x0009 && character <= 0x000d) ||
		character === 0x0020 ||
		character === 0x0085 ||
		character === 0x200e ||
		character === 0x200f ||
		character === 0x2028 ||
		character === 0x2029
	);
}

function isNumber(character: number): boolean {
	return character >= ZERO && character <= ZERO + 9;
}

function isNumberOrUnderscore(character: number): boolean {
	return isNumber(character) || character === 95;
}

function requiredTerm(terms: Record<string, number>, name: string): number {
	const term = terms[name];
	if (typeof term !== "number") {
		throw new Error(`the maintained Rust grammar did not export ${name}`);
	}
	return term;
}
