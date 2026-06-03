#!/usr/bin/env python3
"""Generate deterministic CPython 3.14 parse and public AST snapshots."""

from __future__ import annotations

import argparse
import ast
import json
import os
import struct
import sys
import unicodedata
import warnings
from pathlib import Path
from typing import Any


EXPECTED_PYTHON = (3, 14, 5)
EXPECTED_UNICODE = "16.0.0"
CASE_SCHEMA = "rezel.cpython-python-reference-cases.v1"
SNAPSHOT_SCHEMA = "rezel.cpython-python-reference-snapshot.v1"
STDLIB_FINGERPRINT_SCHEMA = "rezel.cpython-stdlib-ast-fingerprints.v1"
FNV_PRIME = 0x0000_0100_0000_01B3
U64_MASK = (1 << 64) - 1
TOOL_DIR = Path(__file__).resolve().parent
CASES_PATH = TOOL_DIR / "cases" / "python.json"
SNAPSHOT_PATH = TOOL_DIR / "snapshots" / "python.json"


def ast_schema() -> list[dict[str, Any]]:
    classes: list[dict[str, Any]] = []
    for name in sorted(vars(ast)):
        candidate = getattr(ast, name)
        if not isinstance(candidate, type) or not issubclass(candidate, ast.AST):
            continue
        field_types = getattr(candidate, "_field_types", {})
        classes.append(
            {
                "name": name,
                "base": candidate.__base__.__name__,
                "fields": list(candidate._fields),
                "fieldTypes": {
                    field: type_name(field_types[field])
                    for field in candidate._fields
                    if field in field_types
                },
            }
        )
    return classes


def type_name(value: Any) -> str:
    return str(value).replace("<class '", "").replace("'>", "")


def line_byte_offsets(source: str) -> list[int]:
    offsets = [0]
    total = 0
    for line in source.splitlines(keepends=True):
        total += len(line.encode("utf-8"))
        offsets.append(total)
    return offsets


def position(node: ast.AST, source: str, line_offsets: list[int], prefix: str) -> int | None:
    line = getattr(node, f"{prefix}lineno", None)
    column = getattr(node, f"{prefix}col_offset", None)
    if line is None or column is None:
        return None
    if line < 1 or line > len(line_offsets):
        raise AssertionError(f"invalid AST line {line}")
    return line_offsets[line - 1] + column


def project(node: ast.AST, source: str) -> dict[str, Any]:
    offsets = line_byte_offsets(source)

    def visit(value: Any) -> Any:
        if isinstance(value, ast.AST):
            is_constant = isinstance(value, ast.Constant)
            return {
                "kind": type(value).__name__,
                "start": position(value, source, offsets, ""),
                "end": position(value, source, offsets, "end_"),
                "fields": [
                    {
                        "field": field,
                        "value": (
                            {
                                "stringCodePoints": [
                                    ord(character)
                                    for character in getattr(value, field)
                                ]
                            }
                            if is_constant
                            and field == "value"
                            and isinstance(getattr(value, field), str)
                            else visit(getattr(value, field))
                        ),
                    }
                    for field in value._fields
                ],
            }
        if isinstance(value, list):
            return [visit(item) for item in value]
        if value is None or isinstance(value, (str, bool)):
            return value
        if isinstance(value, int):
            return {"integer": str(value)}
        if isinstance(value, float):
            bits = struct.unpack(">Q", struct.pack(">d", value))[0]
            return {"floatBits": bits}
        if isinstance(value, complex):
            return {
                "complexBits": {
                    "real": visit(float(value.real))["floatBits"],
                    "imaginary": visit(float(value.imag))["floatBits"],
                }
            }
        if isinstance(value, bytes):
            return {"bytes": list(value)}
        if value is Ellipsis:
            return {"ellipsis": True}
        raise TypeError(f"unsupported AST value {value!r}")

    return visit(node)


