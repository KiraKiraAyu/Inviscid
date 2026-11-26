"""Unit tests for scripts/generate-manifest.py."""

from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SCRIPT_PATH = Path(__file__).with_name("generate-manifest.py")
SPEC = importlib.util.spec_from_file_location("generate_manifest", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
generate_manifest = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(generate_manifest)


def write_cargo_toml(root: Path, version: str) -> Path:
    cargo_toml = root / "Cargo.toml"
    cargo_toml.write_text(
        f'[package]\nname = "inviscid"\nversion = "{version}"\nedition = "2024"\n',
        encoding="utf-8",
    )
    return cargo_toml


class GenerateManifestTests(unittest.TestCase):
    def test_multi_platform_manifest_and_sha256sums(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            cargo_toml = write_cargo_toml(root, "0.2.0")
            dist = root / "dist"
            dist.mkdir()

            win_bytes = b"PK\x03\x04windows-binary"
            mac_bytes = b"\x1f\x8b\x08macos-bundle"
            lin_bytes = b"\x1f\x8b\x08linux-binary"
            (dist / "inviscid-v0.2.0-windows-x86_64.zip").write_bytes(win_bytes)
            (dist / "inviscid-v0.2.0-macos-aarch64.tar.gz").write_bytes(mac_bytes)
            (dist / "inviscid-v0.2.0-linux-x86_64.tar.gz").write_bytes(lin_bytes)

            out_json = dist / "update-stable.json"
            out_sums = dist / "SHA256SUMS.txt"

            exit_code = generate_manifest.main(
                [
                    "--version",
                    "0.2.0",
                    "--artifacts-dir",
                    str(dist),
                    "--output",
                    str(out_json),
                    "--cargo-toml",
                    str(cargo_toml),
                    "--pub-date",
                    "2026-12-01T10:00:00Z",
                    "--sha256sums-out",
                    str(out_sums),
                ]
            )
            self.assertEqual(exit_code, 0)

            manifest = json.loads(out_json.read_text(encoding="utf-8"))
            self.assertEqual(manifest["schema_version"], 1)
            self.assertEqual(manifest["channel"], "stable")
            self.assertEqual(manifest["version"], "0.2.0")
            self.assertEqual(manifest["tag"], "v0.2.0")
            self.assertEqual(manifest["pub_date"], "2026-12-01T10:00:00Z")
            self.assertEqual(
                manifest["release_notes_url"],
                "https://github.com/KiraKiraAyu/Inviscid/releases/tag/v0.2.0",
            )
            self.assertNotIn("min_updater_version", manifest)
            self.assertEqual(
                manifest["targets"]["x86_64-pc-windows-msvc"],
                {
                    "asset_name": "inviscid-v0.2.0-windows-x86_64.zip",
                    "size": len(win_bytes),
                    "sha256": hashlib.sha256(win_bytes).hexdigest(),
                    "format": "zip",
                    "entry": "inviscid.exe",
                },
            )
            self.assertEqual(
                manifest["targets"]["aarch64-apple-darwin"]["entry"],
                "Inviscid.app",
            )
            self.assertEqual(
                manifest["targets"]["x86_64-unknown-linux-gnu"]["format"],
                "tar.gz",
            )

            sums_lines = out_sums.read_text(encoding="utf-8").splitlines()
            self.assertEqual(
                sums_lines,
                [
                    f"{hashlib.sha256(lin_bytes).hexdigest()}  inviscid-v0.2.0-linux-x86_64.tar.gz",
                    f"{hashlib.sha256(mac_bytes).hexdigest()}  inviscid-v0.2.0-macos-aarch64.tar.gz",
                    f"{hashlib.sha256(win_bytes).hexdigest()}  inviscid-v0.2.0-windows-x86_64.zip",
                ],
            )

    def test_rejects_cargo_version_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            cargo_toml = write_cargo_toml(root, "0.1.0")
            dist = root / "dist"
            dist.mkdir()
            (dist / "inviscid-v0.2.0-windows-x86_64.zip").write_bytes(b"zip")

            exit_code = generate_manifest.main(
                [
                    "--version",
                    "0.2.0",
                    "--artifacts-dir",
                    str(dist),
                    "--output",
                    str(root / "update-stable.json"),
                    "--cargo-toml",
                    str(cargo_toml),
                ]
            )
            self.assertNotEqual(exit_code, 0)

    def test_prerelease_channel_inference_and_stable_rejection(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            cargo_toml = write_cargo_toml(root, "0.2.0-beta.1")
            dist = root / "dist"
            dist.mkdir()
            (dist / "inviscid-v0.2.0-beta.1-linux-x86_64.tar.gz").write_bytes(b"tgz")

            out_beta = root / "update-beta.json"
            exit_code = generate_manifest.main(
                [
                    "--version",
                    "0.2.0-beta.1",
                    "--artifacts-dir",
                    str(dist),
                    "--output",
                    str(out_beta),
                    "--cargo-toml",
                    str(cargo_toml),
                ]
            )
            self.assertEqual(exit_code, 0)
            manifest = json.loads(out_beta.read_text(encoding="utf-8"))
            self.assertEqual(manifest["channel"], "beta")

            bad_code = generate_manifest.main(
                [
                    "--version",
                    "0.2.0-beta.1",
                    "--channel",
                    "stable",
                    "--artifacts-dir",
                    str(dist),
                    "--output",
                    str(root / "bad.json"),
                    "--cargo-toml",
                    str(cargo_toml),
                ]
            )
            self.assertNotEqual(bad_code, 0)

    def test_rejects_empty_dir_and_zero_byte_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            cargo_toml = write_cargo_toml(root, "0.2.0")
            dist = root / "dist"
            dist.mkdir()

            empty_code = generate_manifest.main(
                [
                    "--version",
                    "0.2.0",
                    "--artifacts-dir",
                    str(dist),
                    "--output",
                    str(root / "out.json"),
                    "--cargo-toml",
                    str(cargo_toml),
                ]
            )
            self.assertNotEqual(empty_code, 0)

            (dist / "inviscid-v0.2.0-windows-x86_64.zip").write_bytes(b"")
            zero_code = generate_manifest.main(
                [
                    "--version",
                    "0.2.0",
                    "--artifacts-dir",
                    str(dist),
                    "--output",
                    str(root / "out.json"),
                    "--cargo-toml",
                    str(cargo_toml),
                ]
            )
            self.assertNotEqual(zero_code, 0)

    def test_notes_file_and_min_updater_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            cargo_toml = write_cargo_toml(root, "0.3.0")
            dist = root / "dist"
            dist.mkdir()
            (dist / "inviscid-v0.3.0-macos-x86_64.tar.gz").write_bytes(b"bundle")

            notes_file = root / "notes.json"
            notes_file.write_text(
                json.dumps({"en": "Faster startup", "zh-Hans": "启动速度提升"}, ensure_ascii=False),
                encoding="utf-8",
            )
            out_json = root / "update-stable.json"
            exit_code = generate_manifest.main(
                [
                    "--version",
                    "0.3.0",
                    "--artifacts-dir",
                    str(dist),
                    "--output",
                    str(out_json),
                    "--cargo-toml",
                    str(cargo_toml),
                    "--min-updater-version",
                    "0.2.0",
                    "--notes-file",
                    str(notes_file),
                ]
            )
            self.assertEqual(exit_code, 0)
            manifest = json.loads(out_json.read_text(encoding="utf-8"))
            self.assertEqual(manifest["min_updater_version"], "0.2.0")
            self.assertEqual(
                manifest["notes"],
                {"en": "Faster startup", "zh-Hans": "启动速度提升"},
            )


if __name__ == "__main__":
    unittest.main()
