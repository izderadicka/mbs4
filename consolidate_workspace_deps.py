#!/usr/bin/env python3
"""
consolidate_workspace_deps.py

Consolidate duplicate dependency versions across a Cargo workspace.

- Enumerates members via `cargo metadata` (supports globs, excludes, virtual roots).
- Finds deps that appear in ≥ 2 member crates with the SAME version requirement.
- Moves them to root:
    [workspace.dependencies]          for normal deps
    [workspace.dev-dependencies]      for dev-only deps
- Rewrites member entries to { workspace = true } while preserving:
    features, optional, default-features, package (rename)
- Skips: path/git deps, entries already using workspace=true, target-specific sections,
         build-dependencies (kept local for safety), and version conflicts.
- Won’t overwrite an existing root version unless you pass --force.

Usage:
  python consolidate_workspace_deps.py [--root path/to/Cargo.toml] [--dry-run] [--verbose] [--force]
"""

from __future__ import annotations
import argparse, json, subprocess, sys
from pathlib import Path
from typing import Dict, Tuple, Any, List, DefaultDict
from collections import defaultdict

import tomlkit
from tomlkit import inline_table
from tomlkit.items import Table, InlineTable

Section = str  # "dependencies" | "dev-dependencies" | "build-dependencies"
ROOT_WS_DEPS = "workspace.dependencies"
ROOT_WS_DEVDEPS = "workspace.dev-dependencies"

def run_metadata(root_dir: Path) -> Dict[str, Any]:
    try:
        out = subprocess.check_output(
            ["cargo", "metadata", "--format-version=1", "--no-deps"],
            cwd=root_dir,
            text=True,
        )
    except subprocess.CalledProcessError as e:
        print(e.output, file=sys.stderr)
        raise
    return json.loads(out)

def workspace_member_manifests(root_manifest: Path) -> List[Path]:
    root_dir = root_manifest.parent
    meta = run_metadata(root_dir)
    id_to_manifest = {p["id"]: Path(p["manifest_path"]) for p in meta["packages"]}
    members = [id_to_manifest[mid] for mid in meta["workspace_members"]]
    # Exclude non-existent (shouldn't happen) and the workspace root if it’s virtual
    return [m for m in members if m.exists()]

def load_toml(path: Path) -> tomlkit.TOMLDocument:
    with path.open("r", encoding="utf-8") as f:
        return tomlkit.parse(f.read())

def ensure_table(doc: tomlkit.TOMLDocument | Table, dotted: str) -> Table:
    parts = dotted.split(".")
    cur = doc
    for i, p in enumerate(parts):
        if p not in cur:
            t = tomlkit.table()
            cur[p] = t
            cur = t
        else:
            cur = cur[p]
            if not isinstance(cur, Table):
                raise ValueError(f"{'.'.join(parts[:i+1])} is not a table")
    return cur

def is_simple_registry_dep(item: Any) -> Tuple[bool, str | None, str | None]:
    """
    Returns (is_candidate, version, reason_if_not)

    Candidate if:
      - "1.2"
      - { version = "1.2", ... } and NOT path/git and NOT workspace=true
    """
    if isinstance(item, str):
        return True, item, None
    if isinstance(item, Table):
        if item.get("workspace", False):
            return False, None, "already workspace=true"
        if "path" in item or "git" in item:
            return False, None, "path/git dep"
        ver = item.get("version")
        if isinstance(ver, str):
            return True, ver, None
        return False, None, "no version (maybe inherited/patch?)"
    return False, None, "unsupported item type"

def dep_item_to_workspace_replacement(item):
    it = tomlkit.inline_table()
    it["workspace"] = True
    if isinstance(item, Table):
        for k in ("features", "optional", "default-features", "package"):
            if k in item:
                it[k] = item[k]
    # optional: keep a tidy trailing newline when dumped
    it.trivia.trail = "\n"
    return it

