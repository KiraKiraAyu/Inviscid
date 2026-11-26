#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 || $# -gt 4 ]]; then
    echo "Usage: $0 <binary-path> <version> <output-tar-gz> [svg-icon-path]" >&2
    exit 1
fi

binary_path="$1"
version="$2"
output_tar_gz="$3"
svg_icon_path="${4:-assets/images/Inviscid.svg}"

if [[ ! -f "$binary_path" ]]; then
    echo "error: binary not found: $binary_path" >&2
    exit 1
fi
if [[ -z "$version" ]]; then
    echo "error: version must not be empty" >&2
    exit 1
fi

# CFBundleVersion on macOS should be numeric/dot-separated; strip any prerelease suffix.
bundle_version="${version%%-*}"
bundle_version="${bundle_version%%+*}"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

app_dir="$tmp_dir/Inviscid.app"
contents_dir="$app_dir/Contents"
macos_dir="$contents_dir/MacOS"
resources_dir="$contents_dir/Resources"

mkdir -p "$macos_dir" "$resources_dir"
cp "$binary_path" "$macos_dir/inviscid"
chmod 755 "$macos_dir/inviscid"

icon_plist_entry=""
if [[ -n "$svg_icon_path" && -f "$svg_icon_path" ]] && command -v iconutil >/dev/null 2>&1; then
    iconset_dir="$tmp_dir/Inviscid.iconset"
    mkdir -p "$iconset_dir"
    if command -v rsvg-convert >/dev/null 2>&1; then
        for size in 16 32 128 256 512; do
            double=$((size * 2))
            rsvg-convert -w "$size" -h "$size" "$svg_icon_path" -o "$iconset_dir/icon_${size}x${size}.png"
            rsvg-convert -w "$double" -h "$double" "$svg_icon_path" -o "$iconset_dir/icon_${size}x${size}@2x.png"
        done
        if iconutil -c icns "$iconset_dir" -o "$resources_dir/Inviscid.icns"; then
            icon_plist_entry=$'    <key>CFBundleIconFile</key>\n    <string>Inviscid</string>'
        fi
    elif command -v qlmanage >/dev/null 2>&1 && command -v sips >/dev/null 2>&1; then
        ql_dir="$tmp_dir/ql"
        mkdir -p "$ql_dir"
        if qlmanage -t -s 1024 -o "$ql_dir" "$svg_icon_path" >/dev/null 2>&1; then
            base_png="$ql_dir/$(basename "$svg_icon_path").png"
            if [[ -f "$base_png" ]]; then
                for size in 16 32 128 256 512; do
                    double=$((size * 2))
                    sips -z "$size" "$size" "$base_png" --out "$iconset_dir/icon_${size}x${size}.png" >/dev/null
                    sips -z "$double" "$double" "$base_png" --out "$iconset_dir/icon_${size}x${size}@2x.png" >/dev/null
                done
                if iconutil -c icns "$iconset_dir" -o "$resources_dir/Inviscid.icns"; then
                    icon_plist_entry=$'    <key>CFBundleIconFile</key>\n    <string>Inviscid</string>'
                fi
            fi
        fi
    fi
fi

cat > "$contents_dir/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Inviscid</string>
    <key>CFBundleDisplayName</key>
    <string>Inviscid</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.kirakiraayu.inviscid</string>
    <key>CFBundleExecutable</key>
    <string>inviscid</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${version}</string>
    <key>CFBundleVersion</key>
    <string>${bundle_version}</string>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
${icon_plist_entry}
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>
            <string>Markdown Document</string>
            <key>CFBundleTypeRole</key>
            <string>Editor</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>net.daringfireball.markdown</string>
                <string>public.plain-text</string>
            </array>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>md</string>
                <string>markdown</string>
                <string>mdx</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
EOF

output_dir="$(dirname "$output_tar_gz")"
mkdir -p "$output_dir"
output_abs="$(cd "$output_dir" && pwd)/$(basename "$output_tar_gz")"

COPYFILE_DISABLE=1 tar -C "$tmp_dir" -czf "$output_abs" Inviscid.app
echo "Packaged macOS bundle: $output_abs"