class AstFingerprint:
    def __init__(self) -> None:
        self.state = 0
        self.nodes = 0

    def add_root(self, node: dict[str, Any]) -> None:
        self._add_node(node)

    def hexadecimal(self) -> str:
        return f"{self.state:016x}"

    def _add_node(self, node: dict[str, Any]) -> None:
        self.nodes += 1
        self._add_string(node["kind"])
        self._add_optional_integer(node["start"])
        self._add_optional_integer(node["end"])
        fields = node["fields"]
        self._add_long(len(fields))
        for field in fields:
            self._add_string(field["field"])
            self._add_value(field["value"])

    def _add_value(self, value: Any) -> None:
        if value is None:
            self._add_long(0)
            return
        if isinstance(value, str):
            self._add_long(1)
            self._add_string(value)
            return
        if isinstance(value, bool):
            self._add_long(2)
            self._add_long(int(value))
            return
        if isinstance(value, list):
            self._add_long(9)
            self._add_long(len(value))
            for item in value:
                self._add_value(item)
            return
        if not isinstance(value, dict):
            raise TypeError(f"unsupported normalized AST value {value!r}")
        if {"kind", "start", "end", "fields"} == value.keys():
            self._add_long(8)
            self._add_node(value)
            return
        if value.keys() == {"integer"}:
            self._add_long(3)
            self._add_string(value["integer"])
            return
        if value.keys() == {"floatBits"}:
            self._add_long(4)
            self._add_long(value["floatBits"])
            return
        if value.keys() == {"complexBits"}:
            self._add_long(5)
            bits = value["complexBits"]
            self._add_long(bits["real"])
            self._add_long(bits["imaginary"])
            return
        if value.keys() == {"bytes"}:
            self._add_long(6)
            raw = value["bytes"]
            self._add_long(len(raw))
            for byte in raw:
                self._add_long(byte)
            return
        if value.keys() == {"ellipsis"}:
            self._add_long(7)
            return
        if value.keys() == {"stringCodePoints"}:
            self._add_long(10)
            code_points = value["stringCodePoints"]
            self._add_long(len(code_points))
            for code_point in code_points:
                self._add_long(code_point)
            return
        raise TypeError(f"unsupported normalized AST object {value!r}")

    def _add_optional_integer(self, value: int | None) -> None:
        if value is None:
            self._add_long(0)
        else:
            self._add_long(1)
            self._add_long(value)

    def _add_string(self, value: str) -> None:
        encoded = value.encode("utf-8")
        self._add_long(len(encoded))
        for byte in encoded:
            self._add_long(byte)

    def _add_long(self, value: int) -> None:
        self.state = ((self.state ^ value) * FNV_PRIME) & U64_MASK


def stdlib_sources(root: Path) -> list[Path]:
    sources: list[Path] = []
    for directory, directories, files in os.walk(root):
        directories[:] = sorted(
            name
            for name in directories
            if name not in {"site-packages", "__pycache__"}
        )
        for name in sorted(files):
            if name.endswith(".py"):
                sources.append(Path(directory) / name)
    return sorted(sources)


def write_stdlib_fingerprints(root: Path, output: Path) -> None:
    root = root.resolve(strict=True)
    sources = stdlib_sources(root)
    records: list[dict[str, Any]] = []
    for index, path in enumerate(sources, start=1):
        source = path.read_text(encoding="utf-8")
        relative = path.relative_to(root).as_posix()
        record: dict[str, Any] = {
            "path": relative,
            "accepted": False,
            "errorKind": None,
            "errorLine": None,
            "errorOffset": None,
            "nodes": 0,
            "fingerprint": None,
        }
        try:
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", SyntaxWarning)
                tree = ast.parse(
                    source,
                    filename=relative,
                    mode="exec",
                    type_comments=False,
                )
        except SyntaxError as error:
            record["errorKind"] = type(error).__name__
            record["errorLine"] = error.lineno
            record["errorOffset"] = error.offset
        else:
            projected = project(tree, source)
            fingerprint = AstFingerprint()
            fingerprint.add_root(projected)
            record["accepted"] = True
            record["nodes"] = fingerprint.nodes
            record["fingerprint"] = fingerprint.hexadecimal()
        records.append(record)
        if index % 100 == 0:
            print(
                f"CPython AST fingerprints: {index}/{len(sources)}",
                file=sys.stderr,
            )

    oracle = {
        "schema": STDLIB_FINGERPRINT_SCHEMA,
        "pythonVersion": ".".join(map(str, EXPECTED_PYTHON)),
        "unicodeVersion": EXPECTED_UNICODE,
        "coordinates": "raw-utf8-bytes",
        "mode": "exec",
        "typeComments": False,
        "sources": records,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(oracle, ensure_ascii=False, indent="\t") + "\n",
        encoding="utf-8",
    )


