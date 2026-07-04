#!/usr/bin/env python3
"""Check curated syntax fixtures with the mise-pinned Swift parser."""

from __future__ import annotations

import subprocess
from pathlib import Path


EXPECTED_VERSION = "Swift version 6.3.3"
REPOSITORY = Path(__file__).resolve().parents[3]
FIXTURES = REPOSITORY / "languages" / "swift" / "tests" / "fixtures"


def swift_version() -> str:
    result = subprocess.run(
        ["swiftc", "--version"],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def accepts(path: Path) -> tuple[bool, str]:
    result = subprocess.run(
        ["swiftc", "-frontend", "-parse", str(path)],
        check=False,
        capture_output=True,
        text=True,
    )
    diagnostics = "\n".join(
        part.strip() for part in (result.stdout, result.stderr) if part.strip()
    )
    return result.returncode == 0, diagnostics


def main() -> None:
    version = swift_version()
    if EXPECTED_VERSION not in version:
        raise SystemExit(
            f"expected {EXPECTED_VERSION!r}; run this check through "
            f"`mise run reference:swift`\nactual: {version}"
        )

    failures: list[str] = []
    checked = 0
    for directory, expected in (("accepted", True), ("rejected", False)):
        paths = sorted((FIXTURES / directory).glob("*.swift"))
        if not paths:
            failures.append(f"no {directory} Swift fixtures")
            continue
        for path in paths:
            checked += 1
            actual, diagnostics = accepts(path)
            if actual == expected:
                continue
            expectation = "accept" if expected else "reject"
            failures.append(
                f"{path.relative_to(REPOSITORY)}: expected Swift to {expectation}"
                + (f"\n{diagnostics}" if diagnostics else "")
            )

    if failures:
        raise SystemExit("\n\n".join(failures))
    print(f"Swift 6.3.3 parser oracle checked {checked} fixtures")


if __name__ == "__main__":
    main()
