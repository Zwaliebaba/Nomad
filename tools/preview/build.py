"""Builds the single-file KR-114 voxel preview from the phase-1 assets.

Usage: python3 -I build.py <assets-dir> <template.html> <out.html>
"""
import base64
import json
import struct
import sys


def main() -> None:
    assets, template_path, out_path = sys.argv[1:4]
    station = json.load(open(f"{assets}/voxels/station_scene.json", encoding="utf-8"))
    ships = json.load(open(f"{assets}/voxels/ships_placeholder.json", encoding="utf-8"))
    placement = json.load(open(f"{assets}/layout/ships_placement.json", encoding="utf-8"))
    config = json.load(open(f"{assets}/layout/scene_config.json", encoding="utf-8"))
    fixture = json.load(open(f"{assets}/fixtures/siege_kr114.json", encoding="utf-8"))

    # Sector strengths are live data and come from the fixture (phase-1 doc 07 item 28):
    # sector "pylon_e" takes selection.pylons["E"].
    pylons = fixture["selection"]["pylons"]
    for sector in config["shield"]["sectors"]:
        sector["strength"] = pylons[sector["pylon"].removeprefix("pylon_").upper()]

    # Signed 16-bit positions support larger modular stations without coordinate wrap.
    packed = bytearray()
    modules = []
    for module in station["modules"]:
        modules.append({"id": module["id"], "name": module["name"], "count": len(module["voxels"])})
        for x, y, z, p in module["voxels"]:
            packed += struct.pack("<hhhH", x, y, z, p)

    data = {
        "palette": [[e["rgb"], bool(e["emissive"])] for e in station["palette"]],
        "voxels": base64.b64encode(bytes(packed)).decode("ascii"),
        "modules": modules,
        "shipPalette": ships["palette"],
        "shipEmissive": ships["emissive"],
        "shipModels": {k: m["voxels"] for k, m in ships["models"].items()},
        "placements": placement["placements"],
        "shipPlaneZ": placement["ship_plane_z"],
        "grid": config["ops_grid"],
        "shield": config["shield"],
        "pivot": config["camera"]["pivot_world"],
        "zoomLevels": config["camera"]["zoom_levels"],
        "projectionScale": config["projection"]["pixels_per_voxel_at_zoom_1"]["screen_x_per_world_x"] / 5,
    }
    template = open(template_path, encoding="utf-8").read()
    html = template.replace("/*__DATA__*/null", json.dumps(data, separators=(",", ":")))
    open(out_path, "w", encoding="utf-8").write(html)
    print(f"wrote {out_path}: {len(html) / 1024:.0f} KiB, {len(packed) // 8} station voxels")


if __name__ == "__main__":
    main()