def write_ast_projection(source_path: Path, filename: str, output: Path) -> None:
    source = source_path.read_text(encoding="utf-8")
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", SyntaxWarning)
        tree = ast.parse(
            source,
            filename=filename,
            mode="exec",
            type_comments=False,
        )
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(project(tree, source), ensure_ascii=False, indent="\t") + "\n",
        encoding="utf-8",
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--update", action="store_true")
    mode.add_argument(
        "--stdlib-fingerprints",
        nargs=2,
        metavar=("ROOT", "OUTPUT"),
    )
    mode.add_argument(
        "--project-file",
        nargs=3,
        metavar=("SOURCE", "FILENAME", "OUTPUT"),
    )
    args = parser.parse_args()

    if sys.version_info[:3] != EXPECTED_PYTHON:
        raise SystemExit(f"expected CPython {EXPECTED_PYTHON}, got {sys.version_info[:3]}")
    if unicodedata.unidata_version != EXPECTED_UNICODE:
        raise SystemExit(
            f"expected Unicode {EXPECTED_UNICODE}, got {unicodedata.unidata_version}"
        )
    if args.stdlib_fingerprints is not None:
        root, output = map(Path, args.stdlib_fingerprints)
        write_stdlib_fingerprints(root, output)
        return
    if args.project_file is not None:
        source, filename, output = args.project_file
        write_ast_projection(Path(source), filename, Path(output))
        return

    manifest = json.loads(CASES_PATH.read_text(encoding="utf-8"))
    if manifest["schema"] != CASE_SCHEMA:
        raise SystemExit("unexpected Python reference case schema")
    seen: set[str] = set()
    accepted: list[dict[str, Any]] = []
    rejected: list[dict[str, Any]] = []
    extensions: list[dict[str, Any]] = []
    for case in manifest["cases"]:
        if case["id"] in seen:
            raise SystemExit(f"duplicate case {case['id']}")
        seen.add(case["id"])
        try:
            # The snapshot contract records parse acceptance and the public AST,
            # not version-dependent warning policy for still-accepted escapes.
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", SyntaxWarning)
                tree = ast.parse(
                    case["source"],
                    filename=f"<{case['id']}>",
                    mode=case["mode"],
                    type_comments=case["typeComments"],
                )
        except SyntaxError as error:
            if case["policy"] == "accepted":
                raise
            target = rejected if case["policy"] == "rejected" else extensions
            target.append(
                {
                    **case,
                    "cpythonError": {
                        "kind": type(error).__name__,
                        "line": error.lineno,
                        "offset": error.offset,
                    },
                }
            )
        else:
            if case["policy"] != "accepted":
                raise SystemExit(
                    f"CPython accepted {case['id']} classified as {case['policy']}"
                )
            accepted.append({**case, "ast": project(tree, case["source"])})

    snapshot = {
        "schema": SNAPSHOT_SCHEMA,
        "pythonVersion": ".".join(map(str, EXPECTED_PYTHON)),
        "unicodeVersion": EXPECTED_UNICODE,
        "coordinates": "raw-utf8-bytes",
        "astSchema": ast_schema(),
        "accepted": accepted,
        "rejected": rejected,
        "extensions": extensions,
    }
    encoded = json.dumps(snapshot, ensure_ascii=False, indent="\t") + "\n"
    if args.update:
        SNAPSHOT_PATH.parent.mkdir(parents=True, exist_ok=True)
        SNAPSHOT_PATH.write_text(encoded, encoding="utf-8")
    elif SNAPSHOT_PATH.read_text(encoding="utf-8") != encoded:
        raise SystemExit("CPython snapshot is stale; run reference:cpython:update")


if __name__ == "__main__":
    main()
