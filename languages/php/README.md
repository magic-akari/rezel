# rezel-lang-php

PHP 8.5 concrete syntax for Rezel.

The maintained grammar starts from the pinned `@lezer/php` 1.0.5 grammar and
is updated against PHP 8.5.9's Zend parser and scanner. The pinned Lezer parser
defines the inherited CST compatibility baseline; PHP and Zend define language
membership. Intentional differences are kept as focused reference cases.

## Parsing

`Template` is the default entry point. It preserves text outside PHP tags as
`Text` and parses every PHP island. `Program` parses PHP code without opening
and closing tags:

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let template = rezel_lang_php::parser()
    .with_strict(true)
    .parse("<h1><?= $title ?></h1>")?;
let program = rezel_lang_php::program_parser()
    .with_strict(true)
    .parse("echo $title;")?;
assert_eq!(template.top_node().name().as_ref(), "Template");
assert_eq!(program.top_node().name().as_ref(), "Program");
# Ok(())
# }
```

The default parser recovers from syntax errors. Strict mode rejects syntax
outside the maintained PHP 8.5 contract. Positive acceptance is the primary
conformance target. Negative fixtures use strict mode because it stops at the
first failed parser position instead of paying for recovery search. All public
positions are raw UTF-8 byte offsets. The package accepts valid UTF-8 source
strings; PHP's broader byte-oriented source model is outside this facade.

The PHP CST does not parse `Text` as HTML. A caller may compose another parser
over those ranges through Rezel's mixed-parsing support.

## Syntax authorities

The PHP 8.5.9 Zend parser and scanner decide strict language membership. The
pinned `@lezer/php` 1.0.5 package decides the inherited CST names and shapes.
Newer PHP constructs keep those existing structural conventions and add the
smallest grammar-owned nodes needed to retain their syntax. This separates
language correctness from editor-tree compatibility when the older Lezer
grammar cannot recognize current PHP.

This package does not promise an owned AST. PHP itself does not expose a stable
public AST contract comparable to Go's `go/ast` or `CPython`'s `ast`, and choosing
a third-party semantic model would add a second compatibility surface without
improving parsing coverage. The source-preserving generic CST and partial typed
CST are the maintained v1 surfaces. An owned AST should be introduced only with
an explicit downstream use case and separately versioned authority.

## Verification corpora

The corpus is layered so that each source answers one question:

- the complete pinned Lezer fixture set checks inherited positive CST shape;
- focused PHP 8.5 cases check version boundaries and intentional CST additions;
- exact `--FILE--` sections from PHP 8.5.9 `Zend/tests` check official positive
  syntax combinations;
- pinned real-world application sources measure broad strict
  acceptance and parser throughput at representative file sizes.

`mise run reference:php-src` verifies the complete pinned Zend corpus against
PHP 8.5.9's `TOKEN_PARSE` oracle, then requires Rezel's exact strict rejection
inventory to match. The corpus is part of `verify:full`; the normal verification
suite remains network-free.

Correctness corpora retain complete files and stable upstream identities.
Performance inputs are sampled separately into size bands and run through the
fixed default `Template` entry point. They reuse a constructed parser and use
strict parsing as an unmeasured preflight. `Program` remains a correctness and
API entry point rather than a second benchmark profile. Malformed-source
coverage stays focused: strict first-error rejection is the negative contract
because it is cheaper than recovery search.

## Typed CST

Generated typed wrappers are zero-copy views over the maintained CST. Initial
coverage focuses on roots and stable high-level syntax families and expands
with the language contract; partial typed coverage is not a syntax-coverage
claim.

## Highlighting

The optional `highlight` feature exposes `highlight_spans`. Highlighting is a
syntactic CST projection and performs no name resolution or type analysis.
