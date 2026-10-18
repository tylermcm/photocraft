import tempfile
import unittest
from pathlib import Path
from audit_camera_raw_matrix import contains, guard, navigation_matches, plans, profile, read, sha, signature, union_area


class MatrixTests(unittest.TestCase):
    def test_all_references_and_specs_have_one_consistent_registration(self):
        specs = Path(__file__).resolve().parent.parent/'design/camera-raw'
        jobs = plans(specs, read(specs/'reference-inventory.json'))
        self.assertEqual([j['image'] for j in jobs], list(range(1, 20)))
        self.assertEqual(sum(len(j['audits']) for j in jobs), len(list(specs.glob('*element-regions.json'))))
        with self.assertRaises(ValueError):
            signature([0, 0, 100, 100], [0, 0, 100, 110])

    def test_merged_profile_preserves_the_accepted_menu_design(self):
        specs = Path(__file__).resolve().parent.parent/'design/camera-raw'
        layout, _ = profile(specs)
        self.assertEqual(layout['settingsMenu'], read(specs/'layout.json')['settingsMenu'])
        self.assertEqual(layout['masking'], read(specs/'masking-design.json'))

    def test_fit_navigation_alias_is_checked_without_accepting_a_wrong_zoom(self):
        self.assertTrue(navigation_matches({'zoom': None, 'center': [0.5, 0.5]}, {'zoom': 'fit'}))
        self.assertFalse(navigation_matches({'zoom': 1.0}, {'zoom': 'fit'}))
        self.assertTrue(navigation_matches({'zoom': 0.365552335}, {'zoom': 0.36555232558139533}))
        self.assertFalse(navigation_matches({'zoom': None}, {'zoom': 0.36555232558139533}))

    def test_changed_input_is_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder)/'input.json'
            path.write_text('{}', encoding='utf-8')
            pins = {path: sha(path)}
            guard(pins)
            path.write_text('{"changed":true}', encoding='utf-8')
            with self.assertRaises(RuntimeError):
                guard(pins)

    def test_overlapping_crops_are_not_double_counted(self):
        self.assertEqual(union_area([[0, 0, 10, 10], [5, 0, 15, 10]]), 150)
        self.assertEqual(union_area([[0, 0, 10, 10], [2, 2, 8, 8]]), 100)
        self.assertEqual(union_area([]), 0)

    def test_fragment_defaults_allow_extras_but_not_changed_array_elements(self):
        self.assertTrue(contains({'rows': [{'id': 'a', 'default': False}]}, {'rows': [{'id': 'a'}]}))
        self.assertFalse(contains({'rows': [{'id': 'b'}]}, {'rows': [{'id': 'a'}]}))
        self.assertFalse(contains({'rows': []}, {'rows': [{'id': 'a'}]}))


if __name__ == '__main__':
    unittest.main()
