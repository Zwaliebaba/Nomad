#!/usr/bin/env python3
"""Check the shared palette and MagicaVoxel materials in the module assets.

Run from any directory with Python 3; no external dependencies are required.
This supplements nomad-vox, whose game format intentionally retains only RGB/Emit.
"""

import json
import struct
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULES = ROOT / "art/voxels/modules"


def chunks(data, begin, end):
    while begin < end:
        if begin + 12 > end:
            raise ValueError("truncated chunk header")
        tag, length, children = struct.unpack_from("<4sII", data, begin)
        content = begin + 12
        stop = content + length
        child_end = stop + children
        if child_end > end:
            raise ValueError("chunk extends outside parent")
        yield tag, data[content:stop]
        yield from chunks(data, stop, child_end)
        begin = child_end


def dictionary(data, offset):
    count = struct.unpack_from("<i", data, offset)[0]
    offset += 4
    if count < 0:
        raise ValueError("negative dictionary length")
    result = {}
    for _ in range(count):
        pair = []
        for _ in range(2):
            length = struct.unpack_from("<i", data, offset)[0]
            offset += 4
            if length < 0 or offset + length > len(data):
                raise ValueError("invalid dictionary string")
            pair.append(data[offset:offset + length].decode("utf-8"))
            offset += length
        key, value = pair
        if key in result:
            raise ValueError("duplicate material property")
        result[key] = value
    return result, offset


def inspect(path):
    data = path.read_bytes()
    if data[:4] != b"VOX " or struct.unpack_from("<I", data, 4)[0] != 200:
        raise ValueError("expected MagicaVoxel format 200")
    palettes, materials, colours = [], {}, Counter()
    size = None
    for tag, content in chunks(data, 8, len(data)):
        if tag == b"RGBA":
            if len(content) != 1024:
                raise ValueError("invalid palette length")
            palettes.append(content)
        elif tag == b"MATL":
            index = struct.unpack_from("<i", content)[0]
            props, stop = dictionary(content, 4)
            if stop != len(content) or index in materials:
                raise ValueError("invalid or duplicate material")
            materials[index] = props
        elif tag == b"SIZE":
            size = struct.unpack("<III", content)
            if any(not 1 <= dimension <= 256 for dimension in size):
                raise ValueError("invalid object dimensions")
        elif tag == b"XYZI":
            count = struct.unpack_from("<I", content)[0]
            if size is None or len(content) != 4 + count * 4:
                raise ValueError("invalid voxel array")
            positions = set()
            for offset in range(4, len(content), 4):
                x, y, z, colour = content[offset:offset + 4]
                if colour == 0 or any(v >= limit for v, limit in zip((x, y, z), size)):
                    raise ValueError("voxel outside palette or object")
                if (x, y, z) in positions:
                    raise ValueError("duplicate voxel")
                positions.add((x, y, z))
                colours[colour] += 1
    if len(palettes) != 1 or not colours:
        raise ValueError("expected one palette and nonempty geometry")
    return palettes[0], materials, colours


