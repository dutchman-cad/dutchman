#!/usr/bin/env python3
"""Fail when program code writes, renames or deletes files outside `dutchman_core::safe_output`.

Every file a user keeps (a design, a report, a log) is written by `safe_output`: atomic, never
replacing a file this run did not create, numbered on a name clash. This check keeps direct
`std::fs::write`, `File::create`, `OpenOptions`, `fs::rename`, `fs::copy`, `remove_file` and
`remove_dir_all` from coming back into program code.

Test code (everything from the first `#[cfg(test)]` in a file, `tests/` folders, `tests.rs`,
`*_tests.rs`, examples) is not checked. Program files that still call these directly are listed
in scripts/safe-output-allowlist.txt with the number of calls and why; the count may only go down.

    python3 scripts/check-safe-output.py            # check
    python3 scripts/check-safe-output.py --counts   # print the counts per file
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
ALLOWLIST = ROOT / "scripts" / "safe-output-allowlist.txt"
PATTERN = re.compile(
    r"\b(?:fs::(?:write|rename|copy|remove_file|remove_dir_all|hard_link)"
    r"|File::create(?:_new)?|OpenOptions::new|\.truncate\(true\)|\.persist(?:_noclobber)?)\s*\(?"
)
EXEMPT = ("crates/dutchman-core/src/safe_output.rs",)


def checked(path: pathlib.Path) -> bool:
    rel = path.relative_to(ROOT).as_posix()
    parts = rel.split("/")
    if rel in EXEMPT or "tests" in parts or "examples" in parts or "benches" in parts:
        return False
    return not (path.name in ("tests.rs", "test_support.rs") or path.name.endswith("_tests.rs"))


def count(path: pathlib.Path) -> int:
    total = 0
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip().startswith("#[cfg(test)]"):
            break
        code = line.split("//", 1)[0]
        total += len(PATTERN.findall(code))
    return total


def counts() -> dict:
    found = {}
    for path in sorted((ROOT / "crates").glob("*/src/**/*.rs")):
        if checked(path):
            n = count(path)
            if n:
                found[path.relative_to(ROOT).as_posix()] = n
    return found


def allowed() -> dict:
    result = {}
    for line in ALLOWLIST.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        name, n, _reason = line.split("\t", 2)
        result[name] = int(n)
    return result


def main() -> int:
    found = counts()
    if "--counts" in sys.argv:
        for name, n in found.items():
            print(f"{name}\t{n}")
        return 0
    allow = allowed()
    bad = [
        f"{name}: {n} direct file write/rename/delete call(s), {allow.get(name, 0)} allowed"
        for name, n in found.items()
        if n > allow.get(name, 0)
    ]
    for name, n in allow.items():
        if found.get(name, 0) < n:
            print(f"note: lower the allowance for {name} to {found.get(name, 0)}")
    if bad:
        print("\n".join(bad))
        print(
            "\nWrite user-visible files through dutchman_core::safe_output (atomic, never replacing a file"
            "\nthis run did not create). A file Dutchman owns outright (a cache, a temporary folder, an"
            "\ninstaller download) may be listed in scripts/safe-output-allowlist.txt, with the reason."
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
