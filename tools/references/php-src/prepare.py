#!/usr/bin/env python3
"""Fetch and validate the pinned php-src Zend parser corpus."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import tarfile
import tempfile
import urllib.request
from pathlib import Path, PurePosixPath


PHP_SRC_COMMIT = "dd6e76cce27aaa0ed9f7520648ed1081dfb6af36"
PHP_VERSION = "8.5.9"
EXPECTED_FILES = 5_308
EXPECTED_BYTES = 3_474_668
EXPECTED_DIGEST = "3a1b988ac956a2763540d505a603362e7711c250e2e894811b083d11dbb876f8"

REPOSITORY = Path(__file__).resolve().parents[3]
OUTPUT_ROOT = REPOSITORY / "target" / "reference-sources" / "php-src"
ARCHIVE_ROOT = f"php-src-{PHP_SRC_COMMIT}"
DESTINATION = OUTPUT_ROOT / ARCHIVE_ROOT
ARCHIVE_URL = f"https://codeload.github.com/php/php-src/tar.gz/{PHP_SRC_COMMIT}"


def digest_tests(root: Path) -> tuple[int, int, str]:
    paths = sorted(path for path in root.rglob("*.phpt") if path.is_file())
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


def valid_destination() -> bool:
    tests = DESTINATION / "Zend" / "tests"
    if not tests.is_dir():
        return False
    return digest_tests(tests) == (EXPECTED_FILES, EXPECTED_BYTES, EXPECTED_DIGEST)


def download_archive(replace: bool = False) -> Path:
    archives = OUTPUT_ROOT / "archives"
    archives.mkdir(parents=True, exist_ok=True)
    archive = archives / f"{ARCHIVE_ROOT}.tar.gz"
    if archive.is_file() and not replace:
        return archive

    request = urllib.request.Request(
        ARCHIVE_URL,
        headers={"User-Agent": "rezel-pinned-php-src-corpus"},
    )
    partial = archive.with_suffix(".download")
    with urllib.request.urlopen(request) as response, partial.open("wb") as output:
        shutil.copyfileobj(response, output)
    os.replace(partial, archive)
    print(f"downloaded php-src@{PHP_SRC_COMMIT}")
    return archive


def selected_member(member: tarfile.TarInfo) -> PurePosixPath | None:
    if not member.isfile():
        return None
    path = PurePosixPath(member.name)
    if not path.parts or path.parts[0] != ARCHIVE_ROOT or ".." in path.parts:
        return None
    relative = PurePosixPath(*path.parts[1:])
    if relative == PurePosixPath("LICENSE"):
        return relative
    tests = PurePosixPath("Zend/tests")
    if relative.suffix == ".phpt" and tests in relative.parents:
        return relative
    return None


def extract_archive(archive_path: Path) -> None:
    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".php-src-", dir=OUTPUT_ROOT) as temporary:
        staging = Path(temporary) / ARCHIVE_ROOT
        with tarfile.open(archive_path) as archive:
            for member in archive:
                relative = selected_member(member)
                if relative is None:
                    continue
                source = archive.extractfile(member)
                if source is None:
                    raise RuntimeError(f"archive member has no data: {member.name}")
                destination = staging.joinpath(*relative.parts)
                destination.parent.mkdir(parents=True, exist_ok=True)
                with destination.open("wb") as output:
                    shutil.copyfileobj(source, output)

        actual = digest_tests(staging / "Zend" / "tests")
        expected = (EXPECTED_FILES, EXPECTED_BYTES, EXPECTED_DIGEST)
        if actual != expected:
            raise RuntimeError(f"pinned php-src corpus changed: expected {expected}, found {actual}")

        stamp = {
            "repository": "https://github.com/php/php-src",
            "commit": PHP_SRC_COMMIT,
            "version": PHP_VERSION,
            "root": "Zend/tests",
            "files": EXPECTED_FILES,
            "bytes": EXPECTED_BYTES,
            "sha256": EXPECTED_DIGEST,
        }
        (staging / ".rezel-source.json").write_text(
            json.dumps(stamp, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        if DESTINATION.exists():
            shutil.rmtree(DESTINATION)
        os.replace(staging, DESTINATION)


def main() -> None:
    if valid_destination():
        print(f"using cached php-src@{PHP_SRC_COMMIT}")
        return
    archive = download_archive()
    try:
        extract_archive(archive)
    except (tarfile.TarError, EOFError):
        archive = download_archive(replace=True)
        extract_archive(archive)
    print(f"prepared PHP {PHP_VERSION} Zend/tests corpus")


if __name__ == "__main__":
    main()
