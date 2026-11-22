#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
[ "$(uname -m)" = arm64 ] || { echo 'Build on an Apple Silicon Mac.' >&2; exit 1; }
package=target/macos-package
bundle="$package/Virtual Printer Studio.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Helpers" "$bundle/Contents/Resources"
cp target/release/virtual-printer-studio "$bundle/Contents/MacOS/"
cp target/release/virtual-printer-studio "$bundle/Contents/Helpers/vps-cups-filter"
python3 - "$bundle" <<'PY'
import plistlib,sys,pathlib
bundle=pathlib.Path(sys.argv[1])
p={
 'CFBundleExecutable':'virtual-printer-studio','CFBundleIdentifier':'studio.virtualprinterstudio.desktop',
 'CFBundleName':'Virtual Printer Studio','CFBundleDisplayName':'Virtual Printer Studio',
 'CFBundlePackageType':'APPL','CFBundleShortVersionString':'0.2.0','CFBundleVersion':'20261122',
 'CFBundleIconFile':'AppIcon','LSMinimumSystemVersion':'12.0',
 'NSHighResolutionCapable':True,'LSMultipleInstancesProhibited':True,
 'NSHumanReadableCopyright':'Virtual Printer Studio — Made with Slint',
}
with (bundle/'Contents/Info.plist').open('wb') as f: plistlib.dump(p,f)
PY
icons=target/AppIcon.iconset
mkdir -p "$icons"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" packaging/macos/AppIcon.png --out "$icons/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" packaging/macos/AppIcon.png --out "$icons/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$icons" -o "$bundle/Contents/Resources/AppIcon.icns"
codesign --force --sign - --timestamp=none "$bundle/Contents/Helpers/vps-cups-filter"
codesign --force --sign - --timestamp=none "$bundle"
codesign --verify --deep --strict --verbose=2 "$bundle"
lipo "$bundle/Contents/MacOS/virtual-printer-studio" -verify_arch arm64
file "$bundle/Contents/MacOS/virtual-printer-studio"
otool -L "$bundle/Contents/MacOS/virtual-printer-studio"
cp docs/macos.md "$package/Read me.txt"
ln -s /Applications "$package/Applications" 2>/dev/null || true
ditto -c -k --sequesterRsrc --keepParent "$bundle" target/VirtualPrinterStudio-AppleSilicon.zip
hdiutil create -volname 'Virtual Printer Studio' -srcfolder "$package" -ov -format UDZO target/VirtualPrinterStudio-AppleSilicon.dmg
shasum -a 256 target/VirtualPrinterStudio-AppleSilicon.{dmg,zip} > target/SHA256SUMS-macos.txt
