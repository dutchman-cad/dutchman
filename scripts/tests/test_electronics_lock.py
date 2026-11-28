"""Exercise packaging with synthetic module archives; no network or CAD software."""
import importlib.util
import io
import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

SPEC = importlib.util.spec_from_file_location('electronics', Path(__file__).parents[1] / 'electronics-lock.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ElectronicsLockTests(unittest.TestCase):
    def fetch(self, checksum=None, license=True, mismatch=False):
        data = io.BytesIO()
        with zipfile.ZipFile(data, 'w') as archive:
            archive.writestr('module/dutchman-electronics.exe', b'synthetic executable')
            if license:
                archive.writestr('module/LICENSE', b'synthetic license')
        data = data.getvalue()
        checksum = checksum or hashlib.sha256(data).hexdigest()
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            lock = root / 'lock'
            lock.write_text('[targets.windows]\nasset="test.zip"\nurl="https://example.test/test.zip"\nsha256="' + checksum + '"\n')
            with patch.object(MODULE, 'LOCK', lock), patch.object(MODULE, 'download', return_value=b'wrong' if mismatch else data), patch('sys.argv', ['script', 'fetch', 'windows', str(root / 'out')]):
                MODULE.main()
            self.assertEqual((root / 'out/dutchman-electronics.exe').read_bytes(), b'synthetic executable')
            self.assertTrue((root / 'out/THIRD-PARTY/dutchman-electronics/module/LICENSE').exists())

    def test_stages_program_and_license(self):
        self.fetch()

    def test_placeholder_fails_before_download(self):
        with self.assertRaisesRegex(ValueError, 'checksum is missing'):
            self.fetch(checksum='PLACEHOLDER')

    def test_mismatch_fails(self):
        with self.assertRaisesRegex(ValueError, 'differs'):
            self.fetch(mismatch=True)

    def test_missing_license_fails(self):
        with self.assertRaisesRegex(ValueError, 'license/notice'):
            self.fetch(license=False)