def _to_inline(tbl: Table | InlineTable) -> InlineTable:
    if isinstance(tbl, InlineTable):
        return tbl
    it = inline_table()
    for k, v in tbl.items():
        it[k] = v
    # pretty newline after the inline value
    it.trivia.trail = "\n"
    return it

def set_root_ws_dep(ws_tbl: Table, name: str, ver: str, *, force: bool):
    """
    Ensure ws_tbl[name] is an INLINE table like:
      dep = { version = "x.y.z", features = [...] }
    Preserve existing feature flags; only update version when forced or missing.
    """
    existing = ws_tbl.get(name)

    if existing is None:
        it = inline_table()
        it["version"] = ver
        it.trivia.trail = "\n"
        ws_tbl[name] = it
        return

    if isinstance(existing, str):
        # convert string -> inline table
        it = inline_table()
        it["version"] = ver if force else existing
        it.trivia.trail = "\n"
        ws_tbl[name] = it
        return

    if isinstance(existing, InlineTable):
        if ("version" not in existing) or force:
            existing["version"] = ver
        return

    if isinstance(existing, Table):
        # update version (merge), then convert to inline
        if ("version" not in existing) or force:
            existing["version"] = ver
        ws_tbl[name] = _to_inline(existing)
        return

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", type=Path, default=Path("Cargo.toml"), help="Path to workspace root Cargo.toml")
    ap.add_argument("--dry-run", action="store_true", help="Only print planned changes")
    ap.add_argument("--verbose", "-v", action="store_true", help="Explain skips and show findings")
    ap.add_argument("--force", action="store_true", help="Override existing root versions if they differ")
    args = ap.parse_args()

    root_manifest = args.root.resolve()
    if not root_manifest.exists():
        sys.exit(f"Root manifest not found: {root_manifest}")

    members = workspace_member_manifests(root_manifest)
    if args.verbose:
        print(f"Found {len(members)} members via cargo metadata:")
        for m in members:
            print(f"  - {m}")

    root_doc = load_toml(root_manifest)
    ws_deps_tbl = ensure_table(root_doc, ROOT_WS_DEPS)
    ws_dev_tbl = ensure_table(root_doc, ROOT_WS_DEVDEPS)

    SECTIONS: List[Section] = ["dependencies", "dev-dependencies", "build-dependencies"]

    # dep -> section -> version -> list[(manifest_path, item)]
    occurrences: DefaultDict[str, DefaultDict[Section, DefaultDict[str, List[Tuple[Path, Any]]]]] = defaultdict(
        lambda: defaultdict(lambda: defaultdict(list))
    )
    # For verbosity: reasons why something isn't a candidate per crate
    skip_reasons: DefaultDict[str, List[Tuple[Path, Section, str]]] = defaultdict(list)

    for manifest in members:
        doc = load_toml(manifest)
        for section in SECTIONS:
            tbl = doc.get(section)
            if not isinstance(tbl, Table):
                continue
            for name, item in list(tbl.items()):
                ok, ver, why = is_simple_registry_dep(item)
                if ok:
                    occurrences[name][section][ver].append((manifest, item))
                else:
                    if args.verbose and why:
                        skip_reasons[name].append((manifest, section, why))

    actions = []  # tuples describing what we'll do
    conflicts = []

    # Decide consolidations
    for dep_name, per_section in occurrences.items():
        # Prefer consolidating normal deps; if none, consider dev-only
        normal = per_section.get("dependencies", {})
        if normal:
            for ver, items in normal.items():
                if len(items) >= 2:
                    # Check conflict at root
                    existing = ws_deps_tbl.get(dep_name)
                    if (isinstance(existing, str) and existing != ver) or (isinstance(existing, Table) and existing.get("version") != ver):
                        if args.force:
                            actions.append(("root_set", "dependencies", dep_name, ver))
                        else:
                            conflicts.append((dep_name, "dependencies", existing, ver))
                            continue
                    else:
                        actions.append(("root_set", "dependencies", dep_name, ver))
                    for manifest, item in items:
                        actions.append(("rewrite_member", manifest, "dependencies", dep_name, item))
        else:
            # Dev-only consolidation
            dev = per_section.get("dev-dependencies", {})
            for ver, items in dev.items():
                if len(items) >= 2:
                    existing = ws_dev_tbl.get(dep_name)
                    if (isinstance(existing, str) and existing != ver) or (isinstance(existing, Table) and existing.get("version") != ver):
                        if args.force:
                            actions.append(("root_set", "dev-dependencies", dep_name, ver))
                        else:
                            conflicts.append((dep_name, "dev-dependencies", existing, ver))
                            continue
                    else:
                        actions.append(("root_set", "dev-dependencies", dep_name, ver))
                    for manifest, item in items:
                        actions.append(("rewrite_member", manifest, "dev-dependencies", dep_name, item))

    if args.verbose:
        # Print quick summary of candidates
        print("\nCandidates found:")
        any_cand = False
        for dep_name, per_section in occurrences.items():
            for sec, ver_map in per_section.items():
                for ver, items in ver_map.items():
                    if len(items) >= 2 and sec != "build-dependencies":
                        print(f"  - {dep_name} {ver} ({sec}) in {len(items)} crates")
                        any_cand = True
        if not any_cand:
            print("  (none)")

        # Conflicts that prevent consolidation without --force
        if conflicts:
            print("\nConflicts (root already has different version):")
            for name, sec, existing, found in conflicts:
                print(f"  - {name} [{sec}] root={existing!r} vs found={found!r}")

        # Some common reasons for skips
        if skip_reasons:
            print("\nCommon skip reasons:")
            aggregate = defaultdict(int)
            for _, entries in skip_reasons.items():
                for _mf, _sec, why in entries:
                    aggregate[why] += 1
            for why, count in sorted(aggregate.items(), key=lambda x: -x[1]):
                print(f"  - {why}: {count}")

    # Nothing to do?
    if not actions:
        if not conflicts:
            print("\nNo consolidations planned.")
        else:
            print("\nNo consolidations applied due to conflicts. Re-run with --force to override root versions.")
        return

    # Plan -> edits
    rewrites_by_manifest: DefaultDict[Path, List[Tuple[Section, str, Any]]] = defaultdict(list)
    for act in actions:
        if act[0] == "root_set":
            continue
        _, manifest, section, dep_name, item = act
        rewrites_by_manifest[manifest].append((section, dep_name, item))

    # Dry run?
    if args.dry_run:
        print("\nPlanned changes:")
        for act in actions:
            if act[0] == "root_set":
                _, section, dep, ver = act
                dst = ROOT_WS_DEPS if section == "dependencies" else ROOT_WS_DEVDEPS
                print(f"  - Root: set [{dst}] {dep} = \"{ver}\"")
        for manifest, edits in rewrites_by_manifest.items():
            print(f"  - {manifest}:")
            for section, name, _item in edits:
                print(f"      rewrite {section}.{name} -> {{ workspace = true, ... }}")
        return

    # Apply root changes
    for act in actions:
        if act[0] != "root_set":
            continue
        _, section, dep_name, ver = act
        if section == "dependencies":
            set_root_ws_dep(ws_deps_tbl, dep_name, ver, force=args.force)
        else:
            set_root_ws_dep(ws_dev_tbl, dep_name, ver, force=args.force)


    # Write member manifests
    for manifest, edits in rewrites_by_manifest.items():
        doc = load_toml(manifest)
        for section, dep_name, old_item in edits:
            tbl = doc.get(section)
            if not isinstance(tbl, Table):
                continue
            tbl[dep_name] = dep_item_to_workspace_replacement(old_item)
        with manifest.open("w", encoding="utf-8") as f:
            f.write(tomlkit.dumps(doc))
        print(f"Updated {manifest}")

    # Write root
    with root_manifest.open("w", encoding="utf-8") as f:
        f.write(tomlkit.dumps(root_doc))
    print(f"Updated {root_manifest}")

if __name__ == "__main__":
    main()
