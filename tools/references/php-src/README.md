# PHP 8.5 php-src reference corpus

This verifier checks Rezel's default `Template` entry point against every
UTF-8 `--FILE--` or `--FILEEOF--` section in the pinned PHP 8.5.9
`Zend/tests` corpus. PHP's own `token_get_all($source, TOKEN_PARSE)` classifies
syntax membership without executing the test program.

The preparation step fixes the php-src commit, complete `.phpt` inventory,
byte count, and content digest. The oracle fixes the PHP runtime version and
records each extracted section's byte count and SHA-256. Five byte-oriented
non-UTF-8 tests are outside Rezel's `&str` facade. Oracle rejections are not
used as positive inputs.

Rezel parses official positives in strict mode with zero recovery, at most two
GLR stacks, and five million parser actions per file. `known-rejections.txt`
is an exact, shrinking inventory: a new rejection fails verification, and a
listed case that starts passing also fails until its entry is removed and the
case is promoted to focused coverage.

Run:

```text
mise run reference:php-src
```

The task downloads the selected corpus into `target/reference-sources`, runs
the mise-pinned PHP 8.5.9 oracle, and then runs the Rust verifier. It belongs to
`verify:full`; normal repository verification remains network-free.
