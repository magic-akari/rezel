#!/usr/bin/env python3
"""Fetch and materialize the pinned P0 Swift syntax corpora."""

from __future__ import annotations

import bisect
import hashlib
import json
import os
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
from dataclasses import dataclass
from pathlib import Path, PurePosixPath


SWIFT_COMMIT = "064859e41d68596f486c5d724401cb370f260409"
SWIFT_SYNTAX_COMMIT = "60e8eb850721b5a6eebbd973b39f450a16553bd9"
SWIFT_VERSION = "Swift version 6.3.3"

REPOSITORY = Path(__file__).resolve().parents[3]
OUTPUT_ROOT = REPOSITORY / "target" / "reference-sources" / "swift"

MARKERS = ("0️⃣", "1️⃣", "2️⃣", "3️⃣", "4️⃣", "5️⃣", "6️⃣", "7️⃣", "8️⃣", "9️⃣", "🔟", "ℹ️")


@dataclass(frozen=True)
class Group:
    label: str
    root: str
    count: int
    size: int
    digest: str


@dataclass(frozen=True)
class Upstream:
    name: str
    commit: str
    selected_roots: tuple[str, ...]
    groups: tuple[Group, ...]

    @property
    def archive_root(self) -> str:
        return f"{self.name}-{self.commit}"

    @property
    def archive_url(self) -> str:
        return f"https://codeload.github.com/swiftlang/{self.name}/tar.gz/{self.commit}"

    @property
    def destination(self) -> Path:
        return OUTPUT_ROOT / self.archive_root


SWIFT = Upstream(
    name="swift",
    commit=SWIFT_COMMIT,
    selected_roots=("stdlib/public",),
    groups=(
        Group(
            label="Swift stdlib/public",
            root="stdlib/public",
            count=399,
            size=6_080_507,
            digest="76ecd944d8d3517e269e20583b23bc0c86db4deec5a69d4856fc4ffa8f5401c1",
        ),
    ),
)

SWIFT_SYNTAX = Upstream(
    name="swift-syntax",
    commit=SWIFT_SYNTAX_COMMIT,
    selected_roots=(
        "Sources",
        "Tests/SwiftParserTest",
        "Tests/SwiftParserDiagnosticsTest",
    ),
    groups=(
        Group(
            label="SwiftSyntax Sources",
            root="Sources",
            count=318,
            size=6_673_159,
            digest="6870a19941fc733296246be33ad619ad1fa399bc3f48b7ea11178eaf3af71a77",
        ),
        Group(
            label="SwiftSyntax parser tests",
            root="Tests",
            count=126,
            size=1_318_319,
            digest="ddf86f2f8773527da1318bdc5dce559eda8b6a080ddf2b3d083dc474a0076db4",
        ),
    ),
)

# Filled from the pinned SwiftSyntax commit after static assertParse extraction.
EXPECTED_CASE_COUNT = 3_301
EXPECTED_STRICT_CASE_COUNT = 2_056
EXPECTED_RECOVERY_CASE_COUNT = 1_245
EXPECTED_CASE_BYTES = 186_014
EXPECTED_CASE_DIGEST = "1a844ce21901eff63aa4f21e8b1aa683e655c40df505e8c8de061c2f50eeaaba"
EXPECTED_MANIFEST_DIGEST = "76f42eb385397ec4aed85e767cfa681113fa6e3ded2e7a463e54b3c4cdb3af11"


@dataclass(frozen=True)
class StringToken:
    end: int
    interpolated: bool


@dataclass(frozen=True)
class ParserCase:
    origin: str
    literal: str
    strict: bool


@dataclass(frozen=True)
class Argument:
    label: str | None
    start: int
    end: int


