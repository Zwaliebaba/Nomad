#!/usr/bin/env python3
"""Station kit: voxel modules placed by the slot template (owner direction, round 19 draft).

Builds a holding's voxel scene from a slot assignment instead of a hand-made diorama:

- one voxel model per module kind (core, pylon, refinery, storage, turret, dry dock, solar
  array, harvester, extractor), all in the phase-1 look and palette;
- the slot template these models are placed on: hub and spokes, every inner slot on a spoke
  from the core, the pylons at the end of the diagonal spokes on the dome rim, the solar
  array and the deposits outside the dome on the grid axes (D-130, D-131, D-228);
- generated trusses: the spokes and the power bus to the solar array (D-067: trusses are
  cosmetic and automatic).

The core, the pylons and the asteroids are reused from the phase-1 scene; everything else is
generated. Output is a `voxel-rts/scene@1` file that the client and tools/preview read.

    python3 tools/station-kit/kit.py --layout start  OUT_DIR   # KR-114 start kit (doc 14 §12)
    python3 tools/station-kit/kit.py --layout full   OUT_DIR   # every mother-station slot built
"""

import argparse
import copy
import json
import math
import shutil
from pathlib import Path

ASSETS = Path("Design/phase1-visual-slice/assets")
PHASE1_SCENE = ASSETS / "voxels/station_scene.json"

# ---- Slot template (grid voxels, core at the origin) --------------------------------------
# Grid north is azimuth 225° (D-046): the (-, -) diagonal is screen-up in view 0. Pylons sit
# on the diagonals (sector centres) at the dome rim; the grid axes are the sector boundaries.
DOME_R = 36
SPOKE_Z = 8  # the core's docking arms are at z 8–9
TEMPLATE = {
    "pylons": [(-30, -30), (30, -30), (30, 30), (-30, 30)],  # sector order N, E, S, W; wholly outside the dome (D-255)
    # Standard inner slots, in rules order: the first four exist at every holding.
    "inner": [(22, 0), (0, -22), (-22, 0), (-14, -14), (14, -14), (-14, 14)],
    "dock": (0, 23),
    "solar": (-62, 0),
    "deposits": [(88, 0), (0, 92)],  # well clear of the station; D-228 needs only > 46
}
RADII = {"core": 12, "pylon": 5, "inner": 7, "dock": 11, "solar": 18, "deposit": 10}

LAYOUTS = {
    # Doc 14 §12: core, 4 pylons, solar, harvester on its one ore field, refinery, storage,
    # dry dock. The mother-station site has a single standard ore field (D-098).
    "start": {"inner": ["refinery", "storage", None, None, None, None], "dock": "dry_dock",
              "solar": True, "deposits": ["harvester", None]},
    "full": {"inner": ["refinery", "storage", "refinery", "turret", "turret", "storage"],
             "dock": "dry_dock", "solar": True, "deposits": ["harvester", None]},
}

# ---- Palette ------------------------------------------------------------------------------
NEW_MATERIALS = {
    # name: (base hex, emissive, noise amplitude)
    "hull": ("#8a909f", False, 5),
    "hull_dark": ("#4e5466", False, 4),
    "hull_mid": ("#6a7283", False, 4),
    "deck_top": ("#5d6678", False, 4),
    "stripe": ("#3f485a", False, 3),
    "truss": ("#848a99", False, 4),
    "truss_dark": ("#5a6070", False, 3),
    "panel": ("#2a395a", False, 3),
    "panel_frame": ("#636c7e", False, 3),
    "tank": ("#9aa0ad", False, 4),
    "pipe": ("#707888", False, 3),
    "cont_olive": ("#6f7348", False, 5),
    "cont_rust": ("#8a5b3e", False, 5),
    "cont_tan": ("#b7995a", False, 5),
    "cont_blue": ("#56688a", False, 5),
    "barrel": ("#3a3f4c", False, 2),
    "crystal": ("#9fb6ff", False, 8),
    "rock": ("#5a5454", False, 8),
    "rock_dark": ("#36363f", False, 5),
    "light": ("#7ed8ed", True, 0),
    "emitter": ("#b8f6ff", True, 0),
    "beacon": ("#ff6a55", True, 0),
    "warm": ("#ffb15a", True, 0),
}
VARIANTS = 3


