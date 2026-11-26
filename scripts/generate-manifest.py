#!/usr/bin/env python3
"""Generate and validate an Inviscid auto-update manifest (schema v1)."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import sys
import tomllib

SCHEMA_VERSION = 1
DEFAULT_REPO = "KiraKiraAyu/Inviscid"
ALLOWED_CHANNELS = ("stable", "beta")

# Strict SemVer 2.0.0 pattern (major.minor.patch[-prerelease][+build])
SEMVER_RE = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)"
    r"(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?"
    r"(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$"
)

TARGET_SPECS: tuple[tuple[str, str, str, str], ...] = (
    ("windows-x86_64.zip", "x86_64-pc-windows-msvc", "zip", "inviscid.exe"),
    ("windows-aarch64.zip", "aarch64-pc-windows-msvc", "zip", "inviscid.exe"),
    ("macos-aarch64.tar.gz", "aarch64-apple-darwin", "tar.gz", "Inviscid.app"),
    ("macos-x86_64.tar.gz", "x86_64-apple-darwin", "tar.gz", "Inviscid.app"),
    ("linux-x86_64.tar.gz", "x86_64-unknown-linux-gnu", "tar.gz", "inviscid"),
    ("linux-aarch64.tar.gz", "aarch64-unknown-linux-gnu", "tar.gz", "inviscid"),
)


class ManifestError(ValueError):
    """Raised when release inputs, artifacts, or version metadata are invalid."""


def validate_semver(value: str, label: str) -> str:
    if value.startswith("v"):
        raise ManifestError(f"{label} must not have a leading 'v': {value!r}")
    if not SEMVER_RE.match(value):
        raise ManifestError(f"{label} is not valid SemVer 2.0.0: {value!r}")
    return value


def resolve_channel(version: str, requested_channel: str | None) -> str:
    is_prerelease = "-" in version
    if requested_channel is None:
        return "beta" if is_prerelease else "stable"
    if requested_channel not in ALLOWED_CHANNELS:
        raise ManifestError(
            f"Unsupported channel {requested_channel!r}; expected one of {ALLOWED_CHANNELS}"
        )
    if is_prerelease and requested_channel == "stable":
        raise ManifestError(
            f"Refusing to publish prerelease version {version!r} to 'stable' channel"
        )
    return requested_channel


def verify_cargo_version(cargo_toml: Path, version: str) -> None:
    if not cargo_toml.is_file():
        raise ManifestError(f"Cargo.toml not found: {cargo_toml}")
    data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    package_version = data.get("package", {}).get("version")
    if package_version != version:
        raise ManifestError(
            f"Cargo.toml version ({package_version!r}) does not match release version ({version!r})"
        )


def load_notes(notes_file: Path | None) -> dict[str, str]:
    if notes_file is None:
        return {}
    if not notes_file.is_file():
        raise ManifestError(f"Notes file not found: {notes_file}")
    raw = json.loads(notes_file.read_text(encoding="utf-8"))
    if not isinstance(raw, dict) or not all(
        isinstance(k, str) and isinstance(v, str) for k, v in raw.items()
    ):
        raise ManifestError("Notes file must be a JSON object mapping locale strings to text")
    return dict(raw)


def sha256_file(path: Path) -> tuple[int, str]:
    size = path.stat().st_size
    if size <= 0:
        raise ManifestError(f"Artifact must not be empty (0 bytes): {path}")
    with path.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest().lower()
    if len(digest) != 64:
        raise ManifestError(f"Unexpected SHA-256 digest length for {path}")
    return size, digest


def build_manifest(
    *,
    version: str,
    tag: str,
    channel: str | None,
    artifacts_dir: Path,
    cargo_toml: Path | None,
    repo: str,
    min_updater_version: str | None,
    pub_date: str | None,
    notes_file: Path | None,
) -> tuple[dict[str, object], list[tuple[str, str]]]:
    validate_semver(version, "--version")
    resolved_channel = resolve_channel(version, channel)
    if min_updater_version is not None:
        validate_semver(min_updater_version, "--min-updater-version")

    if cargo_toml is not None:
        verify_cargo_version(cargo_toml, version)

    if not artifacts_dir.is_dir():
        raise ManifestError(f"Artifacts directory does not exist: {artifacts_dir}")

    targets: dict[str, dict[str, object]] = {}
    checksums: list[tuple[str, str]] = []

    for suffix, triple, fmt, entry in TARGET_SPECS:
        asset_name = f"inviscid-{tag}-{suffix}"
        candidate = artifacts_dir / asset_name
        if not candidate.exists():
            continue
        if not candidate.is_file() or candidate.is_symlink():
            raise ManifestError(f"Artifact must be a regular non-symlink file: {candidate}")
        size, digest = sha256_file(candidate)
        targets[triple] = {
            "asset_name": asset_name,
            "size": size,
            "sha256": digest,
            "format": fmt,
            "entry": entry,
        }
        checksums.append((digest, asset_name))

    if not targets:
        raise ManifestError(
            f"No matching release archives found in {artifacts_dir} for tag {tag!r}"
        )

    effective_pub_date = pub_date or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    manifest: dict[str, object] = {
        "schema_version": SCHEMA_VERSION,
        "channel": resolved_channel,
        "version": version,
        "tag": tag,
        "pub_date": effective_pub_date,
    }
    if min_updater_version is not None:
        manifest["min_updater_version"] = min_updater_version
    manifest["release_notes_url"] = f"https://github.com/{repo}/releases/tag/{tag}"
    manifest["notes"] = load_notes(notes_file)
    manifest["targets"] = targets

    checksums.sort(key=lambda item: item[1])
    return manifest, checksums


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True, help="SemVer version without leading 'v'")
    parser.add_argument("--tag", help="Git release tag (defaults to v<version>)")
    parser.add_argument("--channel", choices=ALLOWED_CHANNELS, help="Override release channel")
    parser.add_argument("--artifacts-dir", type=Path, required=True, help="Directory with release archives")
    parser.add_argument("--output", type=Path, required=True, help="Output manifest JSON path")
    parser.add_argument(
        "--cargo-toml",
        type=Path,
        default=Path("Cargo.toml"),
        help="Cargo.toml path to cross-check package version (default: Cargo.toml)",
    )
    parser.add_argument("--repo", default=DEFAULT_REPO, help="GitHub owner/repo slug")
    parser.add_argument("--min-updater-version", help="Minimum updater SemVer before forcing manual upgrade")
    parser.add_argument("--pub-date", help="ISO-8601 UTC publish timestamp")
    parser.add_argument("--notes-file", type=Path, help="Optional JSON file with localized release notes")
    parser.add_argument("--sha256sums-out", type=Path, help="Optional path to write SHA256SUMS.txt")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    tag = args.tag or f"v{args.version}"
    try:
        manifest, checksums = build_manifest(
            version=args.version,
            tag=tag,
            channel=args.channel,
            artifacts_dir=args.artifacts_dir,
            cargo_toml=args.cargo_toml,
            repo=args.repo,
            min_updater_version=args.min_updater_version,
            pub_date=args.pub_date,
            notes_file=args.notes_file,
        )
    except (ManifestError, OSError, ValueError, tomllib.TOMLDecodeError) as err:
        print(f"error: {err}", file=sys.stderr)
        return 1

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )

    if args.sha256sums_out is not None:
        args.sha256sums_out.parent.mkdir(parents=True, exist_ok=True)
        lines = [f"{digest}  {name}\n" for digest, name in checksums]
        args.sha256sums_out.write_text("".join(lines), encoding="utf-8", newline="\n")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
