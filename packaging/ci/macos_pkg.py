"""Inspect final Apple installers without installing or executing their scripts."""

import os
from pathlib import Path, PurePosixPath
import plistlib
import re
import stat
import subprocess
import sys
import tempfile
from typing import Literal, NoReturn
import xml.etree.ElementTree as ET

from archive_payload import Entry, content_hash, staged_entries

Client = Literal["cli", "desktop"]


def fail(message: str) -> NoReturn:
  raise SystemExit(f"Native PKG payload: {message}")


def require(condition: bool, message: str) -> None:
  if not condition:
    fail(message)


def xml(path: Path, tag: str) -> ET.Element:
  root = ET.parse(path).getroot()
  require(root.tag == tag, f"invalid XML root in {path.name}")
  return root


def only_tags(node: ET.Element, tags: set[str]) -> None:
  require(all(child.tag in tags for child in node), f"unexpected XML child in {node.tag}")
  require(not (node.text or "").strip(), f"unexpected XML text in {node.tag}")
  require(all(not (child.tail or "").strip() for child in node), "unexpected XML tail")


def bundle_records(node: ET.Element, retained: str | None) -> None:
  only_tags(node, {"bundle"})
  for bundle in node:
    require(not list(bundle) and not (bundle.text or "").strip(), "malformed bundle record")
    require(bool(bundle.get("id")), "bundle record has no identifier")
    path = bundle.get("path")
    if path is not None:
      path = path.removeprefix("./")
      require(retained is not None and (path == retained or path.startswith(retained + "/"))
        and all(part not in ("", ".", "..") for part in path.split("/")),
        "bundle metadata escapes immutable app")


def package_info(path: Path, identifier: str, version: str, retained: str | None) -> None:
  root = xml(path, "pkg-info")
  require(set(root.attrib) <= {"identifier", "version", "install-location", "relocatable",
    "postinstall-action", "overwrite-permissions", "format-version", "generator-version", "auth"},
    "unexpected package metadata attributes")
  for key, value in {"identifier": identifier, "version": version, "install-location": "/",
    "relocatable": "false", "postinstall-action": "none"}.items():
    require(root.get(key) == value, f"invalid {key} in {path}")
  containers = {"bundle-version", "upgrade-bundle", "update-bundle",
    "atomic-update-bundle", "strict-identifier", "relocate"}
  only_tags(root, containers | {"payload", "bundle"})
  require(len(root.findall("payload")) == 1, "expected exactly one payload metadata record")
  payload = root.findall("payload")[0]
  require(not list(payload) and not (payload.text or "").strip()
    and set(payload.attrib) == {"numberOfFiles", "installKBytes"}
    and all(value.isdecimal() for value in payload.attrib.values()), "malformed payload metadata")
  for tag in containers:
    records = root.findall(tag)
    require(len(records) == 1, f"expected exactly one {tag} metadata record")
    bundle_records(records[0], retained)
  relocate = root.find("relocate")
  require(relocate is not None and not list(relocate) and not relocate.attrib,
    "package relocation must be empty")
  bundles = root.findall("bundle")
  if retained is None:
    require(not bundles and all(not list(root.find(tag)) for tag in containers),
      "non-content component contains bundle metadata")
  else:
    wrapper = ET.Element("bundles")
    wrapper.extend(bundles)
    bundle_records(wrapper, retained)
    require(any(b.get("path", "").removeprefix("./") == retained for b in bundles),
      "immutable app is missing from bundle metadata")


