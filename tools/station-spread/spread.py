#!/usr/bin/env python3
"""Spread the phase-1 mother station: move the outer buildings away from the core (Design D-111).

The owner found the buildings too close together. This script rebuilds
`station_scene.json` and `ships_placement.json` from the original phase-1 layout, so running it
again never compounds a spread. To tune the spread, edit OFFSETS and run it again.

Each building moves away from the core along the connector that joins it to the station (SOL-1
slides sideways along pylon N's face as well), so every joint stays exact. The connector is
stretched by repeating a few slices from its middle, keeping their
pattern (the dry-dock rails alternate every 2 voxels; the solar bus has a light every 4). Offsets
that are multiples of 4 therefore leave no seam. The command core, deck, pylons, asteroids,
beacons and hostile ships stay where they are: the core and pylons define the dome (doc 15 §5.1,
D-100), and doc 10 §8 derives the voxel scale from HSTL-C1's distance to the dome centre.

Run from anywhere inside the repository (it reads the original layout with `git cat-file`):

    python3 tools/station-spread/spread.py           # rewrite the asset files
    python3 tools/station-spread/spread.py --check   # exit 1 if the files are out of date
"""

import argparse
import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

# The phase-1 package as delivered (git blob ids; content-addressed, so they survive any merge).
ORIGINAL_SCENE = "0896fffbc9a92621880ec2c7115c3ce32fe7a254"
ORIGINAL_SHIPS = "b8c06e30f7bf14e40246b79420de9684a528d3f6"

ASSETS = Path("Design/phase1-visual-slice/assets")
SCENE = ASSETS / "voxels/station_scene.json"
SHIPS = ASSETS / "layout/ships_placement.json"

# How far each building group moves, in voxels (x, y). One voxel is about 35 m (doc 10 §8).
# The factories and the dry dock move along their piers and struts only, so the joints stay
# exact. SOL-1 moves diagonally: its bus spine runs along y into pylon N's north face, which is
# 9 voxels wide (x -31..-23), so any x offset from -8 to 0 still lands on it; a pure y move would
# push its bottom panel row into the pylon.
OFFSETS = {
    "factory_fa1": (-4, 0),  # FA-1, ELEV +8, west: its pier runs along x
    "factory_fb2": (4, 0),  # FB-2, ELEV +4, east: its pier runs along x
    "dry_dock": (0, 4),  # SY-2, south: its two struts run along y
    "solar_array": (-4, -4),  # SOL-1, north-west, outside the dome: x in -8..0
    "cargo_pads": (-3, 3),  # the two pads between FA-1 and the dry dock (no connector)
}

Box = tuple[int, int, int, int]  # x0, y0, x1, y1, inclusive


@dataclass(frozen=True)
class Part:
    """Voxels of `module` inside `box` (all of the module if `box` is None)."""

    module: str
    box: Box | None = None


# Which voxels each group moves.
GROUPS = {
    "factory_fa1": [Part("factory_fa1")],
    "factory_fb2": [Part("factory_fb2")],
    "dry_dock": [Part("dry_dock")],
    # The panel field with its bus spine, and the bus markers along its south edge.
    "solar_array": [Part("solar_array"), Part("misc", (-62, -2, -30, -2))],
    "cargo_pads": [Part("misc", (-43, 39, -31, 51))],
}

# Voxels that stay even though their module moves: the bus link between pylon N and the deck.
STAY = [Part("solar_array", (-26, -4, -23, -3))]


@dataclass(frozen=True)
class Bridge:
    """A connector stretched along `axis` when the group `follows` moves.

    The bridge's voxels (of `module` inside `box`) beyond `cut` on the moving side move with the
    group, unless the whole module already moves. The gap opened at the cut is filled with
    copies of the slices `window`, phase-aligned to absolute coordinates so a repeating pattern
    continues; the copies take the group's offset across the axis too. `extra` closes a gap that
    already existed at the cut (the solar bus spine stopped one voxel short of pylon N, while
    its last panel row touched the pylon).
    """

    module: str
    box: Box
    axis: str
    cut: int
    follows: str
    window: tuple[int, int]
    extra: int = 0


BRIDGES = [
    # FA-1 pier: a beam at z 9-10 from FA-1 to a post standing beside the deck (x = -28).
    Bridge("truss", (-35, 19, -27, 20), "x", cut=-31, follows="factory_fa1", window=(-34, -31)),
    # FB-2 pier: a post beside pylon S (x = 15), then a beam at z 5-6 to FB-2.
    Bridge("truss", (15, 31, 22, 32), "x", cut=18, follows="factory_fb2", window=(17, 20)),
    # The two dry-dock struts from the deck edge to SY-2.
    Bridge("truss", (-17, 28, -16, 42), "y", cut=35, follows="dry_dock", window=(32, 35)),
    Bridge("truss", (-6, 28, -5, 42), "y", cut=35, follows="dry_dock", window=(32, 35)),
    # SOL-1's bus spine into pylon N (pylon face at y = -13).
    Bridge(
        "solar_array", (-23, -42, -23, -13), "y", cut=-13, follows="solar_array",
        window=(-24, -21), extra=1,
    ),
]

# Ships placed on a moved building move with it.
SHIP_GROUPS = {"SY-2 build": "dry_dock"}

N6 = ((1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1))


def git_blob(blob: str) -> bytes:
    return subprocess.run(
        ["git", "cat-file", "blob", blob], check=True, capture_output=True
    ).stdout