def hexrgb(h):
    return [int(h[i:i + 2], 16) for i in (1, 3, 5)]


def rgbhex(c):
    return "#" + "".join(f"{max(0, min(255, v)):02x}" for v in c)


class Palette:
    def __init__(self, base):
        self.entries = copy.deepcopy(base)
        self.mat = {}
        for name, (h, emissive, amp) in NEW_MATERIALS.items():
            idx = []
            for k in range(VARIANTS if amp else 1):
                d = (k - 1) * amp
                rgb = rgbhex([v + d for v in hexrgb(h)])
                self.entries.append({"rgb": rgb, "emissive": emissive})
                idx.append(len(self.entries) - 1)
            self.mat[name] = idx

    def __call__(self, name, x, y, z):
        idx = self.mat[name]
        return idx[(x * 73856093 ^ y * 19349663 ^ z * 83492791) % len(idx)]


# ---- Voxel model helpers -----------------------------------------------------------------
class Model:
    """Voxels keyed by position; later writes win."""

    def __init__(self, pal):
        self.v = {}
        self.pal = pal

    def put(self, x, y, z, mat):
        self.v[(x, y, z)] = self.pal(mat, x, y, z)

    def box(self, x0, x1, y0, y1, z0, z1, mat, hollow=False):
        for x in range(x0, x1 + 1):
            for y in range(y0, y1 + 1):
                for z in range(z0, z1 + 1):
                    if hollow and x0 < x < x1 and y0 < y < y1 and z0 < z < z1:
                        continue
                    self.put(x, y, z, mat)

    def disc(self, cx, cy, r, z0, z1, mat):
        for x in range(cx - r, cx + r + 1):
            for y in range(cy - r, cy + r + 1):
                if (x - cx) ** 2 + (y - cy) ** 2 <= r * r + r:
                    for z in range(z0, z1 + 1):
                        self.put(x, y, z, mat)

    def top_stripes(self, x0, x1, y0, y1, z, along="x", every=2):
        for x in range(x0 + 1, x1):
            for y in range(y0 + 1, y1):
                k = (y if along == "x" else x)
                self.put(x, y, z, "stripe" if k % every == 0 else "deck_top")

    def rotated(self, quarter):
        """Rotate about the z axis by quarter × 90° (counter-clockwise in grid xy)."""
        out = Model(self.pal)
        for (x, y, z), p in self.v.items():
            for _ in range(quarter % 4):
                x, y = -y, x
            out.v[(x, y, z)] = p
        return out

    def moved(self, dx, dy, dz=0):
        out = Model(self.pal)
        out.v = {(x + dx, y + dy, z + dz): p for (x, y, z), p in self.v.items()}
        return out


def quarter_for(dx, dy):
    """Quarter turns that point a model built facing +x towards (dx, dy)."""
    ang = math.atan2(dy, dx)
    return round(ang / (math.pi / 2)) % 4


# ---- Module models (built at the origin, facing +x = away from the core) ----------------
# Round 2 detail pass: panel seams every 3 voxels, corner posts, rim lights, pipework and
# greebles, at about the density of the phase-1 factories.
def panelled(m, x0, x1, y0, y1, z0, z1, base="hull_mid", seam="hull_dark", rim="hull"):
    """A box whose side faces carry vertical seams every 3 voxels and a lit top rim."""
    m.box(x0, x1, y0, y1, z0, z1, base)
    for x in range(x0, x1 + 1):
        for y in range(y0, y1 + 1):
            edge_x, edge_y = x in (x0, x1), y in (y0, y1)
            if not (edge_x or edge_y):
                continue
            k = y if edge_x and not edge_y else x
            if k % 3 == 0:
                for z in range(z0, z1):
                    m.put(x, y, z, seam)
            m.put(x, y, z1, rim)
            m.put(x, y, z0, seam)
    for x, y in ((x0, y0), (x0, y1), (x1, y0), (x1, y1)):
        m.box(x, x, y, y, z0, z1 + 1, "hull")


