"""The installer unpacks only the pinned naist-jdic files, verified.

Run: python -m unittest discover -s scripts -p test_karaoke_install.py -v
"""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'install_karaoke_model', Path(__file__).with_name('install_karaoke_model.py'))
install = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(install)


def sha(data):
    return hashlib.sha256(data).hexdigest()


class DictionaryInstall(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.files = {'dict.da': b'trie', 'matrix.mtx': b'costs'}
        self.saved = (install.DICTIONARY, install.DICTIONARY_ARCHIVE)
        install.DICTIONARY = {name: sha(data) for name, data in self.files.items()}

    def tearDown(self):
        install.DICTIONARY, install.DICTIONARY_ARCHIVE = self.saved
        self.tmp.cleanup()

    def archive(self, entries):
        path = self.root / 'naist-jdic-jpreprocess.tar.gz'
        with tarfile.open(path, 'w:gz') as tar:
            for name, data in entries.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
        install.DICTIONARY_ARCHIVE = sha(path.read_bytes())
        return path

    def test_unpacks_only_the_pinned_files(self):
        entries = {f'naist-jdic/{n}': d for n, d in self.files.items()}
        entries['../escape.txt'] = b'nope'
        entries['naist-jdic/extra.bin'] = b'not pinned'
        dest = self.root / 'karaoke-model'
        dest.mkdir()
        install.install_dictionary(self.archive(entries), dest)
        got = {p.name: p.read_bytes() for p in (dest / 'naist-jdic').iterdir()}
        self.assertEqual(got, self.files)
        self.assertFalse((self.root / 'escape.txt').exists())
        self.assertEqual([p.name for p in dest.iterdir()], ['naist-jdic'])
        # A second run with the folder in place changes nothing.
        install.install_dictionary(self.archive(entries), dest)
        self.assertEqual({p.name for p in (dest / 'naist-jdic').iterdir()}, set(self.files))

    def test_refuses_an_unknown_archive(self):
        path = self.archive({f'naist-jdic/{n}': d for n, d in self.files.items()})
        install.DICTIONARY_ARCHIVE = '0' * 64
        dest = self.root / 'karaoke-model'
        dest.mkdir()
        with self.assertRaises(ValueError):
            install.install_dictionary(path, dest)
        self.assertEqual(list(dest.iterdir()), [])

    def test_a_wrong_file_leaves_the_old_folder(self):
        dest = self.root / 'karaoke-model'
        (dest / 'naist-jdic').mkdir(parents=True)
        (dest / 'naist-jdic' / 'dict.da').write_bytes(b'old')
        entries = {f'naist-jdic/{n}': d for n, d in self.files.items()}
        entries['naist-jdic/matrix.mtx'] = b'tampered'
        with self.assertRaises(ValueError):
            install.install_dictionary(self.archive(entries), dest)
        self.assertEqual((dest / 'naist-jdic' / 'dict.da').read_bytes(), b'old')
        self.assertEqual([p.name for p in dest.iterdir()], ['naist-jdic'])

    def test_pins_match_the_aligner(self):
        rust = Path(__file__).parent.parent / 'src-tauri' / 'src' / 'japanese.rs'
        text = rust.read_text(encoding='utf-8')
        for name, digest in self.saved[0].items():
            self.assertIn(f'"{name}",\n        "{digest}"', text)


if __name__ == '__main__':
    unittest.main()