def distribution(path: Path, client: Client, version: str,
  components: dict[str, str], retained: str) -> None:
  root = xml(path, "installer-gui-script")
  require(root.attrib == {"minSpecVersion": "2"}, "invalid distribution attributes")
  only_tags(root, {"title", "options", "choices-outline", "choice", "pkg-ref"})
  titles, options, outlines = (root.findall(tag) for tag in ("title", "options", "choices-outline"))
  require(len(titles) == 1 and titles[0].text == f"Lyonbrown {client}"
    and not list(titles[0]), "invalid distribution title")
  require(len(options) == 1 and options[0].attrib ==
    {"customize": "never", "require-scripts": "false"} and not list(options[0]),
    "distribution must disable scripts and customization")
  require(len(outlines) == 1, "expected one choices outline")
  outline = outlines[0]
  only_tags(outline, {"line"})
  require([line.attrib for line in outline] ==
    [{"choice": f"component{i}"} for i in range(3)]
    and all(not list(line) for line in outline), "invalid choices outline")
  choices = root.findall("choice")
  require(len(choices) == 3, "expected exactly three component choices")
  for index, (choice, identifier) in enumerate(zip(choices, components)):
    only_tags(choice, {"pkg-ref"})
    require(choice.attrib == {"id": f"component{index}", "visible": "false"}
      and len(choice) == 1 and choice[0].attrib == {"id": identifier}
      and not list(choice[0]) and not (choice[0].text or "").strip(), "invalid component choice")
  references: set[str] = set()
  supplements: set[str] = set()
  for reference in root.findall("pkg-ref"):
    identifier = reference.get("id", "")
    require(identifier in components, "unexpected distribution component")
    if list(reference):
      require(reference.attrib == {"id": identifier} and identifier not in supplements
        and len(reference) == 1 and reference[0].tag == "bundle-version",
        "malformed supplemental component reference")
      only_tags(reference, {"bundle-version"})
      bundle_records(reference[0], retained if ".content.h" in identifier else None)
      supplements.add(identifier)
    else:
      require(identifier not in references and reference.get("version") == version
        and (reference.text or "").strip() == f"#{components[identifier]}"
        and set(reference.attrib) <= {"id", "version", "installKBytes", "updateKBytes"},
        "invalid distribution package reference")
      references.add(identifier)
  require(references == set(components) and supplements == set(components),
    "distribution component references are incomplete")


def single_payload(payload: Path, leaf: str, expected: Entry) -> None:
  allowed = {str(parent) for parent in PurePosixPath(leaf).parents} - {"."}
  actual: set[str] = set()
  # Do not follow selector links. scandir gathers each directory once.
  def visit(directory: Path) -> None:
    with os.scandir(directory) as children:
      for child in children:
        path = Path(child.path)
        name = path.relative_to(payload).as_posix()
        metadata = child.stat(follow_symlinks=False)
        if name in allowed:
          require(stat.S_ISDIR(metadata.st_mode), f"payload ancestor is not a directory: {name}")
          visit(path)
        else:
          require(name == leaf, f"unexpected payload entry: {name}")
          mode = stat.S_IMODE(metadata.st_mode)
          if stat.S_ISLNK(metadata.st_mode):
            # Selector link permissions vary with host umask; only its type and target apply.
            entry = Entry("symlink", expected.mode, target=os.readlink(path))
          elif stat.S_ISREG(metadata.st_mode):
            with path.open("rb") as stream:
              entry = Entry("file", mode, metadata.st_size, content_hash(stream))
          else:
            fail(f"unexpected payload type: {name}")
          require(entry == expected, f"payload differs from staging: {name}")
        actual.add(name)
  visit(payload)
  require(actual == allowed | {leaf}, "incomplete component payload")


def check_expanded_pkg(expanded: Path, source: Path, client: Client, version: str) -> None:
  """Validate pkgutil's expanded tree; source is the complete staged app."""
  names = ["daemon.pkg", f"{client}.pkg", f"{client}-content.pkg"]
  require({p.name for p in expanded.iterdir()} == {"Distribution", *names},
    "unexpected or missing expanded components")
  require((expanded / "Distribution").is_file() and not (expanded / "Distribution").is_symlink(),
    "invalid Distribution file")
  content = xml(expanded / names[2] / "PackageInfo", "pkg-info").get("identifier", "")
  match = re.fullmatch(rf"org\.lyonbrown\.{client}\.content\.h([0-9a-f]{{64}})", content)
  require(match is not None, "invalid immutable content identifier")
  identity = content.rsplit(".h", 1)[1]
  retained = f"Library/Application Support/Lyonbrown/Clients/{client}/{version}-{identity}/{source.name}"
  identifiers = ["org.lyonbrown.agentd", f"org.lyonbrown.{client}", content]
  components = dict(zip(identifiers, names))
  for index, (identifier, name) in enumerate(components.items()):
    component = expanded / name
    require(component.is_dir() and not component.is_symlink()
      and {p.name for p in component.iterdir()} == {"PackageInfo", "Bom", "Payload"},
      f"unexpected component files or installed scripts: {name}")
    require(all((component / file).is_file() and not (component / file).is_symlink()
      for file in ("PackageInfo", "Bom")) and (component / "Payload").is_dir()
      and not (component / "Payload").is_symlink(), "invalid component filesystem shape")
    package_info(component / "PackageInfo", identifier, version, retained if index == 2 else None)
  distribution(expanded / "Distribution", client, version, components, retained)
  expected_app = staged_entries(source)
  installed = expanded / names[2] / "Payload" / retained
  # Content ancestors must lead only to the app. The app itself has already been compared fully.
  payload = expanded / names[2] / "Payload"
  current = payload
  for part in PurePosixPath(retained).parts:
    require(not current.is_symlink() and {p.name for p in current.iterdir()} == {part},
      "unexpected immutable content payload entry")
    current = current / part
    require(current.is_dir() and not current.is_symlink(), "invalid immutable app ancestor")
  require(staged_entries(installed) == expected_app, "expanded immutable app differs from staging")
  daemon = expected_app.get(f"{source.name}/Contents/MacOS/agentd")
  if daemon is None or daemon.kind != "file":
    fail("staged agentd must be a regular file")
  single_payload(expanded / names[0] / "Payload",
    "Library/Application Support/Lyonbrown/agentd", daemon)
  selector = "usr/local/bin/agent" if client == "cli" else "Applications/Lyonbrown.app"
  target = "/" + retained + ("/Contents/MacOS/agent-cli" if client == "cli" else "")
  single_payload(expanded / names[1] / "Payload", selector, Entry("symlink", 0o777, target=target))