def audit_tube(path, entry):
    """Check the serialized geometry, including leaks and blocked junctions."""
    data = path.read_bytes()
    geometry, size, translation = None, None, None
    for tag, content in chunks(data, 8, len(data)):
        if tag == b"SIZE":
            if size is not None:
                raise ValueError(f"{path.name}: tube audit expects one object")
            size = struct.unpack("<III", content)
        elif tag == b"XYZI":
            geometry = [tuple(content[i:i + 3]) for i in range(4, len(content), 4)]
        elif tag == b"nTRN":
            _, offset = dictionary(content, 4)
            frame, _ = dictionary(content, offset + 16)
            if frame.get("_r", "4") != "4":
                raise ValueError(f"{path.name}: unexpected tube rotation")
            translation = tuple(map(int, frame["_t"].split()))
    if geometry is None or size is None or translation is None:
        raise ValueError(f"{path.name}: missing tube geometry or transform")
    voxels = {tuple(p[i] + translation[i] - size[i] // 2 for i in range(3))
              for p in geometry}
    bounds = [[min(p[i] for p in voxels) for i in range(3)],
              [max(p[i] for p in voxels) for i in range(3)]]
    if bounds != entry["bounds"]:
        raise ValueError(f"{path.name}: stale tube bounds")
    ports = entry["connection_ports"]
    profile = {(u, v) for u in range(-7, 8) for v in range(-7, 8)
               if 3.2 ** 2 < u * u + v * v <= 6.2 ** 2}
    targets = []
    for port in ports:
        dx, dy, dz = port["outward"]
        if dz != 0 or abs(dx) + abs(dy) != 1 or port["centre"] != [dx * 16, dy * 16, 9]:
            raise ValueError(f"{path.name}: invalid tube connection axis")
        mouth = {(-x * dy + y * dx, z - 9) for x, y, z in voxels
                 if x * dx + y * dy == 16}
        if mouth != profile:
            raise ValueError(f"{path.name}: coupling profiles differ or bore is capped")
        targets.append((dx * 15, dy * 15, 9))
    # Temporarily seal the mating planes to distinguish the bore from exterior air.
    # The connected interior must reach all port centres without leaking outside.
    seen, stack = {targets[0]}, [targets[0]]
    while stack:
        x, y, z = stack.pop()
        if any(value < bounds[0][i] or value > bounds[1][i]
               for i, value in enumerate((x, y, z))):
            raise ValueError(f"{path.name}: tube shell has a leak")
        for dx, dy, dz in ((1, 0, 0), (-1, 0, 0), (0, 1, 0),
                           (0, -1, 0), (0, 0, 1), (0, 0, -1)):
            p = (x + dx, y + dy, z + dz)
            if any(p[0] * port["outward"][0] + p[1] * port["outward"][1] >= 16
                   for port in ports):
                continue
            if p not in voxels and p not in seen:
                seen.add(p)
                stack.append(p)
    if any(p not in seen or p in voxels for p in targets):
        raise ValueError(f"{path.name}: blocked tube or disconnected junction")


def main():
    manifest = json.loads((MODULES / "ModulePalette.json").read_text())
    entries = manifest["materials"]
    expected_palette = bytearray(1024)
    expected_materials = {}
    for entry in entries:
        index = entry["index"]
        rgba = bytes.fromhex(entry["rgb"].removeprefix("#")) + b"\xff"
        expected_palette[(index - 1) * 4:index * 4] = rgba
        expected_materials[index] = entry["material"]
    inventory = json.loads((MODULES / "ModuleInventory.json").read_text())
    listed = {entry["id"]: entry for entry in inventory["models"]}
    manifest = json.loads((MODULES / "set.json").read_text())
    declared = {entry["id"]: entry.get("file", entry["id"] + ".vox") for entry in manifest["models"]}
    if any(declared.get(model_id) != entry["file"] for model_id, entry in listed.items()):
        raise ValueError("module inventory and set.json source filenames differ")
    paths = sorted(MODULES / entry["file"] for entry in listed.values())
    if not paths or any(not path.is_file() for path in paths):
        raise ValueError("module file membership differs from inventory")
    glass_models = {"biodome", "greenhouse_pair"}
    emit = {9, 10, 11, 12}
    if {i for i, props in expected_materials.items() if props["_type"] == "_emit"} != emit:
        raise ValueError("expected cyan, white, red and orange Emit slots")
    if expected_materials[16]["_type"] != "_glass":
        raise ValueError("glazing slot 16 must be Glass")
    by_file = {entry["file"]: entry for entry in listed.values()}
    for path in paths:
        entry = by_file[path.name]
        model_id = entry["id"]
        palette, materials, colours = inspect(path)
        if palette != expected_palette or materials != expected_materials:
            raise ValueError(f"{path.name}: shared palette or material table differs")
        if set(colours) - set(expected_materials):
            raise ValueError(f"{path.name}: voxel uses an undocumented palette slot")
        glass = colours[16]
        if (glass > 0) != (model_id in glass_models):
            raise ValueError(f"{path.name}: unexpected or missing glazing")
        if entry["voxels"] != colours.total() or entry["glass_voxels"] != glass:
            raise ValueError(f"{path.name}: inventory counts are stale")
        if not sum(colours[index] for index in emit):
            raise ValueError(f"{path.name}: no light voxels use Emit")
        if model_id.startswith("connector_"):
            audit_tube(path, entry)
        print(f"{path.name}: {colours.total():6d} voxels; {glass:4d} Glass; palette/materials OK")
    print(f"PASS: {len(paths)} files share all 256 RGBA entries and {len(entries)} material definitions")


if __name__ == "__main__":
    main()
