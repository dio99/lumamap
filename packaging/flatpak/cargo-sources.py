#!/usr/bin/env python3
"""Skriver cargo-sources.json ur Cargo.lock, så att Flatpak kan bygga utan nät.

Samma format som flatpak-cargo-generator, men bara för crates.io-paket (vi har
inga git-beroenden) och utan andra beroenden än Python 3.11.

    python3 packaging/flatpak/cargo-sources.py
"""

import json
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"

lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
sources = []
for pkg in lock["package"]:
    if "source" not in pkg:
        continue  # våra egna crates
    if pkg["source"] != CRATES_IO:
        sys.exit(f"Okänd källa för {pkg['name']}: {pkg['source']}")
    name, version, checksum = pkg["name"], pkg["version"], pkg["checksum"]
    dest = f"cargo/vendor/{name}-{version}"
    sources.append({
        "type": "archive",
        "archive-type": "tar-gzip",
        "url": f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
        "sha256": checksum,
        "dest": dest,
    })
    sources.append({
        "type": "inline",
        "contents": json.dumps({"package": checksum, "files": {}}),
        "dest": dest,
        "dest-filename": ".cargo-checksum.json",
    })
sources.append({
    "type": "inline",
    "contents": '[source.vendored-sources]\ndirectory = "cargo/vendor"\n\n[source.crates-io]\nreplace-with = "vendored-sources"\n',
    "dest": "cargo",
    "dest-filename": "config.toml",
})
out = Path(__file__).with_name("cargo-sources.json")
out.write_text(json.dumps(sources, indent=2) + "\n")
print(f"{out.relative_to(ROOT)}: {len(sources) // 2} paket")
