#!/usr/bin/env python3
"""Assemble the authored SpaceStation layout; module .vox files stay authoritative.

Run from anywhere. --check verifies every generated artifact without writing it.
No dependencies beyond Python 3. Reads the same palette/material audit as the art tools.
"""
import argparse
import importlib.util
import json
import math
import struct
from collections import deque
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULES = ROOT / 'art/voxels/modules'
SCENE = ROOT / 'art/scenes/SpaceStation'
BASE = ROOT / 'Design/phase1-visual-slice/assets'
spec = importlib.util.spec_from_file_location('audit_modules', ROOT / 'tools/vox/audit_modules.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def chunk(tag, content=b'', children=b''):
    return tag + struct.pack('<II', len(content), len(children)) + content + children


def dictionary(values):
    result = struct.pack('<i', len(values))
    for key, value in values.items():
        for text in (key, str(value)):
            data = text.encode('utf-8')
            result += struct.pack('<i', len(data)) + data
    return result


def read_source(path):
    data = path.read_bytes()
    size, translation, voxels = None, None, None
    for tag, content in audit.chunks(data, 8, len(data)):
        if tag == b'SIZE':
            if size is not None:
                raise ValueError(f'{path}: this assembly tool expects a single source object')
            size = struct.unpack('<III', content)
        elif tag == b'XYZI':
            voxels = [tuple(content[i:i + 4]) for i in range(4, len(content), 4)]
        elif tag == b'nTRN':
            _, offset = audit.dictionary(content, 4)
            frame, _ = audit.dictionary(content, offset + 16)
            if frame.get('_r', '4') != '4':
                raise ValueError(f'{path}: bake rotation into the voxels first')
            translation = list(map(int, frame.get('_t', '0 0 0').split()))
    if size is None or voxels is None or translation is None:
        raise ValueError(f'{path}: missing geometry/transform')
    return {tuple(v[i] + translation[i] - size[i] // 2 for i in range(3)): v[3] for v in voxels}


def moved(voxels, position, turns):
    result = {}
    for (x, y, z), colour in voxels.items():
        for _ in range(turns):
            x, y = -y, x
        result[x + position[0], y + position[1], z + position[2]] = colour
    return result


def tube(template, start, end):
    axes = [i for i in range(3) if start[i] != end[i]]
    if not axes:
        return {}
    if len(axes) != 1:
        raise ValueError('a straight tube must have axis-aligned endpoints')
    axis = axes[0]
    low, high = sorted((start[axis], end[axis]))
    length = high - low
    if length < 6:
        raise ValueError('tube is too short for its coupling collars')
    slices = {}
    for (x, y, z), colour in template.items():
        slices.setdefault(x, []).append((y, z - 9, colour))
    result = {}
    for distance in range(length + 1):
        source_x = (-16 + distance if distance < 3 else
                    16 - (length - distance) if distance > length - 3 else
                    -13 + (distance - 3) % 27)
        for u, v, colour in slices[source_x]:
            p = list(start)
            p[axis] = low + distance
            if axis == 0:
                p[1] += u; p[2] += v
            elif axis == 1:
                p[0] += u; p[2] += v
            else:
                p[0] += u; p[1] += v
            result[tuple(p)] = colour
    return result


def bounds(voxels):
    return [[min(p[i] for p in voxels) for i in range(3)],
            [max(p[i] for p in voxels) for i in range(3)]]


def encode_vox(instances, palette):
    packed = chunk(b'PACK', struct.pack('<I', len(instances)))
    nodes = chunk(b'nTRN', struct.pack('<i', 0) + dictionary({'_name': 'SpaceStation'}) +
                  struct.pack('<iiii', 1, -1, -1, 1) + dictionary({'_t': '0 0 0'}))
    nodes += chunk(b'nGRP', struct.pack('<i', 1) + dictionary({}) +
                   struct.pack('<i', len(instances)) +
                   b''.join(struct.pack('<i', 2 + 2 * i) for i in range(len(instances))))
    for index, (name, voxels) in enumerate(instances.items()):
        lo, hi = bounds(voxels)
        size = [hi[i] - lo[i] + 1 for i in range(3)]
        if max(size) > 256:
            raise ValueError(f'{name}: object exceeds MagicaVoxel dimensions')
        packed += chunk(b'SIZE', struct.pack('<III', *size))
        packed += chunk(b'XYZI', struct.pack('<I', len(voxels)) + b''.join(
            bytes([p[i] - lo[i] for i in range(3)] + [c]) for p, c in sorted(voxels.items())))
        centre = ' '.join(str(lo[i] + size[i] // 2) for i in range(3))
        nodes += chunk(b'nTRN', struct.pack('<i', 2 + 2 * index) + dictionary({'_name': name}) +
                       struct.pack('<iiii', 3 + 2 * index, -1, -1, 1) +
                       dictionary({'_t': centre, '_r': '4'}))
        nodes += chunk(b'nSHP', struct.pack('<i', 3 + 2 * index) + dictionary({}) +
                       struct.pack('<ii', 1, index) + dictionary({}))
    rgba = b''.join(bytes.fromhex(m['rgb'][1:]) + b'\xff' for m in palette)
    packed += chunk(b'RGBA', rgba + bytes((256 - len(palette)) * 4))
    packed += nodes
    for material in palette:
        packed += chunk(b'MATL', struct.pack('<i', material['index']) + dictionary(material['material']))
    return b'VOX ' + struct.pack('<I', 200) + chunk(b'MAIN', children=packed)


def json_bytes(value):
    return (json.dumps(value, indent=2, ensure_ascii=False) + '\n').encode()


def build():
    layout = json.loads((SCENE / 'layout.json').read_text())
    inventory = json.loads((MODULES / 'ModuleInventory.json').read_text())['models']
    palette = json.loads((MODULES / 'ModulePalette.json').read_text())['materials']
    sources = {m['id']: read_source(MODULES / m['file']) for m in inventory}
    parts = {m['id']: moved(sources[m['model']], m['position'], m['quarter_turns'])
             for m in layout['instances']}
    if len(parts) != len(layout['instances']):
        raise ValueError('instance names must be unique')
    model_by_instance = {m['id']: m['model'] for m in layout['instances']}
    graph = {name: set() for name in parts}
    carved = 0
    for connection in layout['connections']:
        a, b = connection['start'], connection['end']
        axes = [i for i in range(3) if a[i] != b[i]]
        if len(axes) > 1:
            raise ValueError(f'{connection["id"]}: route is not axis-aligned')
        for name, point in ((connection['source'], a), (connection['target'], b)):
            if name not in parts:
                raise ValueError(f'unknown endpoint {name}')
            if model_by_instance[name].startswith('connector_') or not axes:
                continue
            axis = axes[0]
            remove = [p for p in parts[name] if abs(p[axis] - point[axis]) <= 5 and
                      sum((p[i] - point[i]) ** 2 for i in range(3) if i != axis) <= 3.2 ** 2]
            for p in remove:
                del parts[name][p]
            carved += len(remove)
        graph[connection['source']].add(connection['target'])
        graph[connection['target']].add(connection['source'])
        geometry = tube(sources['connector_straight'], a, b)
        if geometry:
            parts[connection['id']] = geometry
    # Mounted art must physically touch its supporting module.
    for parent, child in layout['mounts']:
        neighbours = ((1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1))
        if not any((x + dx, y + dy, z + dz) in parts[parent]
                   for x, y, z in parts[child] for dx, dy, dz in neighbours):
            raise ValueError(f'{child}: mounting does not touch {parent}')
        graph[parent].add(child); graph[child].add(parent)
    reached, queue = {'Core'}, deque(['Core'])
    while queue:
        for adjacent in graph[queue.popleft()] - reached:
            reached.add(adjacent); queue.append(adjacent)
    if reached != set(graph):
        raise ValueError(f'disconnected modules: {set(graph) - reached}')
    # Only matching coupling zones may overlap. Assign shared voxels to one object.
    owners, shared = {}, 0
    for name, voxels in parts.items():
        for p in list(voxels):
            if p in owners:
                previous = owners[p]
                allowed = any(
                    {name, previous} <= {c['source'], c['target'], c['id']} and
                    any(sum((p[i] - q[i]) ** 2 for i in range(3)) <= 7.5 ** 2
                        for q in (c['start'], c['end'])) for c in layout['connections'])
                if not allowed:
                    raise ValueError(f'unexpected overlap at {p}: {name}, {previous}')
                del voxels[p]; shared += 1
            else:
                owners[p] = name
    physical = {name: set() for name in parts}
    for (x, y, z), owner in owners.items():
        for p in ((x + 1, y, z), (x, y + 1, z), (x, y, z + 1)):
            other = owners.get(p)
            if other is not None and other != owner:
                physical[owner].add(other); physical[other].add(owner)
    reached_geometry, queue = {'Core'}, deque(['Core'])
    while queue:
        for adjacent in physical[queue.popleft()] - reached_geometry:
            reached_geometry.add(adjacent); queue.append(adjacent)
    if reached_geometry != set(parts):
        raise ValueError(f'geometry is not physically connected: {set(parts) - reached_geometry}')
    shield = layout['shield']
    projectors = set(shield['projectors'])
    if len(projectors) != 4 or not projectors <= set(model_by_instance):
        raise ValueError('the scene needs four shield-projector towers')
    if {name for name, model in model_by_instance.items() if model == shield['projector_model']} != projectors:
        raise ValueError('shield-projector tower models must only occur at the external projector positions')
    plant = {p: c for name, voxels in parts.items() if name not in projectors
             and name not in {connection['id'] for connection in layout['connections']}
             for p, c in voxels.items()}
    plant_min, plant_max = bounds(plant)
    centre = shield['center']
    emitter_errors = []
    emitter_contacts = {}
    generator_links = []
    generator_clearances = []
    external_feeds = {connection['id'] for connection in layout['connections']
                      if connection['source'] in projectors or connection['target'] in projectors}
    for instance in layout['instances']:
        if instance['id'] not in projectors:
            continue
        _, _, z = instance['position']
        tower_min, tower_max = bounds(parts[instance['id']])
        if not any(tower_max[i] < plant_min[i] or tower_min[i] > plant_max[i] for i in (0, 1)):
            raise ValueError(f'{instance["id"]}: shield projector is inside the plant footprint')
        if z + shield['emitter_height'] != shield['center'][2]:
            raise ValueError('projector emitter crowns must share the shield centre height')
        emitter_local = tuple(shield['emitter_local'])
        if sources[instance['model']].get(emitter_local) not in {
                m['index'] for m in palette if m['material']['_type'] == '_emit'}:
            raise ValueError('shield attachment point must be an actual Emit voxel in the generator')
        source = sources[instance['model']]
        above = (emitter_local[0], emitter_local[1], emitter_local[2] + 1)
        if above in source or emitter_local[2] != max(p[2] for p in source):
            raise ValueError('shield attachment must be on the exposed top face of the highest emitter lens')
        contact = tuple(shield['emitter_contact_local'])
        if contact != (emitter_local[0] + 0.5, emitter_local[1] + 0.5, emitter_local[2] + 1):
            raise ValueError('shield contact must be the centre of the emitter voxel top face')
        emitter = next(iter(moved({contact: 1}, instance['position'], instance['quarter_turns'])))
        emitter_contacts[instance['id']] = list(emitter)
        delta_x, delta_y = emitter[0] - centre[0], emitter[1] - centre[1]
        distance = math.hypot(delta_x, delta_y)
        if not 8 <= distance - shield['radius_xy'] <= 64:
            raise ValueError('generator emitters must be just outside the field, linked across a short gap')
        field_contact = [centre[0] + delta_x * shield['radius_xy'] / distance,
                         centre[1] + delta_y * shield['radius_xy'] / distance, emitter[2]]
        generator_links.append(dict(generator=instance['id'], emitter=list(emitter), contact=field_contact))
        def nearest_squared(value, axis):
            return 0 if value <= centre[axis] <= value + 1 else min(abs(value - centre[axis]), abs(value + 1 - centre[axis])) ** 2
        clearance = min((nearest_squared(x, 0) + nearest_squared(y, 1)) / shield['radius_xy'] ** 2 +
                        nearest_squared(z, 2) / shield['radius_z'] ** 2 for x, y, z in parts[instance['id']])
        generator_clearances.append(clearance)
        if clearance <= 1:
            raise ValueError(f'{instance["id"]}: generator housing must be entirely outside the shield')
        radius = ((field_contact[0] - centre[0]) ** 2 + (field_contact[1] - centre[1]) ** 2) / shield['radius_xy'] ** 2 + (field_contact[2] - centre[2]) ** 2 / shield['radius_z'] ** 2
        emitter_errors.append(abs(radius - 1))
        if emitter_errors[-1] > 1e-9:
            raise ValueError(f'{instance["id"]}: energy link must end on the field surface')
    coverage = max((max(abs(x - centre[0]), abs(x + 1 - centre[0])) ** 2 +
                    max(abs(y - centre[1]), abs(y + 1 - centre[1])) ** 2) /
                   shield['radius_xy'] ** 2 +
                   max(abs(z - centre[2]), abs(z + 1 - centre[2])) ** 2 / shield['radius_z'] ** 2
                   for (x, y, z), owner in owners.items() if owner not in projectors | external_feeds)
    if coverage > 1:
        raise ValueError(f'the protective field does not cover the plant and tubes: {coverage}')
    used = set(model_by_instance.values()) | {'connector_straight'}
    if used != set(sources):
        raise ValueError(f'module type coverage differs: {used ^ set(sources)}')
    modules = []
    for name, voxels in parts.items():
        lo, hi = bounds(voxels)
        modules.append(dict(id=name, name=name, elevation_tier=0, bounds=dict(min=lo, max=hi),
                            voxel_count=len(voxels), voxels=[[x, y, z, c - 1] for (x, y, z), c in sorted(voxels.items())]))
    game_palette = [dict(rgb=m['rgb'], emissive=m['material']['_type'] == '_emit',
                         material=m['material']['_type'].removeprefix('_')) for m in palette]
    scene = dict(format='voxel-rts/scene@1', generated='tools/station-scene/build.py; edit layout.json or the module .vox sources',
                 units=dict(voxel='1 grid cell'), origin='Core slot datum; free-floating assembly',
                 shading=dict(top=1.0, left=0.77, right=0.48), palette=game_palette, modules=modules)
    outputs = {SCENE / 'SpaceStation.vox': encode_vox(parts, palette),
               SCENE / 'assets/voxels/station_scene.json': (json.dumps(scene, separators=(',', ':')) + '\n').encode()}
    for relative in ('layout/hud_layout.json', 'voxels/ships_placeholder.json'):
        outputs[SCENE / 'assets' / relative] = (BASE / relative).read_bytes()
    fixture = json.loads((BASE / 'fixtures/siege_kr114.json').read_text())
    fixture['selection']['pylons'] = dict(N=1.0, E=1.0, S=1.0, W=1.0)
    outputs[SCENE / 'assets/fixtures/siege_kr114.json'] = json_bytes(fixture)
    config = json.loads((BASE / 'layout/scene_config.json').read_text())
    config['projection']['pixels_per_voxel_at_zoom_1'] = dict(screen_x_per_world_x=1.35, screen_y_per_world_x=0.675, screen_y_per_world_z=-1.35)
    config['projection']['origin_screen_view0'] = [960, 580]
    config['camera']['pivot_world'] = [0, 0, 0]
    config['camera']['ops_area_half_extent_voxels'] = 400
    for style in ('minor', 'major'):
        config['ops_grid'][style]['alpha_near'] = 0.0001
        config['ops_grid'][style]['alpha_far'] = 0.00005
    config['shield'].update(center=shield['center'], radius_xy=shield['radius_xy'],
                            radius_z=shield['radius_z'], rest_alpha_fill=0.025,
                            edge_alpha_fill=0.08, rim_alpha=0.45,
                            shape_note='Generator housings remain wholly outside the field (D-255); visible energy links run from their top lenses to the surface. Main plant and internal tubes are protected; external feeds are exposed.')
    config['shield']['generator_links'] = generator_links
    config['shield']['generator_link_style'] = dict(alpha=0.8, width_ref_px=2, dash_ref_px=[4, 2])
    for style in ('ring', 'ring_far', 'ring_weak'):
        config['shield'][style]['alpha'] = 0
    config['shield']['below_grid_opacity'] = 1
    config['shield']['hits']['impacts'] = []
    outputs[SCENE / 'assets/layout/scene_config.json'] = json_bytes(config)
    outputs[SCENE / 'assets/layout/ships_placement.json'] = json_bytes(dict(ship_plane_z=16, note='Module assembly showcase; no placeholder ships.', placements=[]))
    report = dict(module_types=len(used), placed_modules=len(layout['instances']), tube_runs=len(parts)-len(layout['instances']),
                  connections=len(layout['connections']), connected_instances=len(reached), visible_objects=len(parts),
                  physically_connected_objects=len(reached_geometry),
                  voxel_count=len(owners), carved_port_voxels=carved, shared_coupling_voxels=shared,
                  bounds=bounds(owners), glass_voxels=sum(c == 16 for voxels in parts.values() for c in voxels.values()),
                  shield_projectors=len(projectors), shield_covers_plant_and_internal_tubes=True,
                  shield_generators_outside=True, shield_generator_min_normalized_radius_squared=min(generator_clearances),
                  shield_emitter_boundary_max_error=max(emitter_errors), generator_energy_links=generator_links,
                  exposed_generator_feeds=sorted(external_feeds),
                  shield_emitter_contacts=emitter_contacts, shield_attachment='exposed_top_face',
                  shield_max_normalized_radius_squared=coverage,
                  main_plant_bounds=[plant_min, plant_max],
                  source_files=[m['file'] for m in inventory])
    outputs[SCENE / 'BuildReport.json'] = json_bytes(report)
    return outputs, report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    outputs, report = build()
    for path, data in outputs.items():
        if args.check:
            if not path.is_file() or path.read_bytes() != data:
                raise SystemExit(f'stale generated file: {path.relative_to(ROOT)}')
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    print(('Verified' if args.check else 'Built') + f' SpaceStation: {report["module_types"]} types, '
          f'{report["placed_modules"]} placements, {report["tube_runs"]} tube runs, {report["voxel_count"]:,} voxels')


if __name__ == '__main__':
    main()