def refinery(pal):
    m = Model(pal)
    panelled(m, -5, 5, -5, 5, 4, 8)
    m.top_stripes(-5, 5, -5, 5, 8, along="y")
    # three tanks with bands, linked by a manifold
    for cx, cy in ((-2, -2), (-2, 3), (3, -2)):
        m.disc(cx, cy, 2, 9, 14, "tank")
        for z in (10, 13):
            m.disc(cx, cy, 2, z, z, "pipe")
        m.disc(cx, cy, 1, 15, 15, "hull_dark")
    m.box(-2, 3, -2, -2, 11, 11, "pipe")
    m.box(-2, -2, -2, 3, 11, 11, "pipe")
    # control block with a lit window strip and a vent stack
    panelled(m, 1, 5, 1, 5, 9, 11, base="hull", seam="hull_mid", rim="hull_mid")
    m.put(5, 3, 10, "light")
    m.box(4, 4, 4, 4, 12, 15, "pipe")
    m.put(4, 4, 16, "warm")
    # ore hopper facing outward (+x), with a chute into the body
    m.box(6, 8, -2, 2, 7, 8, "hull_dark")
    m.box(6, 8, -3, 3, 9, 9, "hull_mid")
    m.box(9, 9, -3, 3, 8, 9, "hull")
    m.put(9, 0, 10, "warm")
    return m


def storage(pal):
    m = Model(pal)
    m.box(-5, 5, -5, 5, 4, 4, "hull_dark")
    m.top_stripes(-5, 5, -5, 5, 4)
    # rack frame: posts, top rails and diagonal bracing on the long sides
    for x in (-5, 5):
        for y in (-5, 0, 5):
            m.box(x, x, y, y, 5, 13, "truss")
    for y in (-5, 5):
        m.box(-5, 5, y, y, 13, 13, "truss")
        for i in range(0, 8):
            m.put(-5 + i, y, 5 + i, "truss_dark")
    for x in (-5, 5):
        m.box(x, x, -5, 5, 13, 13, "truss")
    # containers in clamped bays, with darker door ends
    colours = ["cont_olive", "cont_rust", "cont_tan", "cont_blue"]
    k = 0
    for z0 in (5, 9):
        for y0 in (-4, 1):
            for x0 in (-4, 0):
                c = colours[(k * 3 + z0 + y0) % 4]
                m.box(x0, x0 + 3, y0, y0 + 2, z0, z0 + 2, c)
                m.box(x0 + 3, x0 + 3, y0, y0 + 2, z0, z0 + 2, "barrel")
                m.put(x0 + 1, y0, z0 + 3, "truss_dark")  # clamp
                k += 1
    # gantry crane across the top
    m.box(-6, 6, 0, 0, 14, 14, "hull_mid")
    m.box(1, 2, -1, 1, 15, 15, "hull")
    m.box(1, 1, 0, 0, 12, 13, "truss_dark")
    for x, y in ((5, 5), (-5, -5)):
        m.put(x, y, 14, "light")
    return m


def turret(pal):
    m = Model(pal)
    # stepped armoured base
    panelled(m, -4, 4, -4, 4, 4, 5)
    m.box(-3, 3, -3, 3, 6, 6, "hull_dark")
    m.disc(0, 0, 3, 7, 7, "pipe")  # ring track
    # turret head with a slit and a sensor box
    m.disc(0, 0, 3, 8, 9, "hull")
    m.box(-2, 2, -2, 2, 10, 10, "hull_mid")
    m.box(1, 3, -1, 1, 9, 9, "hull_dark")
    m.box(-3, -2, 1, 2, 10, 11, "hull_mid")
    m.put(-3, 2, 12, "beacon")
    # twin barrels with dark muzzles
    for y in (-1, 1):
        m.box(3, 9, y, y, 9, 9, "pipe")
        m.put(10, y, 9, "barrel")
    m.put(4, 4, 6, "light")
    return m