def digest_swift_files(root: Path) -> tuple[int, int, str]:
    paths = sorted(path for path in root.rglob("*.swift") if path.is_file())
    digest = hashlib.sha256()
    size = 0
    for path in paths:
        data = path.read_bytes()
        relative = path.relative_to(root).as_posix().encode()
        digest.update(relative)
        digest.update(b"\0")
        digest.update(len(data).to_bytes(8, "big"))
        digest.update(data)
        size += len(data)
    return len(paths), size, digest.hexdigest()


def valid_group(root: Path, group: Group) -> bool:
    if not root.is_dir():
        return False
    actual = digest_swift_files(root)
    expected = (group.count, group.size, group.digest)
    return actual == expected


def valid_upstream(upstream: Upstream) -> bool:
    return all(valid_group(upstream.destination / group.root, group) for group in upstream.groups)


def download_archive(upstream: Upstream, replace: bool = False) -> Path:
    archive_directory = OUTPUT_ROOT / "archives"
    archive_directory.mkdir(parents=True, exist_ok=True)
    archive = archive_directory / f"{upstream.archive_root}.tar.gz"
    if archive.is_file() and not replace:
        return archive

    request = urllib.request.Request(
        upstream.archive_url,
        headers={"User-Agent": "rezel-pinned-swift-corpus"},
    )
    partial = archive.with_suffix(".download")
    with urllib.request.urlopen(request) as response, partial.open("wb") as output:
        shutil.copyfileobj(response, output)
    os.replace(partial, archive)
    print(f"downloaded {upstream.name}@{upstream.commit}")
    return archive


def selected_member(upstream: Upstream, member: tarfile.TarInfo) -> PurePosixPath | None:
    if not member.isfile() or not member.name.endswith(".swift"):
        return None
    path = PurePosixPath(member.name)
    if not path.parts or path.parts[0] != upstream.archive_root or ".." in path.parts:
        return None
    relative = PurePosixPath(*path.parts[1:])
    for selected_root in upstream.selected_roots:
        root = PurePosixPath(selected_root)
        if relative == root or root in relative.parents:
            return relative
    return None