def in_box(v, box: Box | None) -> bool:
    return box is None or (box[0] <= v[0] <= box[2] and box[1] <= v[1] <= box[3])


def in_part(module: str, v, part: Part) -> bool:
    return module == part.module and in_box(v, part.box)


def spread(scene: dict) -> dict:
    """Returns a new scene with OFFSETS applied. `scene` is the original layout."""
    moved_modules = {
        part.module for group in GROUPS.values() for part in group if part.box is None
    }
    by_id = {m["id"]: m for m in scene["modules"]}
    new_voxels = {m["id"]: [] for m in scene["modules"]}

    for module in scene["modules"]:
        mid = module["id"]
        for v in module["voxels"]:
            dx, dy = 0, 0
            if not any(in_part(mid, v, s) for s in STAY):
                for name, parts in GROUPS.items():
                    if any(in_part(mid, v, p) for p in parts):
                        dx, dy = OFFSETS[name]
                for b in BRIDGES:
                    if mid != b.module or mid in moved_modules or not in_box(v, b.box):
                        continue
                    ox, oy = OFFSETS[b.follows]
                    offset = ox if b.axis == "x" else oy
                    coord = v[0] if b.axis == "x" else v[1]
                    if offset and (coord - b.cut) * offset > 0:
                        dx, dy = ox, oy
            new_voxels[mid].append([v[0] + dx, v[1] + dy, v[2], v[3]])

    for b in BRIDGES:
        ox, oy = OFFSETS[b.follows]
        offset = ox if b.axis == "x" else oy
        if not offset:
            continue
        step = 1 if offset > 0 else -1
        a = 0 if b.axis == "x" else 1
        across, across_offset = (1, oy) if b.axis == "x" else (0, ox)
        first, last = b.window
        width = last - first + 1
        template = [v for v in by_id[b.module]["voxels"] if in_box(v, b.box)]
        for i in range(1, abs(offset) + b.extra + 1):
            p = b.cut + step * i
            source = first + (p - first) % width
            for v in template:
                if v[a] == source:
                    copy = list(v)
                    copy[a] = p
                    copy[across] += across_offset
                    new_voxels[b.module].append(copy)

    out = dict(scene)
    out["modules"] = []
    for module in scene["modules"]:
        voxels = new_voxels[module["id"]]
        m = dict(module)
        m["bounds"] = {
            "min": [min(v[i] for v in voxels) for i in range(3)],
            "max": [max(v[i] for v in voxels) for i in range(3)],
        }
        m["voxel_count"] = len(voxels)
        m["voxels"] = voxels
        out["modules"].append(m)
    return out


def owners(scene: dict) -> dict:
    cells = {}
    for m in scene["modules"]:
        for v in m["voxels"]:
            cells.setdefault((v[0], v[1], v[2]), []).append(m["id"])
    return cells


def contacts(scene: dict) -> set:
    cells = owners(scene)
    pairs = set()
    for (x, y, z), ids in cells.items():
        for dx, dy, dz in N6:
            for b in cells.get((x + dx, y + dy, z + dz), ()):
                for a in ids:
                    if a < b:
                        pairs.add((a, b))
    return pairs


def check(original: dict, new: dict) -> list[str]:
    """Problems the spread introduced: overlapping voxels or lost joints."""
    problems = []
    before = {c for c, ids in owners(original).items() if len(ids) > 1}
    after = {c: ids for c, ids in owners(new).items() if len(ids) > 1}
    for cell, ids in sorted(after.items()):
        if cell not in before:
            problems.append(f"overlap at {cell}: {ids}")
    for a, b in sorted(contacts(original) - contacts(new)):
        problems.append(f"{a} no longer touches {b}")
    return problems


def move_ships(ships: dict) -> dict:
    out = json.loads(json.dumps(ships))
    for placement in out["placements"]:
        group = SHIP_GROUPS.get(placement["group"])
        if group:
            dx, dy = OFFSETS[group]
            placement["pos"][0] += dx
            placement["pos"][1] += dy
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true", help="only check the files are current")
    args = parser.parse_args()

    root = Path(
        subprocess.run(
            ["git", "rev-parse", "--show-toplevel"], check=True, capture_output=True, text=True
        ).stdout.strip()
    )
    original = json.loads(git_blob(ORIGINAL_SCENE))
    new = spread(original)
    problems = check(original, new)
    if problems:
        for p in problems:
            print(f"error: {p}", file=sys.stderr)
        return 1
    scene_text = json.dumps(new, separators=(",", ":"))
    ships_text = json.dumps(move_ships(json.loads(git_blob(ORIGINAL_SHIPS))), indent=1)

    outputs = {root / SCENE: scene_text, root / SHIPS: ships_text}
    if args.check:
        stale = [str(p.relative_to(root)) for p, t in outputs.items() if p.read_text() != t]
        for p in stale:
            print(f"out of date: {p} (run tools/station-spread/spread.py)", file=sys.stderr)
        return 1 if stale else 0
    for path, text in outputs.items():
        path.write_text(text)

    print("moved buildings (bounds in voxels, inclusive):")
    for m in new["modules"]:
        old = next(o for o in original["modules"] if o["id"] == m["id"])
        if m["bounds"] != old["bounds"]:
            b = m["bounds"]
            print(f"  {m['id']:<13} min {b['min']} max {b['max']}  ({m['voxel_count']} voxels)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