def dry_dock(pal):
    """Open frame along +x (radial), 22 long, for one frigate."""
    m = Model(pal)
    x0, x1 = -10, 10
    for y in (-5, 5):
        m.box(x0, x1, y, y, 3, 3, "truss")
        m.box(x0, x1, y, y, 12, 12, "truss")
        for x in range(x0, x1 + 1, 4):
            m.box(x, x, y, y, 3, 12, "hull_mid" if x in (x0, x1) else "truss")

        for x in range(x0, x1 - 3, 4):  # diagonal bracing in each bay
            for i in range(4):
                m.put(x + i, y, 4 + i * 2, "truss_dark")
    for x in range(x0, x1 + 1, 4):
        m.box(x, x, -5, 5, 12, 12, "truss_dark")
    # gantry crane riding the top rails, over the hull
    m.box(2, 3, -6, 6, 13, 13, "hull_mid")
    m.box(2, 3, -1, 1, 11, 12, "hull_dark")

    # back wall towards the core: machinery and a lit hatch
    panelled(m, x0 - 3, x0 - 1, -5, 5, 3, 12)
    m.box(x0 - 1, x0 - 1, -2, 2, 6, 9, "hull_dark")
    m.put(x0, 0, 8, "light")
    m.box(-6, 6, -1, 1, 2, 2, "truss_dark")  # keel cradle
    for x in (-6, 0, 6):
        m.box(x, x, -2, 2, 2, 3, "truss")
    # approach lights at the open end
    m.put(x1 + 1, -5, 12, "light")
    m.put(x1 + 1, 5, 12, "light")
    return m


def pylon(pal):
    """Shield pylon on an outrigger outside the dome (D-255): a housing where the spoke
    arrives, a braced mast, and the emitter crown on the equator (D-100, z = 16)."""
    m = Model(pal)
    panelled(m, -3, 3, -3, 3, 5, 10)
    m.top_stripes(-3, 3, -3, 3, 10)
    for d in ((-4, 0), (4, 0), (0, -4), (0, 4)):  # cooling fins
        m.box(min(0, d[0]) if d[0] else -1, max(0, d[0]) if d[0] else 1,
              min(0, d[1]) if d[1] else -1, max(0, d[1]) if d[1] else 1, 6, 9, "hull_dark")
    m.box(-1, 1, -1, 1, 1, 4, "truss_dark")  # lower counter-spar

    for x, y in ((-1, -1), (1, -1), (-1, 1), (1, 1)):
        m.box(x, x, y, y, 11, 15, "truss")
    for z in (12, 14):
        m.box(-1, 1, -1, 1, z, z, "truss_dark")
    m.box(-2, 2, -2, 2, 15, 15, "hull_mid")  # crown ring
    for x, y in ((-2, -2), (2, -2), (-2, 2), (2, 2)):
        m.put(x, y, 16, "emitter")
    m.box(0, 0, 0, 0, 16, 18, "emitter")
    return m


def empty_mount(pal):
    m = Model(pal)
    panelled(m, -2, 2, -2, 2, 7, 8)
    for x, y in ((-2, -2), (2, 2)):
        m.put(x, y, 9, "truss_dark")
    m.put(0, 0, 9, "light")
    return m