def validate_bom_paths(lines: list[str], expected: set[str]) -> None:
  paths: set[str] = set()
  for name in lines:
    require(name not in paths, "duplicate BOM path")
    require(name == "." or (name.startswith("./") and "\\" not in name
      and all(part not in ("", ".", "..") for part in name[2:].split("/"))),
      "unsafe or unrooted BOM path")
    paths.add(name)
  # pkgbuild includes AppleDouble transport records; expand-full consumes them into
  # filesystem metadata. Only counterparts of real payload entries are permitted.
  transport: set[str] = set()
  for name in paths - expected:
    parent, _, basename = name.rpartition("/")
    require(basename.startswith("._") and parent + "/" + basename[2:] in expected,
      f"unexpected BOM path: {name}")
    transport.add(name)
  require(paths - transport == expected, "BOM inventory differs from expanded payload")


def check_bom_inventory(expanded: Path) -> None:
  for component in expanded.glob("*.pkg"):
    payload = component / "Payload"
    expected = {"."}
    # os.walk lists links without traversing them, preserving directory ancestors.
    for directory, directories, files in os.walk(payload, followlinks=False):
      for name in directories + files:
        expected.add("./" + (Path(directory) / name).relative_to(payload).as_posix())
    result = subprocess.run(["/usr/bin/lsbom", "-s", str(component / "Bom")],
      check=True, capture_output=True, text=True, timeout=60)
    validate_bom_paths(result.stdout.splitlines(), expected)


def check_macos_pkg(package: Path, source: Path, client: Client, version: str) -> None:
  require(sys.platform == "darwin", "final macOS PKG inspection requires macOS pkgutil")
  try:
    with tempfile.TemporaryDirectory(prefix="lyonbrown-pkg-inspect-") as temporary:
      expanded = Path(temporary) / "expanded"
      subprocess.run(["/usr/sbin/pkgutil", "--expand-full", str(package.resolve()), str(expanded)],
        check=True, capture_output=True, text=True, timeout=300)
      check_expanded_pkg(expanded, source, client, version)
      check_bom_inventory(expanded)
  except (OSError, subprocess.SubprocessError, ET.ParseError) as error:
    fail(f"cannot inspect {package.name}: {error}")


def check_macos_packages(output: Path, cli: Path, desktop: Path) -> None:
  versions: set[str] = set()
  sources: tuple[tuple[Client, Path], ...] = (("cli", cli), ("desktop", desktop))
  for client, source in sources:
    with (source / "Contents/Info.plist").open("rb") as stream:
      info = plistlib.load(stream)
    version = info.get("CFBundleVersion") if isinstance(info, dict) else None
    if not isinstance(version, str):
      fail("staged app requires a string CFBundleVersion")
    require(bool(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.+-]+)?", version)),
      "staged app requires a semantic CFBundleVersion")
    versions.add(version)
    check_macos_pkg(output / f"Lyonbrown-{client}-{version}.pkg", source, client, version)
  require(len(versions) == 1, "CLI and Desktop package versions differ")
  # Compare the two shared binaries without rereading the complete CEF trees.
  with (cli / "Contents/MacOS/agentd").open("rb") as cli_daemon, (
    desktop / "Contents/MacOS/agentd").open("rb") as desktop_daemon:
    require(content_hash(cli_daemon) == content_hash(desktop_daemon), "shared daemon bytes differ")
