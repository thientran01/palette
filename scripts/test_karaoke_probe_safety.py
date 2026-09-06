import tempfile
import unittest
from pathlib import Path
from research.karaoke_probe_safety import write_new_json, validate_disjoint_windows

class ProbeSafetyTests(unittest.TestCase):
    def test_existing_evidence_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'meta.json'
            path.write_text('original evidence', encoding='utf-8')
            with self.assertRaises(FileExistsError):
                write_new_json(path, {'replacement': True})
            self.assertEqual(path.read_text(encoding='utf-8'), 'original evidence')

    def test_adjacent_row_margins_cannot_overwrite_each_other(self):
        with self.assertRaises(ValueError):
            validate_disjoint_windows([(500, 2500), (1500, 3500)])
        validate_disjoint_windows([(500, 2500), (2500, 3500)])

if __name__ == '__main__':
    unittest.main()