def solar(pal):
    """Four panel wings around a hub, on support struts; the bus enters from +x."""
    m = Model(pal)
    panelled(m, -3, 3, -3, 3, SPOKE_Z - 2, SPOKE_Z + 2)
    m.box(-1, 1, -1, 1, SPOKE_Z + 3, SPOKE_Z + 4, "hull_dark")  # transformer
    m.put(0, 0, SPOKE_Z + 5, "light")
    for qx in (-1, 1):
        for qy in (-1, 1):
            xa, ya = 5 * qx, 5 * qy
            xb, yb = 18 * qx, 18 * qy
            for x in range(min(xa, xb), max(xa, xb) + 1):
                for y in range(min(ya, yb), max(ya, yb) + 1):
                    frame = (x % 5 == 0) or (y % 5 == 0) or x in (xa, xb) or y in (ya, yb)
                    m.put(x, y, SPOKE_Z, "panel_frame" if frame else "panel")
            for t in range(5, 19, 6):  # struts under each wing
                m.put(t * qx, t * qy, SPOKE_Z - 1, "truss_dark")
    for x in range(-18, 19):
        m.put(x, 0, SPOKE_Z, "truss")
        m.put(x, 0, SPOKE_Z - 1, "truss_dark")
    for y in range(-18, 19):
        m.put(0, y, SPOKE_Z, "truss")
        m.put(0, y, SPOKE_Z - 1, "truss_dark")
    return m


def harvester(pal):
    """Mining rig clamped onto the ore field's largest rock: platform, four clamp legs,
    a drill tower with its bit in the rock, an ore bin and a beacon mast."""
    m = Model(pal)
    panelled(m, -4, 3, -3, 3, 9, 10)
    m.top_stripes(-4, 3, -3, 3, 10)
    for x, y in ((-4, -3), (-4, 3), (3, -3), (3, 3)):  # clamp legs gripping the rock
        m.box(x, x, y, y, 4, 8, "truss_dark")
        m.box(x - (1 if x < 0 else -1), x, y, y, 4, 4, "hull_dark")
    m.box(-1, 1, -1, 1, 11, 16, "truss")  # drill tower
    m.box(-2, 2, -2, 2, 17, 17, "hull_mid")
    m.box(0, 0, 0, 0, 2, 8, "pipe")  # drill shaft into the rock
    m.put(0, 0, 1, "warm")
    m.box(-4, -2, 0, 3, 11, 13, "hull_dark")  # ore bin
    m.box(-3, -2, 1, 2, 14, 14, "rock")
    m.box(3, 3, -3, -3, 11, 15, "truss_dark")  # beacon mast
    m.put(3, -3, 16, "warm")
    return m


def extractor(pal, crystals=True):
    m = harvester(pal)
    if crystals:
        for (x, y, h) in ((4, -2, 4), (5, 1, 6), (3, 3, 3)):
            m.box(x, x, y, y, 6, 6 + h, "crystal")
    return m


def debris(pal, seed, size):
    """A small rough rock: a noisy blob of `size` voxels across."""
    m = Model(pal)
    r = size / 2
    for x in range(-size, size + 1):
        for y in range(-size, size + 1):
            for z in range(0, size + 1):
                h = (x * 92837111 ^ y * 689287499 ^ z * 283923481 ^ seed * 1234577) % 100
                d = (x * x + y * y) / (r * r) + ((z - r) ** 2) / (r * r)
                if d <= 1.0 + (h - 50) / 160:
                    m.put(x, y, z, "rock" if h % 3 else "rock_dark")
    return m


MODELS = {"refinery": refinery, "storage": storage, "turret": turret,
          "dry_dock": dry_dock, "harvester": harvester, "extractor": extractor}


# ---- Trusses ------------------------------------------------------------------------------
def truss_line(m, a, b, z, light_every=6, width=1):
    """A 2-wide lattice truss from a to b along one grid axis or diagonal."""
    (ax, ay), (bx, by) = a, b
    n = max(abs(bx - ax), abs(by - ay))
    if n == 0:
        return
    sx, sy = (bx - ax) / n, (by - ay) / n
    diag = sx != 0 and sy != 0
    for i in range(n + 1):
        x, y = round(ax + sx * i), round(ay + sy * i)
        top = "truss" if i % 2 else "truss_dark"
        if diag:
            m.put(x, y, z, "truss")
            m.put(x, y, z + 1, top)
            m.put(x + (1 if sx > 0 else -1), y, z, "truss_dark")
        else:
            px, py = (0, 1) if sx else (1, 0)
            m.put(x, y, z, "truss")
            m.put(x + px, y + py, z, "truss")
            m.put(x, y, z + 1, top)
            m.put(x + px, y + py, z + 1, "truss_dark" if i % 2 else "truss")
        if light_every and i % light_every == light_every // 2:
            m.put(x, y, z + 2, "light")


