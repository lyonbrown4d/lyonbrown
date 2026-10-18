"""Verify pinned vendor metadata and native package outputs; never extracts SDKs."""

import argparse
import hashlib
import json
from pathlib import Path
from typing import TypedDict
import urllib.request


class Archive(TypedDict):
  name: str
  sha1: str
  size: int
  type: str


class Selected(TypedDict):
  platform: str
  archive: Archive


class Pin(TypedDict):
  source: str
  cef_version: str
  crate_version: str
  export_cef_dir_checksum: str
  targets: dict[str, Selected]


class Artifact(TypedDict):
  name: str
  size: int
  sha256: str


def require(condition: bool, message: str) -> None:
  if not condition:
    raise SystemExit(message)


def digest(path: Path, algorithm: str) -> str:
  with path.open("rb") as stream:
    return hashlib.file_digest(stream, algorithm).hexdigest()


def parse_archive(value: object) -> Archive:
  if not isinstance(value, dict):
    raise SystemExit("CEF archive metadata must be an object")
  name, sha1, size, kind = (value.get(k) for k in ("name", "sha1", "size", "type"))
  if not (isinstance(name, str) and isinstance(sha1, str)
    and isinstance(size, int) and isinstance(kind, str)):
    raise SystemExit("CEF archive metadata has invalid field types")
  return {"name": name, "sha1": sha1, "size": size, "type": kind}


def check_metadata(actual: object, expected: Archive) -> None:
  require(parse_archive(actual) == parse_archive(expected), "CEF archive metadata differs from pin")


def check_index(pin: Pin, selected: Selected) -> None:
  with urllib.request.urlopen(pin["source"], timeout=60) as response:
    index = json.load(response)
  versions = index[selected["platform"]]["versions"]
  version = next((v for v in versions if v["cef_version"] == pin["cef_version"]), None)
  require(version is not None, "Pinned CEF version is not published for this target")
  archive = next((f for f in version["files"] if f["type"] == "minimal"), None)
  require(archive is not None, "Pinned minimal CEF SDK is not published")
  check_metadata(archive, selected["archive"])
  registry = "https://index.crates.io/ex/po/export-cef-dir"
  with urllib.request.urlopen(registry, timeout=60) as response:
    versions = [json.loads(line) for line in response]
  utility = next((v for v in versions if v["vers"] == pin["crate_version"]), None)
  require(utility is not None, "Pinned export-cef-dir is not published")
  require(not utility["yanked"]
    and utility["cksum"] == pin["export_cef_dir_checksum"],
    "Pinned export-cef-dir registry checksum changed or version was yanked")
  print(f"Verified vendor index: {selected['archive']['name']}")


def check_sdk(sdk: Path, expected: Archive) -> None:
  check_metadata(json.loads((sdk / "archive.json").read_text()), expected)
  archive = sdk.parent / expected["name"]
  require(archive.stat().st_size == expected["size"], "CEF archive size differs from pin")
  require(digest(archive, "sha1") == expected["sha1"], "CEF archive SHA-1 differs from pin")
  print(f"Verified downloaded CEF SDK: {expected['name']}")
  # export-cef-dir already performed maintained extraction. Reclaim archive space.
  archive.unlink()


def check_files(root: Path, names: list[str]) -> None:
  for name in names:
    require((root / name).is_file(), f"Native payload is missing {root / name}")


def check_outputs(output: Path, target: str) -> list[Artifact]:
  if "windows" in target:
    expected = {"*.msi": 2}
    for client, executable in (("cli", "agent"), ("desktop", "agent-desktop")):
      stages = list(output.glob(f"windows-{client}-*/payload"))
      require(len(stages) == 1, f"Expected one {client} Windows native payload")
      check_files(stages[0], [f"{executable}.exe", f"{executable}.dll",
        f"{executable}.pdb", "libcef.dll", "icudtl.dat", "resources.pak",
        "locales/en-US.pak", "agent-install.exe"])
  elif "linux" in target:
    expected = {"*.deb": 3, "*.rpm": 3}
    for client, executable in (("cli", "agent-cli"), ("desktop", "agent-desktop")):
      stage = Path("target/release/lyonbrown-linux") / client
      check_files(stage / "runtime", ["libcef.so", "icudtl.dat", "resources.pak",
        "locales/en-US.pak"])
      check_files(stage / "programs", [executable, "chrome-sandbox"])
  else:
    expected = {"*.pkg": 2, "*.dmg": 1, "*.tar.gz": 2}
    apps = [output / "Lyonbrown.app", *output.glob("lyonbrown-cli-*/Lyonbrown CLI.app")]
    require(len(apps) == 2, "Expected CLI and Desktop native app bundles")
    for app in apps:
      frameworks = app / "Contents/Frameworks"
      require((frameworks / "Chromium Embedded Framework.framework").is_dir(),
        f"Missing native CEF framework in {app}")
      helpers = list(frameworks.glob("*.app/Contents/MacOS/*"))
      require(len(helpers) == 5 and all(p.is_file() for p in helpers),
        f"Expected all five maintained sandbox helper executables in {app}")
  artifacts: list[Artifact] = []
  for pattern, count in expected.items():
    paths = sorted(output.glob(pattern))
    require(len(paths) == count, f"Expected {count} {pattern} artifacts; found {len(paths)}")
    for path in paths:
      require(path.stat().st_size > 0, f"Empty package: {path}")
      artifacts.append({"name": path.name, "size": path.stat().st_size,
        "sha256": digest(path, "sha256")})
  return artifacts


def main() -> None:
  parser = argparse.ArgumentParser(description=__doc__)
  parser.add_argument("target")
  parser.add_argument("--index", action="store_true")
  parser.add_argument("--sdk", type=Path)
  parser.add_argument("--artifacts", type=Path)
  args = parser.parse_args()
  pin: Pin = json.loads(Path(__file__).with_name("cef-sdk.json").read_text())
  selected = pin["targets"][args.target]
  if args.index:
    check_index(pin, selected)
  if args.sdk:
    check_sdk(args.sdk, selected["archive"])
  if args.artifacts:
    report = {"target": args.target, "cef": selected, "cef_version": pin["cef_version"],
      "artifacts": check_outputs(args.artifacts, args.target),
      "acceptance": "Native compilation, non-ignored tests and package generation only; "
        "GUI, sandbox startup and install/upgrade/repair/uninstall are separate acceptance."}
    (args.artifacts / "native-packages.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
  main()