def extract_archive(upstream: Upstream, archive_path: Path) -> None:
    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=f".{upstream.name}-", dir=OUTPUT_ROOT) as temporary:
        staging = Path(temporary) / upstream.archive_root
        with tarfile.open(archive_path) as archive:
            for member in archive:
                relative = selected_member(upstream, member)
                if relative is None:
                    continue
                source = archive.extractfile(member)
                if source is None:
                    raise RuntimeError(f"archive member has no data: {member.name}")
                destination = staging.joinpath(*relative.parts)
                destination.parent.mkdir(parents=True, exist_ok=True)
                with destination.open("wb") as output:
                    shutil.copyfileobj(source, output)

        failures = []
        for group in upstream.groups:
            root = staging / group.root
            actual = digest_swift_files(root)
            expected = (group.count, group.size, group.digest)
            if actual != expected:
                failures.append(f"{group.label}: expected {expected}, found {actual}")
        if failures:
            raise RuntimeError("pinned corpus content changed:\n" + "\n".join(failures))

        stamp = {
            "repository": f"https://github.com/swiftlang/{upstream.name}",
            "commit": upstream.commit,
            "groups": [group.__dict__ for group in upstream.groups],
        }
        (staging / ".rezel-source.json").write_text(
            json.dumps(stamp, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        if upstream.destination.exists():
            shutil.rmtree(upstream.destination)
        os.replace(staging, upstream.destination)


def ensure_upstream(upstream: Upstream) -> None:
    if valid_upstream(upstream):
        print(f"using cached {upstream.name}@{upstream.commit}")
        return

    archive = download_archive(upstream)
    try:
        extract_archive(upstream, archive)
    except (tarfile.TarError, EOFError):
        archive = download_archive(upstream, replace=True)
        extract_archive(upstream, archive)


def string_start(text: str, position: int) -> tuple[int, int] | None:
    cursor = position
    while cursor < len(text) and text[cursor] == "#":
        cursor += 1
    hashes = cursor - position
    if cursor >= len(text) or text[cursor] != '"':
        return None
    quotes = 3 if text.startswith('"""', cursor) else 1
    return hashes, quotes


def skip_line_comment(text: str, position: int) -> int:
    newline = text.find("\n", position + 2)
    return len(text) if newline < 0 else newline + 1


def skip_block_comment(text: str, position: int) -> int:
    cursor = position + 2
    depth = 1
    while cursor < len(text):
        if text.startswith("/*", cursor):
            depth += 1
            cursor += 2
        elif text.startswith("*/", cursor):
            depth -= 1
            cursor += 2
            if depth == 0:
                return cursor
        else:
            cursor += 1
    raise ValueError("unterminated block comment in SwiftSyntax test source")


def skip_interpolation(text: str, position: int) -> int:
    cursor = position
    depth = 1
    while cursor < len(text):
        if text.startswith("//", cursor):
            cursor = skip_line_comment(text, cursor)
            continue
        if text.startswith("/*", cursor):
            cursor = skip_block_comment(text, cursor)
            continue
        if string_start(text, cursor) is not None:
            cursor = scan_string(text, cursor).end
            continue
        if text[cursor] == "(":
            depth += 1
        elif text[cursor] == ")":
            depth -= 1
            if depth == 0:
                return cursor + 1
        cursor += 1
    raise ValueError("unterminated string interpolation in SwiftSyntax test source")


def scan_string(text: str, position: int) -> StringToken:
    start = string_start(text, position)
    if start is None:
        raise ValueError("expected Swift string literal")
    hashes, quotes = start
    cursor = position + hashes + quotes
    closing = '"' * quotes + "#" * hashes
    escape = "\\" + "#" * hashes
    interpolated = False

    while cursor < len(text):
        if text.startswith(closing, cursor):
            return StringToken(cursor + len(closing), interpolated)
        if text.startswith(escape, cursor):
            escaped = cursor + len(escape)
            if escaped < len(text) and text[escaped] == "(":
                interpolated = True
                cursor = skip_interpolation(text, escaped + 1)
                continue
            cursor = min(escaped + 1, len(text))
            continue
        cursor += 1
    raise ValueError("unterminated Swift string literal in SwiftSyntax test source")


def skip_trivia(text: str, position: int) -> int:
    cursor = position
    while cursor < len(text):
        if text[cursor].isspace():
            cursor += 1
        elif text.startswith("//", cursor):
            cursor = skip_line_comment(text, cursor)
        elif text.startswith("/*", cursor):
            cursor = skip_block_comment(text, cursor)
        else:
            return cursor
    return cursor


def identifier_at(text: str, position: int) -> tuple[str, int] | None:
    if position >= len(text) or not (text[position].isalpha() or text[position] == "_"):
        return None
    cursor = position + 1
    while cursor < len(text) and (text[cursor].isalnum() or text[cursor] == "_"):
        cursor += 1
    return text[position:cursor], cursor


def scan_argument(text: str, position: int) -> tuple[int, str]:
    cursor = position
    parens = 0
    brackets = 0
    braces = 0
    while cursor < len(text):
        if text.startswith("//", cursor):
            cursor = skip_line_comment(text, cursor)
            continue
        if text.startswith("/*", cursor):
            cursor = skip_block_comment(text, cursor)
            continue
        if string_start(text, cursor) is not None:
            cursor = scan_string(text, cursor).end
            continue

        character = text[cursor]
        if character == "(":
            parens += 1
        elif character == ")":
            if parens == 0 and brackets == 0 and braces == 0:
                return cursor, ")"
            parens -= 1
        elif character == "[":
            brackets += 1
        elif character == "]":
            brackets -= 1
        elif character == "{":
            braces += 1
        elif character == "}":
            braces -= 1
        elif character == "," and parens == 0 and brackets == 0 and braces == 0:
            return cursor, ","
        cursor += 1
    raise ValueError("unterminated assertParse call in SwiftSyntax test source")


def argument_label(text: str, start: int, end: int) -> str | None:
    cursor = skip_trivia(text, start)
    identifier = identifier_at(text, cursor)
    if identifier is None:
        return None
    label, cursor = identifier
    cursor = skip_trivia(text, cursor)
    return label if cursor < end and text[cursor] == ":" else None


def parse_call_tail(text: str, position: int) -> tuple[int, list[Argument], bool]:
    cursor = skip_trivia(text, position)
    if cursor >= len(text):
        raise ValueError("unterminated assertParse call in SwiftSyntax test source")
    if text[cursor] == ")":
        return cursor + 1, [], True
    if text[cursor] != ",":
        end, delimiter = scan_argument(text, cursor)
        while delimiter != ")":
            end, delimiter = scan_argument(text, end + 1)
        return end + 1, [], False

    arguments = []
    cursor += 1
    while True:
        cursor = skip_trivia(text, cursor)
        if cursor < len(text) and text[cursor] == ")":
            return cursor + 1, arguments, True
        end, delimiter = scan_argument(text, cursor)
        arguments.append(Argument(argument_label(text, cursor, end), cursor, end))
        if delimiter == ")":
            return end + 1, arguments, True
        cursor = end + 1


def matching_call_end(text: str, position: int) -> int:
    end, delimiter = scan_argument(text, position)
    while delimiter != ")":
        end, delimiter = scan_argument(text, end + 1)
    return end + 1


def diagnostics_are_empty(text: str, argument: Argument) -> bool:
    cursor = skip_trivia(text, argument.start)
    identifier = identifier_at(text, cursor)
    if identifier is None:
        return False
    _, cursor = identifier
    cursor = skip_trivia(text, cursor)
    if cursor >= argument.end or text[cursor] != ":":
        return False
    cursor = skip_trivia(text, cursor + 1)
    if cursor >= argument.end or text[cursor] != "[":
        return False
    cursor = skip_trivia(text, cursor + 1)
    if cursor >= argument.end or text[cursor] != "]":
        return False
    return skip_trivia(text, cursor + 1) == argument.end


def parser_cases(test_root: Path) -> tuple[list[ParserCase], dict[str, int]]:
    cases: list[ParserCase] = []
    inventory = {
        "calls": 0,
        "dynamic": 0,
        "interpolated": 0,
        "custom_entry": 0,
        "compound_literal": 0,
    }
    roots = (
        test_root / "SwiftParserTest",
        test_root / "SwiftParserDiagnosticsTest",
    )
    paths = sorted(path for root in roots for path in root.rglob("*.swift"))
    for path in paths:
        if path.name == "Assertions.swift":
            continue
        text = path.read_text(encoding="utf-8")
        newlines = [index for index, character in enumerate(text) if character == "\n"]
        cursor = 0
        while cursor < len(text):
            if text.startswith("//", cursor):
                cursor = skip_line_comment(text, cursor)
                continue
            if text.startswith("/*", cursor):
                cursor = skip_block_comment(text, cursor)
                continue
            if string_start(text, cursor) is not None:
                cursor = scan_string(text, cursor).end
                continue

            identifier = identifier_at(text, cursor)
            if identifier is None:
                cursor += 1
                continue
            call_start = cursor
            name, identifier_end = identifier
            cursor = identifier_end
            if name != "assertParse":
                continue
            open_paren = skip_trivia(text, identifier_end)
            if open_paren >= len(text) or text[open_paren] != "(":
                continue

            inventory["calls"] += 1
            argument_start = skip_trivia(text, open_paren + 1)
            if string_start(text, argument_start) is None:
                inventory["dynamic"] += 1
                cursor = matching_call_end(text, argument_start)
                continue

            token = scan_string(text, argument_start)
            call_end, arguments, literal_is_complete = parse_call_tail(text, token.end)
            cursor = call_end
            if not literal_is_complete:
                inventory["compound_literal"] += 1
                continue
            if arguments and arguments[0].label is None:
                inventory["custom_entry"] += 1
                continue
            if token.interpolated:
                inventory["interpolated"] += 1
                continue

            diagnostics = next(
                (argument for argument in arguments if argument.label == "diagnostics"),
                None,
            )
            strict = diagnostics is None or diagnostics_are_empty(text, diagnostics)
            line = bisect.bisect_left(newlines, call_start) + 1
            relative = path.relative_to(test_root).as_posix()
            cases.append(
                ParserCase(
                    origin=f"{relative}:{line}",
                    literal=text[argument_start:token.end],
                    strict=strict,
                )
            )
    return cases, inventory


def swift_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def generated_case_program(cases: list[ParserCase]) -> str:
    entries = []
    for index, case in enumerate(cases):
        filename = f"{index:05}.swift"
        strict = "true" if case.strict else "false"
        entries.append(
            "Case("
            f"filename: {swift_string(filename)}, "
            f"origin: {swift_string(case.origin)}, "
            f"strict: {strict}, "
            f"source: {case.literal}"
            "),"
        )
    marker_values = ", ".join(swift_string(marker) for marker in MARKERS)
    return f"""\
struct Case {{
    let filename: String
    let origin: String
    let strict: Bool
    let source: String
}}

let cases = [
{chr(10).join(entries)}
]
let markers = Set<Character>([{marker_values}])
let hexDigits = Array("0123456789abcdef".utf8)

func hexEncoded(_ source: String) -> String {{
    var output = [UInt8]()
    output.reserveCapacity(source.utf8.count * 2)
    for byte in source.utf8 {{
        output.append(hexDigits[Int(byte >> 4)])
        output.append(hexDigits[Int(byte & 0x0f)])
    }}
    return String(decoding: output, as: UTF8.self)
}}

for testCase in cases {{
    let source = String(testCase.source.filter {{ !markers.contains($0) }})
    let mode = testCase.strict ? "strict" : "recovery"
    print("\\(testCase.filename)\\t\\(mode)\\t\\(testCase.origin)\\t\\(hexEncoded(source))")
}}
"""


def validate_swift_toolchain() -> None:
    result = subprocess.run(
        ["swiftc", "--version"],
        check=True,
        capture_output=True,
        text=True,
    )
    if SWIFT_VERSION not in result.stdout:
        raise RuntimeError(f"expected {SWIFT_VERSION!r}, found:\n{result.stdout}")


def macos_sdk() -> Path:
    candidates = []
    if sdk_root := os.environ.get("SDKROOT"):
        candidates.append(Path(sdk_root))
    candidates.extend(
        (
            Path("/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk"),
            Path("/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"),
        )
    )
    for candidate in candidates:
        if candidate.is_dir():
            return candidate
    raise RuntimeError("a macOS SDK is required by the mise-pinned Swift toolchain")


def valid_materialized_cases(destination: Path) -> bool:
    manifest = destination / "manifest.tsv"
    stamp_path = destination / ".rezel-cases.json"
    if not manifest.is_file() or not stamp_path.is_file():
        return False
    try:
        stamp = json.loads(stamp_path.read_text(encoding="utf-8"))
        records = [line.split("\t") for line in manifest.read_text(encoding="utf-8").splitlines()]
    except (json.JSONDecodeError, OSError):
        return False
    if any(len(record) != 4 for record in records):
        return False

    strict_count = sum(record[1] == "strict" for record in records)
    recovery_count = sum(record[1] == "recovery" for record in records)
    if strict_count + recovery_count != len(records):
        return False
    count, size, digest = digest_swift_files(destination)
    expected = (
        EXPECTED_CASE_COUNT,
        EXPECTED_STRICT_CASE_COUNT,
        EXPECTED_RECOVERY_CASE_COUNT,
        EXPECTED_CASE_BYTES,
        EXPECTED_CASE_DIGEST,
    )
    actual = (count, strict_count, recovery_count, size, digest)
    manifest_digest = hashlib.sha256(manifest.read_bytes()).hexdigest()
    return (
        actual == expected
        and manifest_digest == EXPECTED_MANIFEST_DIGEST
        and stamp.get("swift_syntax_commit") == SWIFT_SYNTAX_COMMIT
        and stamp.get("source_test_digest") == SWIFT_SYNTAX.groups[1].digest
    )


def materialize_cases() -> None:
    destination = OUTPUT_ROOT / f"swift-syntax-cases-{SWIFT_SYNTAX_COMMIT}"
    if valid_materialized_cases(destination):
        print(f"using cached SwiftSyntax parser cases@{SWIFT_SYNTAX_COMMIT}")
        return

    tests = SWIFT_SYNTAX.destination / "Tests"
    cases, extraction_inventory = parser_cases(tests)
    strict_count = sum(case.strict for case in cases)
    recovery_count = len(cases) - strict_count

    if len(cases) != EXPECTED_CASE_COUNT:
        raise RuntimeError(
            "SwiftSyntax assertParse inventory changed: "
            f"expected {EXPECTED_CASE_COUNT}, found {len(cases)}; "
            f"strict={strict_count}, recovery={recovery_count}, extraction={extraction_inventory}"
        )

    validate_swift_toolchain()
    with tempfile.TemporaryDirectory(prefix=".swift-syntax-cases-", dir=OUTPUT_ROOT) as temporary:
        temporary_root = Path(temporary)
        staging = temporary_root / destination.name
        staging.mkdir()
        generated = temporary_root / "Materialize.swift"
        generated.write_text(generated_case_program(cases), encoding="utf-8")
        executable = temporary_root / "materialize"
        module_cache = temporary_root / "module-cache"
        subprocess.run(
            [
                "swiftc",
                str(generated),
                "-module-cache-path",
                str(module_cache),
                "-sdk",
                str(macos_sdk()),
                "-o",
                str(executable),
            ],
            check=True,
        )
        result = subprocess.run([str(executable)], check=True, capture_output=True, text=True)
        manifest = []
        for line in result.stdout.splitlines():
            filename, mode, origin, encoded = line.split("\t")
            data = bytes.fromhex(encoded)
            (staging / filename).write_bytes(data)
            manifest.append(f"{filename}\t{mode}\t{origin}\t{len(data)}")
        (staging / "manifest.tsv").write_text("\n".join(manifest) + "\n", encoding="utf-8")

        count, size, digest = digest_swift_files(staging)
        actual = (count, strict_count, recovery_count, size, digest)
        expected = (
            EXPECTED_CASE_COUNT,
            EXPECTED_STRICT_CASE_COUNT,
            EXPECTED_RECOVERY_CASE_COUNT,
            EXPECTED_CASE_BYTES,
            EXPECTED_CASE_DIGEST,
        )
        if actual != expected:
            raise RuntimeError(f"materialized SwiftSyntax case inventory changed: expected {expected}, found {actual}")

        stamp = {
            "swift_syntax_commit": SWIFT_SYNTAX_COMMIT,
            "source_test_digest": SWIFT_SYNTAX.groups[1].digest,
            "extraction": extraction_inventory,
            "cases": {
                "count": count,
                "strict": strict_count,
                "recovery": recovery_count,
                "bytes": size,
                "digest": digest,
            },
        }
        (staging / ".rezel-cases.json").write_text(
            json.dumps(stamp, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        if destination.exists():
            shutil.rmtree(destination)
        os.replace(staging, destination)
    print(
        f"materialized {len(cases)} SwiftSyntax parser cases "
        f"({strict_count} strict, {recovery_count} recovery)"
    )


def main() -> None:
    ensure_upstream(SWIFT)
    ensure_upstream(SWIFT_SYNTAX)
    materialize_cases()


if __name__ == "__main__":
    main()
