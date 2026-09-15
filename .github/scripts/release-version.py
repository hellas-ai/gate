"""Validate Cargo/Tauri version agreement, and optionally the release tag."""

import json
import re
import sys
import tomllib
from pathlib import Path

root = Path(__file__).resolve().parents[2]
workspace = tomllib.loads((root / "Cargo.toml").read_text())
package = tomllib.loads((root / "src-tauri/Cargo.toml").read_text())["package"]
config = json.loads((root / "src-tauri/tauri.conf.json").read_text())
version = workspace["workspace"]["package"]["version"]
package_version = package["version"]
if package_version == {"workspace": True}:
    package_version = version

if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", version):
    sys.exit(f"Invalid release version: {version}")
if config["version"] != version or package_version != version:
    sys.exit("Cargo workspace, desktop package, and Tauri versions must agree")
if len(sys.argv) > 2:
    sys.exit("Usage: release-version.py [vVERSION]")
if len(sys.argv) == 2 and sys.argv[1] != f"v{version}":
    sys.exit(f"Release tag must be v{version}, got {sys.argv[1]}")
print(version)
