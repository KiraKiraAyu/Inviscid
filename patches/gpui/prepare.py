"""Materialize the pinned GPUI crate and local diff without changing Cargo caches."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tarfile
import tempfile
import time
import tomllib
from urllib.request import urlopen


ROOT = Path(__file__).resolve().parents[2]
VERSION = '0.2.2'
ARCHIVE_SHA256 = '979b45cfa6ec723b6f42330915a1b3769b930d02b2d505f9697f8ca602bee707'
STAMP = '.inviscid-patch.json'


class PreparationError(RuntimeError):
    """A source or patch failed validation; existing output is preserved."""


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def tree_hashes(root):
    hashes = {}
    for parent, directories, files in os.walk(root):
        directories[:] = sorted(name for name in directories if name not in ('target', '.git'))
        for name in sorted(files):
            path = Path(parent) / name
            relative = path.relative_to(root).as_posix()
            if relative not in (STAMP, 'Cargo.lock'):
                hashes[relative] = digest(path)
    return hashes


def extract_archive(archive, destination):
    """Extract regular crate files only, stripping the expected package prefix."""
    with tarfile.open(archive, 'r:gz') as source:
        members = source.getmembers()
        for member in members:
            path = PurePosixPath(member.name)
            if (not path.parts or path.parts[0] != f'gpui-{VERSION}'
                    or path.is_absolute() or '..' in path.parts
                    or '\\' in member.name or ':' in member.name
                    or not (member.isfile() or member.isdir())):
                raise PreparationError(f'Unsafe crate member: {member.name}')
        for member in members:
            relative = PurePosixPath(member.name).parts[1:]
            if not relative:
                continue
            target = destination.joinpath(*relative)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(member) as content, target.open('wb') as output:
                    shutil.copyfileobj(content, output)
                target.chmod(member.mode & 0o777)


def apply_patch(source, patch, ceiling):
    # Keep Git outside repository discovery even when staging inside this project.
    environment = dict(os.environ, GIT_CEILING_DIRECTORIES=str(ceiling.resolve()))
    for check in (True, False):
        command = ['git', '-c', 'core.autocrlf=false', 'apply', '--no-index', '--whitespace=nowarn']
        if check:
            command.append('--check')
        result = subprocess.run(command + [str(patch)], cwd=source, env=environment,
                                capture_output=True, text=True)
        if result.returncode:
            raise PreparationError(f'GPUI patch failed:\n{result.stderr.strip()}')


def rename_directory(source, destination):
    """Allow brief Windows indexer/antivirus handles to release the directory."""
    for attempt in range(6):
        try:
            source.rename(destination)
            return
        except PermissionError:
            if os.name != 'nt' or attempt == 5:
                raise
            time.sleep(0.1)


def prepare(destination, patch, archive):
    """Return whether a verified source tree was created or regenerated."""
    destination = Path(destination).absolute()
    if destination.is_symlink():
        raise PreparationError(f'Refusing a symlink destination: {destination}')
    if digest(archive) != ARCHIVE_SHA256:
        raise PreparationError(f'GPUI archive SHA-256 mismatch: {archive}')
    # Normalize checkout line endings; git apply always receives the canonical diff.
    patch_text = patch.read_text(encoding='utf-8')
    patch_hash = hashlib.sha256(patch_text.encode('utf-8')).hexdigest()
    if destination.exists():
        try:
            stamp = json.loads((destination / STAMP).read_text(encoding='utf-8'))
            expected = stamp['files']
        except (OSError, ValueError, KeyError) as error:
            raise PreparationError(f'Unmanaged GPUI directory: {destination}') from error
        if tree_hashes(destination) != expected:
            raise PreparationError(f'Generated GPUI has local changes; preserve them before preparing: {destination}')
        if stamp.get('archive_sha256') == ARCHIVE_SHA256 and stamp.get('patch_sha256') == patch_hash:
            return False

    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.gpui-', dir=destination.parent) as temporary:
        staging = Path(temporary).resolve()
        if not staging.is_relative_to(destination.parent.resolve()):
            raise PreparationError('Staging directory is outside the dependency directory')
        source = staging / 'source'
        source.mkdir()
        extract_archive(archive, source)
        manifest = tomllib.loads((source / 'Cargo.toml').read_text(encoding='utf-8'))
        if (manifest['package']['name'], manifest['package']['version']) != ('gpui', VERSION):
            raise PreparationError('Crate manifest does not match the pinned GPUI version')
        staged_patch = staging / 'local.patch'
        staged_patch.write_text(patch_text, encoding='utf-8', newline='\n')
        apply_patch(source, staged_patch, staging)
        stamp = {'archive_sha256': ARCHIVE_SHA256, 'patch_sha256': patch_hash,
                 'files': tree_hashes(source)}
        (source / STAMP).write_text(json.dumps(stamp, indent=2) + '\n', encoding='utf-8')
        previous = staging / 'previous'
        if destination.exists():
            rename_directory(destination, previous)
        try:
            rename_directory(source, destination)
        except OSError:
            if previous.exists():
                rename_directory(previous, destination)
            raise
    return True


def find_archive(explicit, offline):
    if explicit is not None:
        return explicit.resolve()
    filename = f'gpui-{VERSION}.crate'
    cargo_root = Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo')).expanduser()
    local_cache = ROOT / 'target/dependency-archives'
    candidates = list((cargo_root / 'registry/cache').glob(f'*/{filename}'))
    if (local_cache / filename).is_file():
        candidates.append(local_cache / filename)
    for candidate in candidates:
        if digest(candidate) == ARCHIVE_SHA256:
            return candidate
    if offline:
        raise PreparationError('Verified GPUI archive not cached; supply --archive PATH or prepare once online')
    local_cache.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='gpui-', dir=local_cache) as temporary:
        downloaded = Path(temporary) / filename
        url = f'https://static.crates.io/crates/gpui/{filename}'
        with urlopen(url, timeout=30) as response, downloaded.open('wb') as output:
            total = 0
            while chunk := response.read(1024 * 1024):
                total += len(chunk)
                if total > 32 * 1024 * 1024:
                    raise PreparationError('GPUI archive exceeds the expected download size limit')
                output.write(chunk)
        if digest(downloaded) != ARCHIVE_SHA256:
            raise PreparationError('Downloaded GPUI archive SHA-256 mismatch')
        downloaded.replace(local_cache / filename)
    return local_cache / filename


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, help='Use this pinned .crate archive')
    parser.add_argument('--offline', action='store_true', help='Require an existing archive')
    arguments = parser.parse_args()
    try:
        archive = find_archive(arguments.archive, arguments.offline)
        changed = prepare(ROOT / 'vendor/gpui', Path(__file__).with_name('windows-frame-upload.patch'), archive)
    except (PreparationError, OSError, tarfile.TarError, ValueError, KeyError) as error:
        parser.exit(1, f'GPUI preparation failed: {error}\n')
    print('GPUI prepared.' if changed else 'GPUI already prepared; source hashes verified.')


if __name__ == '__main__':
    main()
