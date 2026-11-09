"""Behavioral checks for reproducible, non-destructive GPUI preparation."""
import hashlib
import io
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import prepare as subject


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='gpui-prepare-test-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.destination = self.root / 'vendor' / 'gpui'
        self.archive = self.make_archive()
        self.patch = self.make_patch(2)
        checksum = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.checksum_patch = patch.object(subject, 'ARCHIVE_SHA256', checksum)
        self.checksum_patch.start()
        self.addCleanup(self.checksum_patch.stop)

    def make_archive(self, extra=(), filename='gpui.crate'):
        archive = self.root / filename
        members = [
            ('gpui-0.2.2/Cargo.toml', b'[package]\nname = "gpui"\nversion = "0.2.2"\n'),
            ('gpui-0.2.2/src/lib.rs', b'pub const VALUE: u8 = 1;\n'),
            ('gpui-0.2.2/LICENSE-APACHE', b'Fixture license\n'),
        ]
        with tarfile.open(archive, 'w:gz') as stream:
            for name, content in members:
                member = tarfile.TarInfo(name)
                member.size = len(content)
                stream.addfile(member, io.BytesIO(content))
            for member in extra:
                stream.addfile(member)
        return archive

    def make_patch(self, value, filename='fix.patch'):
        path = self.root / filename
        path.write_bytes((
            'diff --git a/src/lib.rs b/src/lib.rs\n'
            '--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n'
            '-pub const VALUE: u8 = 1;\n'
            f'+pub const VALUE: u8 = {value};\n'
        ).encode())
        return path

    def prepare(self, patch_path=None):
        return subject.prepare(self.destination, [patch_path or self.patch], self.archive)

    def test_patch_series_is_ordered_and_reproducible(self):
        second = self.make_patch(3, 'second.patch')
        second.write_bytes(second.read_bytes().replace(b'-pub const VALUE: u8 = 1;',
                                                       b'-pub const VALUE: u8 = 2;'))
        patches = [self.patch, second]
        self.assertTrue(subject.prepare(self.destination, patches, self.archive))
        self.assertEqual((self.destination / 'src/lib.rs').read_bytes(),
                         b'pub const VALUE: u8 = 3;\n')
        self.assertFalse(subject.prepare(self.destination, patches, self.archive))
        before = self.contents()
        with self.assertRaises(subject.PreparationError):
            subject.prepare(self.destination, list(reversed(patches)), self.archive)
        self.assertEqual(self.contents(), before)

        second.write_bytes(second.read_bytes().replace(b'+pub const VALUE: u8 = 3;',
                                                       b'+pub const VALUE: u8 = 4;'))
        self.assertTrue(subject.prepare(self.destination, patches, self.archive))
        self.assertEqual((self.destination / 'src/lib.rs').read_bytes(),
                         b'pub const VALUE: u8 = 4;\n')

    def test_failed_second_patch_preserves_existing_output(self):
        self.prepare()
        before = self.contents()
        conflicting = self.make_patch(3, 'conflicting.patch')
        with self.assertRaises(subject.PreparationError):
            subject.prepare(self.destination, [self.patch, conflicting], self.archive)
        self.assertEqual(self.contents(), before)

    def contents(self):
        return {path.relative_to(self.destination): path.read_bytes()
                for path in self.destination.rglob('*') if path.is_file()}

    def test_prepares_verified_archive_with_real_git_patch(self):
        self.assertTrue(self.prepare())
        self.assertEqual((self.destination / 'src/lib.rs').read_bytes(),
                         b'pub const VALUE: u8 = 2;\n')
        self.assertTrue((self.destination / 'Cargo.toml').is_file())
        self.assertTrue((self.destination / 'LICENSE-APACHE').is_file())
        self.assertFalse((self.destination / 'gpui-0.2.2').exists())

    def test_repeated_prepare_preserves_contents_and_timestamps(self):
        self.prepare()
        contents = self.contents()
        timestamps = {path: path.stat().st_mtime_ns
                      for path in self.destination.rglob('*') if path.is_file()}
        self.assertFalse(self.prepare())
        self.assertEqual(self.contents(), contents)
        self.assertEqual({path: path.stat().st_mtime_ns for path in timestamps}, timestamps)

    def test_new_patch_rebuilds_from_pristine_archive(self):
        self.prepare()
        self.assertTrue(self.prepare(self.make_patch(3, 'updated.patch')))
        self.assertEqual((self.destination / 'src/lib.rs').read_bytes(),
                         b'pub const VALUE: u8 = 3;\n')
        self.assertFalse(self.prepare(self.root / 'updated.patch'))

    def test_crlf_patch_is_equivalent_to_lf_checkout(self):
        self.prepare()
        source = self.destination / 'src/lib.rs'
        original_mtime = source.stat().st_mtime_ns
        crlf = self.root / 'windows-checkout.patch'
        crlf.write_bytes(self.patch.read_bytes().replace(b'\n', b'\r\n'))
        self.assertFalse(self.prepare(crlf))
        self.assertEqual(source.read_bytes(), b'pub const VALUE: u8 = 2;\n')
        self.assertEqual(source.stat().st_mtime_ns, original_mtime)

    def test_generated_cargo_lock_changes_do_not_require_reconstruction(self):
        self.prepare()
        lock = self.destination / 'Cargo.lock'
        lock.write_bytes(b'Generated standalone test dependency resolutions.\n')
        self.assertFalse(self.prepare())
        self.assertEqual(lock.read_bytes(),
                         b'Generated standalone test dependency resolutions.\n')

    def test_modified_generated_file_is_preserved_and_rejected(self):
        self.prepare()
        edited = self.destination / 'src/lib.rs'
        edited.write_bytes(b'Local work must survive.\n')
        with self.assertRaises(subject.PreparationError):
            self.prepare()
        self.assertEqual(edited.read_bytes(), b'Local work must survive.\n')

    def test_corrupt_archive_does_not_touch_existing_output(self):
        self.prepare()
        before = self.contents()
        self.archive.write_bytes(b'Not the pinned crate archive.\n')
        with self.assertRaises(subject.PreparationError):
            self.prepare()
        self.assertEqual(self.contents(), before)

    def test_failed_patch_preserves_existing_output(self):
        self.prepare()
        before = self.contents()
        bad = self.make_patch(3, 'bad.patch')
        bad.write_bytes(bad.read_bytes().replace(b'VALUE: u8 = 1', b'VALUE: u8 = 99'))
        with self.assertRaises(subject.PreparationError):
            self.prepare(bad)
        self.assertEqual(self.contents(), before)

    def test_failed_first_prepare_does_not_publish_partial_output(self):
        self.patch.write_bytes(b'Invalid unified patch.\n')
        with self.assertRaises(subject.PreparationError):
            self.prepare()
        self.assertFalse(self.destination.exists())

    @unittest.skipUnless(os.name == 'nt', 'Windows transient handle contention')
    def test_transient_windows_publication_error_is_retried(self):
        rename = Path.rename
        attempts = []

        def transient(source, destination):
            if source.name == 'source':
                attempts.append(source)
                if len(attempts) == 1:
                    raise PermissionError('Simulated transient Windows file handle')
            return rename(source, destination)

        with patch.object(Path, 'rename', transient), patch('time.sleep'):
            self.assertTrue(self.prepare())
        self.assertEqual(len(attempts), 2)
        self.assertEqual((self.destination / 'src/lib.rs').read_bytes(),
                         b'pub const VALUE: u8 = 2;\n')

    def test_persistent_publication_failure_restores_previous_tree(self):
        self.prepare()
        before = self.contents()
        updated = self.make_patch(3, 'updated.patch')
        rename = Path.rename

        def reject_publication(source, destination):
            if source.name == 'source':
                raise PermissionError('Simulated persistent publication failure')
            return rename(source, destination)

        with patch.object(Path, 'rename', reject_publication), patch('time.sleep'):
            with self.assertRaises((PermissionError, subject.PreparationError)):
                self.prepare(updated)
        self.assertEqual(self.contents(), before)

    def test_unmanaged_destination_is_preserved_and_rejected(self):
        self.destination.mkdir(parents=True)
        marker = self.destination / 'local.txt'
        marker.write_bytes(b'Unmanaged work\n')
        with self.assertRaises(subject.PreparationError):
            self.prepare()
        self.assertEqual(marker.read_bytes(), b'Unmanaged work\n')
        self.assertEqual(set(self.contents()), {Path('local.txt')})

    def test_extract_rejects_unsafe_paths_and_links(self):
        unsafe = ['../escape', '/absolute', 'gpui-0.2.2/../escape',
                  'gpui-0.2.2/src/../../escape', r'gpui-0.2.2\escape',
                  'C:/absolute', 'unrelated-package/file']
        for index, name in enumerate(unsafe):
            with self.subTest(name=name):
                archive = self.make_archive([tarfile.TarInfo(name)], f'bad-{index}.crate')
                with self.assertRaises(subject.PreparationError):
                    subject.extract_archive(archive, self.root / f'out-{index}')
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
            with self.subTest(link_type=kind):
                member = tarfile.TarInfo('gpui-0.2.2/link')
                member.type = kind
                member.linkname = '../outside'
                archive = self.make_archive([member], f'link-{kind!r}.crate')
                with self.assertRaises(subject.PreparationError):
                    subject.extract_archive(archive, self.root / f'link-out-{kind!r}')

    def test_custom_cargo_home_archive_is_used_offline_without_network(self):
        cargo_home = self.root / 'custom-cargo-home'
        cached = cargo_home / 'registry/cache/test-registry/gpui-0.2.2.crate'
        cached.parent.mkdir(parents=True)
        cached.write_bytes(self.archive.read_bytes())
        with patch.dict(os.environ, {'CARGO_HOME': str(cargo_home)}), \
                patch.object(subject, 'ROOT', self.root / 'isolated-project'), \
                patch.object(subject, 'urlopen') as network:
            self.assertEqual(subject.find_archive(None, True), cached)
            network.assert_not_called()

    def test_offline_cache_miss_never_downloads(self):
        with patch.dict(os.environ, {'CARGO_HOME': str(self.root / 'empty-cache')}), \
                patch.object(subject, 'ROOT', self.root / 'isolated-project'), \
                patch.object(subject, 'urlopen') as network:
            with self.assertRaises(subject.PreparationError):
                subject.find_archive(None, True)
            network.assert_not_called()

    def test_cli_forwards_explicit_archive_and_offline_flag(self):
        project = self.root / 'cli-project'
        with patch.object(subject, 'ROOT', project), \
                patch.object(sys, 'argv', ['prepare.py', '--archive', str(self.archive), '--offline']), \
                patch.object(subject, 'find_archive', return_value=self.archive) as locate, \
                patch.object(subject, 'prepare', return_value=True) as materialize, \
                patch('builtins.print'):
            subject.main()
        locate.assert_called_once_with(self.archive, True)
        materialize.assert_called_once_with(
            project / 'vendor/gpui',
            [Path(subject.__file__).with_name('windows-frame-upload.patch'),
             Path(subject.__file__).with_name('windows-ime-input.patch'),
             Path(subject.__file__).with_name('windows-text-baseline.patch')], self.archive)

    def test_explicit_archive_bypasses_cache_lookup_and_download(self):
        with patch.object(subject, 'urlopen') as network:
            self.assertEqual(subject.find_archive(self.archive, True), self.archive.resolve())
            network.assert_not_called()

    def test_download_is_verified_before_publishing_cache_entry(self):
        project = self.root / 'download-project'
        with patch.dict(os.environ, {'CARGO_HOME': str(self.root / 'empty-cache')}), \
                patch.object(subject, 'ROOT', project), \
                patch.object(subject, 'urlopen', return_value=io.BytesIO(self.archive.read_bytes())) as network:
            downloaded = subject.find_archive(None, False)
        self.assertEqual(downloaded, project / 'target/dependency-archives/gpui-0.2.2.crate')
        self.assertEqual(downloaded.read_bytes(), self.archive.read_bytes())
        network.assert_called_once_with(
            'https://static.crates.io/crates/gpui/gpui-0.2.2.crate', timeout=30)

    def test_download_checksum_failure_does_not_replace_cache(self):
        project = self.root / 'download-project'
        cached = project / 'target/dependency-archives/gpui-0.2.2.crate'
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b'Preexisting invalid cache entry\n')
        with patch.dict(os.environ, {'CARGO_HOME': str(self.root / 'empty-cache')}), \
                patch.object(subject, 'ROOT', project), \
                patch.object(subject, 'urlopen', return_value=io.BytesIO(b'Corrupt download\n')):
            with self.assertRaises(subject.PreparationError):
                subject.find_archive(None, False)
        self.assertEqual(cached.read_bytes(), b'Preexisting invalid cache entry\n')


if __name__ == '__main__':
    unittest.main()
