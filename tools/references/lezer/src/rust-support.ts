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

const LOWER_B = 98;
const LOWER_E = 101;
const LOWER_F = 102;
const LOWER_R = 114;
const UPPER_E = 69;
const ZERO = 48;
const DOT = 46;
const PLUS = 43;
const MINUS = 45;
const HASH = 35;
const QUOTE = 34;
const PIPE = 124;
const LESS_THAN = 60;
const GREATER_THAN = 62;

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
				case "closureParam":
					return closureParam(requiredTerm(terms, "closureParamDelim"));
				case "tpDelim":
					return typeParameterDelimiters(requiredTerm(terms, "tpOpen"), requiredTerm(terms, "tpClose"));
				case "literalTokens":
					return literalTokens({
						float: requiredTerm(terms, "Float"),
						rawString: requiredTerm(terms, "RawString"),
					});
				default:
					throw new Error(`unexpected Rust external tokenizer ${name}`);
			}
		},
		externalPropSource(name) {
			assert.equal(name, "rustHighlighting");
			return emptyProperties;
		},
	});
}

function literalTokens(terms: LiteralTerms): ExternalTokenizer {
	return new ExternalTokenizer((input) => {
		const character = next(input);
		if (isNumber(character)) {
			scanNumber(input, terms.float);
		} else if (character === LOWER_B || character === LOWER_R) {
			scanRawString(input, terms.rawString);
		}
	});
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
		const f32 = after === ZERO + 3 && input.peek(2) === ZERO + 2;
		const f64 = after === ZERO + 6 && input.peek(2) === ZERO + 4;
		if (!f32 && !f64) {
			return;
		}
		input.advance(3);
		isFloat = true;
	}

	if (isFloat) {
		input.acceptToken(float);
	}
}

function scanRawString(input: InputStream, rawString: number): void {
	if (next(input) === LOWER_B) {
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
		if (next(input) === LESS_THAN) {
			input.acceptToken(open, 1);
		} else if (next(input) === GREATER_THAN) {
			input.acceptToken(close, 1);
		}
	});
}

function next(input: InputStream): number {
	return input.next;
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