# ---- Assembly ---------------------------------------------------------------------------
def place(scene, name, model, at, facing_from_core=True, z=0):
    q = quarter_for(at[0], at[1]) if facing_from_core else 0
    scene[name] = model.rotated(q).moved(at[0], at[1], z)


def phase1_module(phase1, mid, pal, dx=0, dy=0, dz=0, keep=None):
    src = next(m for m in phase1["modules"] if m["id"] == mid)
    out = Model(pal)
    for x, y, z, p in src["voxels"]:
        if keep and not keep(x, y, z):
            continue
        out.v[(x + dx, y + dy, z + dz)] = p
    return out


def build(layout_name):
    phase1 = json.load(open(PHASE1_SCENE, encoding="utf-8"))
    pal = Palette(phase1["palette"])
    lay = LAYOUTS[layout_name]
    scene = {}

    # Core: the phase-1 spindle without the deck stubs below its docking arms.
    scene["core"] = phase1_module(
        phase1, "core_spindle", pal,
        keep=lambda x, y, z: z >= 5 or max(abs(x), abs(y)) <= 7)

    for pid, slot in zip(("pylon_n", "pylon_e", "pylon_s", "pylon_w"), TEMPLATE["pylons"]):
        scene[pid] = pylon(pal).moved(*slot)

    spokes = Model(pal)
    # Axial spokes from the docking arms (radius 11) to every inner slot on an axis, and to
    # the dock; diagonal spokes from the core to each pylon through the diagonal slots.
    for slot in TEMPLATE["inner"][:3] + [TEMPLATE["dock"]]:
        r = max(abs(slot[0]), abs(slot[1]))
        u = (slot[0] // r, slot[1] // r)
        truss_line(spokes, (u[0] * 12, u[1] * 12), (u[0] * (r - 6), u[1] * (r - 6)), SPOKE_Z,
                   light_every=0)
    for p in TEMPLATE["pylons"]:
        u = (1 if p[0] > 0 else -1, 1 if p[1] > 0 else -1)
        truss_line(spokes, (u[0] * 7, u[1] * 7), (p[0] - u[0] * 4, p[1] - u[1] * 4), SPOKE_Z,
                   light_every=0)
    # Collars where the axial spokes meet the core's docking arms and the modules.
    for slot in TEMPLATE["inner"][:3] + [TEMPLATE["dock"]]:
        r = max(abs(slot[0]), abs(slot[1]))
        u = (slot[0] // r, slot[1] // r)
        for dist in (12, r - 6):
            cx, cy = u[0] * dist, u[1] * dist
            px, py = (0, 1) if u[0] else (1, 0)
            for a in (-1, 0, 1, 2):
                for zz in (SPOKE_Z - 1, SPOKE_Z + 2):
                    spokes.put(cx + px * a, cy + py * a, zz, "hull_mid")
            spokes.put(cx - px, cy - py, SPOKE_Z, "hull_mid")
            spokes.put(cx + px * 2, cy + py * 2, SPOKE_Z + 1, "hull_mid")
    scene["spokes"] = spokes

    for i, (slot, kind) in enumerate(zip(TEMPLATE["inner"], lay["inner"])):
        model = MODELS[kind](pal) if kind else empty_mount(pal)
        place(scene, f"inner_{i}_{kind or 'empty'}", model, slot)

    place(scene, "dry_dock", dry_dock(pal), TEMPLATE["dock"])

    if lay["solar"]:
        scene["solar_array"] = solar(pal).moved(*TEMPLATE["solar"])
        bus = Model(pal)
        sx = TEMPLATE["solar"][0]
        truss_line(bus, (-28, 0), (sx + 4, 0), SPOKE_Z, light_every=8)
        scene["power_bus"] = bus

    # Deposit 0: the ore field, well out from the station. Rocks from phase 1, spread around
    # the slot at varied heights, with the harvester clamped on the largest. No conveyor: ore
    # reaches the refinery abstractly (doc 14 §6), and a long truss read as clutter.
    d = TEMPLATE["deposits"][0]
    rocks = [("asteroid_1", (54, 7, 5), (d[0] - 2, d[1] - 6, 0)),
             ("asteroid_2", (67, 22, 4), (d[0] + 16, d[1] + 12, 4)),
             ("asteroid_3", (48, 26, 2), (d[0] + 2, d[1] + 20, 8)),
             ("asteroid_4", (68, 5, 2), (d[0] + 20, d[1] - 16, 10)),
             ("asteroid_5", (59, 35, 2), (d[0] - 10, d[1] + 16, 2))]
    for rid, c, (tx, ty, tz) in rocks:
        scene[rid] = phase1_module(phase1, rid, pal, tx - c[0], ty - c[1], tz)
    scatter = [(-14, -4, 3, 3), (24, 4, 6, 2), (8, -22, 2, 3), (-6, 28, 5, 2), (30, 24, 1, 2),
               (-18, 12, 9, 2), (14, 30, 4, 1)]
    field = Model(pal)
    for i, (dx, dy, dz, size) in enumerate(scatter):
        field.v.update(debris(pal, i, size).moved(d[0] + dx, d[1] + dy, dz).v)
    scene["ore_debris"] = field
    if lay["deposits"][0]:
        h = MODELS[lay["deposits"][0]](pal).moved(d[0] - 2, d[1] - 6, 4)
        scene["harvester"] = h

    return phase1, pal, scene


def write(out_dir, layout_name):
    phase1, pal, scene = build(layout_name)
    modules = []
    for name, m in scene.items():
        vox = sorted([x, y, z, p] for (x, y, z), p in m.v.items())
        xs, ys, zs = zip(*[(v[0], v[1], v[2]) for v in vox])
        modules.append({"id": name, "name": name, "elevation_tier": 0,
                        "bounds": {"min": [min(xs), min(ys), min(zs)], "max": [max(xs), max(ys), max(zs)]},
                        "voxel_count": len(vox), "voxels": vox})
    out = dict(phase1)
    out["palette"] = pal.entries
    out["modules"] = modules
    out_dir = Path(out_dir)
    # A full asset folder for tools/preview: copy phase 1, replace the scene and recentre.
    if (out_dir / "voxels").exists():
        shutil.rmtree(out_dir)
    shutil.copytree(ASSETS, out_dir, ignore=shutil.ignore_patterns("fonts"))
    json.dump(out, open(out_dir / "voxels/station_scene.json", "w"), separators=(",", ":"))
    cfg = json.load(open(out_dir / "layout/scene_config.json"))
    cfg["camera"]["pivot_world"] = [-4, 4, 0]
    cfg["shield"]["center"] = [0, 0, 16]
    json.dump(cfg, open(out_dir / "layout/scene_config.json", "w"), indent=1)
    pl = json.load(open(out_dir / "layout/ships_placement.json"))
    for p in pl["placements"]:
        if p["model"] == "warden_frigate":
            p["pos"] = [TEMPLATE["dock"][0], TEMPLATE["dock"][1], 4]
            p["heading_deg"] = 90
        if p["group"].startswith("CAP WING"):
            p["pos"][0] -= 6
            p["pos"][1] -= 26
    json.dump(pl, open(out_dir / "layout/ships_placement.json", "w"), indent=1)
    total = sum(m["voxel_count"] for m in modules)
    print(f"{layout_name}: {len(modules)} modules, {total} voxels -> {out_dir}")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--layout", choices=sorted(LAYOUTS), default="start")
    ap.add_argument("out_dir")
    a = ap.parse_args()
    write(a.out_dir, a.layout)
