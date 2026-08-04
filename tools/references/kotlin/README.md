# Kotlin 2.4.10 parser reference

This focused reference pins the Kotlin compiler light parser to version
2.4.10. The source fixtures are selected without semantic normalization from
`compiler/psi/psi-impl/testData/psi` and `compiler/testData` at JetBrains
Kotlin commit `5687445832cd835b4509b9fbc264cdf1a8201093`.

Run the parse-only oracle with:

```sh
mise run reference:kotlin
```

Files under `parse-accepted` must produce no light-parser errors. Files under
`parse-rejected` must produce at least one. Compiler comments such as
`COMPILATION_ERRORS` describe later semantic diagnostics and do not change
the parse-only classification.

These directories are the Kotlin compiler oracle classification. Rezel's
current strict positive support is executed separately by
`languages/kotlin/tests/reference.rs`, so it does not redefine that compiler
contract. Compiler-rejected fixtures remain oracle evidence rather than a Rezel
acceptance gate during the architecture migration.

## Standard-library corpus

The independent broad-corpus gate runs with:

```sh
mise run reference:stdlib:kotlin
```

It reads `kotlin-stdlib-sources.jar` from the mise-pinned Kotlin 2.4.10
distribution without downloading source. The runner verifies the compiler
build and archive digest, extracts the archive into a temporary directory,
and recursively selects every `.kt` file with no exclusions. It then verifies
the exact sorted path and per-file content inventories before parsing all 380
files as complete `KotlinFile` inputs in strict mode with the default runtime
resource limits. The current checkpoint accepts all 380 files, and
`stdlib/stdlib-rejections.txt` is empty.

This corpus establishes broad positive acceptance and scale over the pinned
standard library. It does not replace the focused compiler-light-parser
classification above, establish negative acceptance or semantic validity, or
redefine the Kotlin language contract around syntax absent from the archive.

## Distribution corpus

The wider migration gate runs with:

```sh
mise run reference:distribution:kotlin
```

It verifies all 13 `*-sources.jar` archives shipped by the same mise-pinned
Kotlin 2.4.10 distribution. Archive sizes and SHA-256 digests, per-archive
source counts and bytes, and the aggregate sorted path and content inventories
are fixed before parsing. The corpus contains 1,299 `.kt` or `.kts` files and
11,547,367 UTF-8 bytes.

The current checkpoint accepts all 1,299 sources, and
`stdlib/distribution-rejections.txt` is empty. If a strict rejection is
recorded later, each entry fixes the relative path, UTF-8 byte offset, and
parse-error kind. An unexpected rejection, a moved failure, or an accidentally
stale entry fails the task. This snapshot is not a claim about negative
language syntax.
