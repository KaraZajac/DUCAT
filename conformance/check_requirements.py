#!/usr/bin/env python3
"""Every third-party import in conformance/ is declared in requirements.txt.

The second implementation imports its primitives lazily, inside the functions
that need them — which reads well and hides a trap: a dependency nobody
declared is invisible on the machine that happens to have it, and shows up on
a clean one as a *crashed vector*, which looks exactly like a disagreement.
That is the worst possible disguise for a missing package, because it is the
shape of a real finding.

This walks the ASTs (deferred imports included), maps each top-level module to
the distribution that provides it, and fails if the distribution is not named
in requirements.txt. Run it before trusting a green suite:

    python3 conformance/check_requirements.py
"""
import ast
import pathlib
import sys
from importlib import metadata

HERE = pathlib.Path(__file__).resolve().parent


def imported_modules() -> set[str]:
    mods: set[str] = set()
    for path in sorted(HERE.glob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                for alias in node.names:
                    mods.add(alias.name.split(".")[0])
            elif isinstance(node, ast.ImportFrom):
                # level > 0 is a relative import: our own code.
                if node.module and node.level == 0:
                    mods.add(node.module.split(".")[0])
    return mods


def declared() -> set[str]:
    req = HERE / "requirements.txt"
    if not req.is_file():
        return set()
    names = set()
    for line in req.read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        for sep in (">=", "==", "<=", "~=", ">", "<", "["):
            if sep in line:
                line = line.split(sep, 1)[0]
        names.add(line.strip().lower().replace("_", "-"))
    return names


def main() -> int:
    stdlib = set(sys.stdlib_module_names)
    # Which distribution provides each importable top-level module.
    provided = metadata.packages_distributions()
    have = declared()

    undeclared: list[str] = []
    unknown: list[str] = []
    for mod in sorted(imported_modules()):
        if mod in stdlib or mod == "__future__":
            continue
        dists = provided.get(mod)
        if not dists:
            # Not installed here either — cannot tell what would provide it.
            unknown.append(mod)
            continue
        if not any(d.lower().replace("_", "-") in have for d in dists):
            undeclared.append(f"{mod} (from {', '.join(dists)})")

    for mod in unknown:
        print(f"  ? {mod} — not installed here; cannot check what provides it")
    for entry in undeclared:
        print(f"  ! {entry} — imported but not in requirements.txt")

    if undeclared:
        print(f"requirements — {len(undeclared)} undeclared dependency(ies)")
        return 1
    print(f"requirements — every third-party import is declared ({len(have)} named)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
