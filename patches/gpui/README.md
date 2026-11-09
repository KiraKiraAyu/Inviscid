# GPUI Windows Patches

This directory contains an ordered patch series and reproducible preparation script for [`gpui`](https://crates.io/crates/gpui) (`0.2.2`).

The materialized source in `vendor/gpui` is gitignored and treated as an ephemeral build artifact consumed locally by `Cargo.toml`.

## 1. Quick Start

### Prerequisites

- Python 3.11+
- Git
- Standard Inviscid build toolchain (Rust 1.99+)

### Bootstrap & Build

Run the preparation script from the workspace root before your initial build or whenever the patch changes:

```sh
# Fetch/extract upstream gpui 0.2.2, verify SHA-256, and apply patch
python patches/gpui/prepare.py

# Build and run Inviscid
cargo run --release
```

### Offline / Air-Gapped Environments

If the `.crate` archive is already present in Cargo's local cache or network access is restricted:

```sh
# Reuse Cargo cache without internet access
python patches/gpui/prepare.py --offline

# Or supply an explicit archive path
python patches/gpui/prepare.py --archive /path/to/gpui-0.2.2.crate
```

### Self-Test Preparation Tool

```sh
python -m unittest discover -s patches/gpui -p test_prepare.py
```

## 2. Problem & Root Cause

In text-intensive documents spanning multiple font atlas textures (common in Live Preview and Markdown with mixed scripts, math, and code blocks), upstream GPUI's DirectX 11 renderer re-maps the dynamic instance buffer for **every primitive draw batch**.

A single preview frame often triggered **~150 D3D11 `Map()` / `Unmap()` calls**. The driver synchronization overhead dominated frame submission time, creating noticeable micro-stutter and high P99 frame times during continuous scrolling.

## 3. Architecture & Patch Details

`windows-frame-upload.patch` restructures instance buffer synchronization in `src/platform/windows/directx_renderer.rs`:

1. **Single Frame Upload**:
   All monochrome sprites and quad primitives across the entire scene are uploaded to GPU memory in **one** contiguous map per primitive type per frame, prior to the batch loop.
2. **Sub-Range StructuredBuffer Views (SRVs)**:
   Individual draw batches retain their exact composite order, pipeline state, and atlas bindings. Rather than slicing the buffer via repeated uploads, each batch binds an SRV configured with `FirstElement` pointing to its slice offset.
3. **Zero Shader Changes**:
   Because `FirstElement` offsets the view base, vertex shaders continue indexing `SV_InstanceID` starting from zero. No HLSL or `DrawInstanced` parameter changes are required.
4. **Bounded SRV Cache**:
   SRV descriptors are cached per pipeline and bounded to 1,024 entries. Views are automatically invalidated if the dynamic instance buffer is reallocated to accommodate larger scenes.
5. **Isolation**:
   Shadows, paths, and custom pipeline stages remain untouched on their original execution paths.

## 4. Benchmark & Validation

All tests and measurements were conducted on Windows (2026-11-08) comparing default GPUI vs. patched GPUI under identical workloads (600 frames/run, fixed tail scene of a long document with CJK, styled formatting, emoji, and LaTeX math).

### Submission Latency & Resource Utilization

| Metric                           | Upstream GPUI | Patched GPUI  | Delta      |
| :------------------------------- | :------------ | :------------ | :--------- |
| **D3D11 Buffer Maps / Frame**    | 149           | **3**         | **-98.0%** |
| **Anchor Scroll Submission P99** | 1.857 ms      | **0.142 ms**  | **-92.3%** |
| **Wheel Scroll Submission P99**  | 2.626 ms      | **0.811 ms**  | **-69.1%** |
| **Peak Working Set (Anchor)**    | 293.8 MiB     | **189.0 MiB** | **-35.7%** |
| **Peak Working Set (Wheel)**     | 341.4 MiB     | **224.6 MiB** | **-34.2%** |

> **Note on P95 latency**: Wheel scroll P95 increased slightly from 0.317 ms to 0.586 ms due to initial cold-cache SRV creation, which stabilizes once the view cache warms up.

### Correctness & Test Suite

- **Pixel Equivalence**: Byte-for-byte identical BGRA frame buffer outputs across 8 distinct layout tests (Source Mode, Live Preview, CJK typography, styled headings, emoji, math rendering).
- **Inviscid Test Suite**: All 572 application unit and integration tests pass cleanly.
- **Partition Invariant Tests**: 6 dedicated scene partition tests added to GPUI's test suite to guarantee slice order, gapless iteration, and tile-ID collision handling:
  ```sh
  cargo test --manifest-path vendor/gpui/Cargo.toml --target-dir target --lib --features test-support mono_upload_tests -j 1 --config profile.dev.debug=0 -- --test-threads=1
  ```

## 5. IME Key Ownership

`windows-ime-input.patch` fixes the Windows input boundary independently of the renderer patch.
Upstream GPUI recovers the original key from `VK_PROCESSKEY` using `ImmGetVirtualKey` and can
dispatch it as an application action after the IME has already cleared or committed its preedit.
For example, cancelling the final preedit letter with Backspace can also delete committed text.

The patch routes processed keys to `TranslateMessage` before decoding application actions,
with the original message unchanged and without holding a window-state borrow during translation.
It also prevents processed keys from becoming application KeyUp/system-key events. Active
compositions retain the existing platform ownership behavior. No editor clock, timeout, or
pending swallow token is involved, so the next independent key is handled normally.

The Win32 contract is documented in [ImmGetVirtualKey](https://learn.microsoft.com/en-us/windows/win32/api/imm/nf-imm-immgetvirtualkey)
and [WM_KEYDOWN](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-keydown).

Four platform regression tests cover processed keys after cancellation/commit, active
composition editing/navigation, and the next independent key. Application tests cover both
cancellation APIs, explicit/fallback UTF-16 ranges, CJK/emoji, and normal Backspace after cancellation.
These simulate the input contract; they do not substitute for a manual Rime session.

```powershell
python patches/gpui/prepare.py --offline
# Reuse the application's resolved versions for standalone GPUI tests. This generated
# lockfile is intentionally excluded from the source integrity stamp.
Copy-Item Cargo.lock vendor/gpui/Cargo.lock
cargo test --offline --manifest-path vendor/gpui/Cargo.toml --target-dir target --lib --features test-support ime_key_ownership_tests -j 1 --config profile.dev.debug=0 -- --test-threads=1
cargo test --offline --lib ime_ -j 1 --config profile.dev.debug=0 -- --test-threads=1
```

## 6. Maintenance & Upstream Strategy

- **Single Source of Truth**: Never edit `vendor/gpui/` manually. `prepare.py` records cryptographic checksums in `.inviscid-patch.json` and will refuse to run against a modified or unmanaged directory. Modify the relevant tracked patch, then regenerate. The ordered series is hashed and applied atomically; a failed patch leaves the existing tree intact.
- **Minimal Surface**: Keep the DirectX upload and Windows IME changes in separate patches with their own regression tests.
- **Exit Criteria**: Remove each patch when the pinned upstream release contains its equivalent fix. Remove the preparation tooling and Cargo override once neither patch is needed.
