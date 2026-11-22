#!/usr/bin/env python3
"""Build the offline Text Roleplay template from exact local RTWs.

This recipe starts Character's welcome UI, imports a card and opens its linked
Chat. AI replies remain a subsequent checkpoint.
"""

import argparse
import hashlib
from io import BytesIO
import json
import os
from pathlib import Path
import tempfile
import tomllib
import zipfile


SOURCES = (
    ("chat", "rintawa.rtwkit.chat"),
    ("character", "rintawa.rtwkit.character"),
    ("web-runtime", "rintawa.rtwkit.web-runtime"),
)
DOMAIN = ("chat", "character")


def archive_entry(name: str, data: bytes) -> tuple[zipfile.ZipInfo, bytes]:
    entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
    entry.compress_type = zipfile.ZIP_STORED
    entry.create_system = 3
    entry.external_attr = 0o100644 << 16
    return entry, data


def source(artifacts: Path, slug: str, expected_id: str) -> tuple[bytes, dict]:
    path = artifacts / f"{slug}.rtw"
    data = path.read_bytes()
    with zipfile.ZipFile(BytesIO(data)) as package:
        if len(package.infolist()) != len(set(package.namelist())):
            raise ValueError(f"duplicate entries in {path}")
        root = tomllib.loads(package.read("rtw.toml").decode("utf-8"))
        if root != {"format": 1, "content": "rintawa.extension@1", "entry": "manifest.toml"}:
            raise ValueError(f"{path} is not an extension RTW")
        manifest = tomllib.loads(package.read("manifest.toml").decode("utf-8"))
        if manifest.get("id") != expected_id:
            raise ValueError(f"{path} has unexpected package ID")
        if not isinstance(manifest.get("version"), str) or not manifest["version"]:
            raise ValueError(f"{path} has no version")
    return data, manifest


def component(manifest: dict, component_id: str) -> dict:
    matches = [row for row in manifest.get("components", []) if row.get("id") == component_id]
    if len(matches) != 1:
        raise ValueError(f"expected one {component_id} component in {manifest['id']}")
    return matches[0]


def descriptor(artifacts: Path) -> tuple[dict, dict[str, bytes]]:
    packages = {}
    for slug, expected_id in SOURCES:
        packages[slug] = source(artifacts, slug, expected_id)
    web_runtime, web_manifest = packages["web-runtime"]
    runtime = component(web_manifest, "runtime")
    if runtime.get("target") != "rintawa.runtime.wasm-component@1":
        raise ValueError("web-runtime has an unexpected execution target")

    slots = []
    grants = []
    embedded = []
    files = {}
    for slug in DOMAIN:
        data, manifest = packages[slug]
        digest = "sha256:" + hashlib.sha256(data).hexdigest()
        parts = [("backend", "rintawa.runtime.wasm-component@1"),
                 ("ui", "rintawa.runtime.web-bundle@2")]
        if slug == "character":
            parts.append(("story", "rintawa.runtime.web-bundle@2"))
        for component_id, target in parts:
            part = component(manifest, component_id)
            if part.get("target") != target:
                raise ValueError(f"{slug}/{component_id} has an unexpected target")
            permissions = part.get("permissions", {}).get("runtime", [])
            if not isinstance(permissions, list) or len(permissions) != len(set(permissions)):
                raise ValueError(f"{slug}/{component_id} has invalid runtime permissions")
            grants.extend({"slot": slug, "componentId": component_id, "permission": permission}
                          for permission in permissions)
        slots.append({"slot": slug, "artifact": digest, "enabled": True,
                      "packageId": manifest["id"], "version": manifest["version"]})
        path = f"extensions/{slug}.rtw"
        embedded.append({"digest": digest, "path": path})
        files[path] = data

    runtime_digest = "sha256:" + hashlib.sha256(web_runtime).hexdigest()
    runtime_path = "extensions/web-runtime.rtw"
    embedded.append({"digest": runtime_digest, "path": runtime_path})
    files[runtime_path] = web_runtime
    composition = {
        "preferredProviders": [
            {"contractId": f"rintawa.{slug}", "contractVersion": 1,
             "provider": {"slot": slug, "componentId": "backend"}}
            for slug in DOMAIN
        ] + [{"contractId":"rintawa.roleplay.workflow","contractVersion":1,
              "provider":{"slot":"character","componentId":"story"}}],
        "authoritativeProviders": [
            {"point": f"rintawa/{slug}/v1",
             "provider": {"slot": slug, "componentId": "backend"}}
            for slug in DOMAIN
        ],
        "requestedGrants": grants,
    }
    result = {
        "format": 1,
        "name": "Text Roleplay",
        "description": "A character-first story with independent Characters and Chat.",
        "components": slots,
        "composition": composition,
        "entry": {"slot": "character", "componentId": "story"},
        "hostRequirements": [{"artifact": runtime_digest, "componentId": "runtime",
                              "target": "rintawa.runtime.web-bundle@2"}],
        "seed": None,
        "assets": [],
        "artifacts": embedded,
    }
    return result, files


def build(artifacts: Path, output: Path) -> None:
    if output.exists():
        raise FileExistsError(f"refusing to replace {output}")
    document, files = descriptor(artifacts)
    manifest = b'format = 1\ncontent = "rintawa.world-template@1"\nentry = "template.json"\n'
    body = json.dumps(document, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    if len(body) > 1024 * 1024:
        raise ValueError("template.json exceeds 1 MiB")
    files = {"rtw.toml": manifest, "template.json": body, **files}
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(prefix=".roleplay-", suffix=".rtw", dir=output.parent,
                                         delete=False) as created:
            temporary = Path(created.name)
        with zipfile.ZipFile(temporary, "w") as package:
            for name, data in sorted(files.items()):
                package.writestr(*archive_entry(name, data))
        with temporary.open("rb") as ready:
            os.fsync(ready.fileno())
        os.link(temporary, output)  # Fails if another process published this path.
        directory = os.open(output.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    print(f"{output} (Text Roleplay, no AI replies)")
    for slot in document["components"]:
        print(f"  {slot['packageId']} sourceVersion={slot['version']} artifact={slot['artifact']}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", type=Path, required=True, help="directory with exact local RTWs")
    parser.add_argument("--output", type=Path, required=True, help="new template RTW path; never overwritten")
    args = parser.parse_args()
    build(args.artifacts, args.output)


if __name__ == "__main__":
    main()
