"""Require the Cargo and Nix source graphs to name one Hellas revision."""
import json
from pathlib import Path
import tomllib
from urllib.parse import parse_qs, urlsplit

root = Path(__file__).resolve().parents[1]
flake = json.loads((root / "flake.lock").read_text())
node = flake["nodes"][flake["root"]]["inputs"]["hellas"]
expected = flake["nodes"][node]["locked"]["rev"]
manifest = tomllib.loads((root / "src-tauri/Cargo.toml").read_text())
for section in ("dependencies", "dev-dependencies"):
    for name, dependency in manifest.get(section, {}).items():
        if name.startswith("hellas-"):
            assert dependency.get("rev") == expected, f"{name}: Cargo/Nix revision mismatch"
lock = tomllib.loads((root / "Cargo.lock").read_text())
for package in lock["package"]:
    if package["name"].startswith("hellas-") and "source" in package:
        source = urlsplit(package["source"].removeprefix("git+"))
        assert parse_qs(source.query).get("rev") == [expected], f"{package['name']}: stale git source"
        assert source.fragment == expected, f"{package['name']}: unexpected resolved revision"
print(f"Cargo and Nix agree on Hellas {expected}")
