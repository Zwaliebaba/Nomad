"""Regression checks for shield attachment and coverage on the actual assembly."""
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('station_scene', Path(__file__).with_name('build.py'))
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


class ShieldAssemblyTests(unittest.TestCase):
    def altered(self, **shield):
        layout = json.loads((builder.SCENE / 'layout.json').read_text())
        layout['shield'].update(shield)
        return layout

    def build_layout(self, layout):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'layout.json').write_text(json.dumps(layout))
            with patch.object(builder, 'SCENE', root):
                return builder.build()

    def test_external_generators_link_to_field_and_plant_is_covered(self):
        outputs, report = self.build_layout(self.altered())
        self.assertEqual(report['shield_projectors'], 4)
        self.assertTrue(report['shield_generators_outside'])
        self.assertGreater(report['shield_generator_min_normalized_radius_squared'], 1)
        self.assertEqual(len(report['exposed_generator_feeds']), 4)
        self.assertEqual(report['shield_emitter_boundary_max_error'], 0)
        self.assertTrue(report['shield_covers_plant_and_internal_tubes'])
        self.assertLessEqual(report['shield_max_normalized_radius_squared'], 1)
        self.assertEqual(report['physically_connected_objects'], report['visible_objects'])
        self.assertTrue(all(point[2] == 61 for point in report['shield_emitter_contacts'].values()))
        config = json.loads(next(data for path, data in outputs.items() if path.name == 'scene_config.json'))
        for style in ('ring', 'ring_far', 'ring_weak'):
            self.assertEqual(config['shield'][style]['alpha'], 0)
        self.assertEqual(config['shield']['below_grid_opacity'], 1)

    def test_a_buried_core_is_not_a_top_emitter(self):
        with self.assertRaisesRegex(ValueError, 'exposed top face'):
            self.build_layout(self.altered(emitter_local=[0, 0, 46], emitter_height=47,
                emitter_contact_local=[0.5, 0.5, 47], center=[0.5, 0.5, 47]))

    def test_a_partly_protected_generator_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'housing must be entirely outside'):
            self.build_layout(self.altered(radius_xy=341))

    def test_a_field_beyond_the_towers_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'outside the field'):
            self.build_layout(self.altered(radius_xy=360))

    def test_a_field_that_leaves_part_of_the_station_outside_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'does not cover the plant and tubes'):
            self.build_layout(self.altered(radius_z=60))

    def test_attachment_must_be_actual_emissive_geometry(self):
        with self.assertRaisesRegex(ValueError, 'actual Emit voxel'):
            self.build_layout(self.altered(emitter_local=[0, 0, 22], emitter_height=22, center=[0, 0, 22]))


if __name__ == '__main__':
    unittest.main()
