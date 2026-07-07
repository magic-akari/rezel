# Language adaptation case studies

These case studies apply the language-package workflow to pinned external
artifacts. They are worked examples, not substitutes for the grammar reference,
not claims that Rezel ships the language, and not authorities for a language's
current specification.

A case study may be language-specific because its purpose is to preserve the
reasoning that connects one concrete upstream grammar to Rezel's generic
boundaries. Each study must:

- pin the upstream package version and source revision;
- distinguish grammar semantics from language-specific policy;
- inventory every external declaration and the terms it can produce;
- trace each declaration to its source implementation and grammar use sites;
- state the equivalent Rezel binding and focused verification obligations;
- identify behavior that remains declarative and must not be moved into an
  adapter;
- avoid presenting upstream behavior as a current language-membership claim.

General conclusions belong in the adjacent
[grammar syntax](../02-grammar-syntax.md),
[grammar design](../grammar-design/README.md), and
[language adapter](../04-language-adapters.md) chapters. A case study should
link to those rules rather than redefine them.

Available studies:

- [Audit external declarations in a JavaScript grammar](javascript-externals.md)
  is the primary broad tokenizer study. It combines multiple external
  tokenizers with context, fallback, dialect checks, parser-state queries,
  bounded lookahead, zero-length terms, and a property source. Its coverage
  table also records the external forms that this upstream grammar does not
  use; those forms remain specified by the generic chapters rather than being
  inferred from one language.
