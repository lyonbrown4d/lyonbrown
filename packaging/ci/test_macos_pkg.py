"""Final installer metadata and payload regressions, independent of the CEF SDK."""

import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

from macos_pkg import check_expanded_pkg, check_macos_pkg, validate_bom_paths


class BomPathTest(unittest.TestCase):
  def test_inventory_includes_ancestors_and_only_known_transport_records(self) -> None:
    expected = {".", "./usr", "./usr/agent"}
    validate_bom_paths([".", "./usr", "./usr/agent", "./._usr", "./usr/._agent"], expected)
    for defect in ([".", "./usr/agent"], [".", "./usr", "./usr/agent", "./extra"],
      [".", "./usr", "./usr/agent", "./._extra"], [".", "usr", "./usr/agent"],
      [".", "./usr", "./usr/._agent"],
      [".", "./usr", "./usr/../agent"], [".", "./usr", "./usr/agent", "./usr/agent"]):
      with self.subTest(paths=defect), self.assertRaises(SystemExit):
        validate_bom_paths(defect, expected)
    # A literal source ._ file is a required entry, never discarded as transport.
    literal = expected | {"./usr/._agent"}
    validate_bom_paths(sorted(literal), literal)
    with self.assertRaises(SystemExit):
      validate_bom_paths(sorted(expected), literal)


@unittest.skipUnless(os.name == "posix", "expanded Apple payloads require POSIX filesystem modes")
class MacPkgTest(unittest.TestCase):
  def setUp(self) -> None:
    temporary = tempfile.TemporaryDirectory()
    self.addCleanup(temporary.cleanup)
    self.root = Path(temporary.name)
    self.source = self.root / "Lyonbrown CLI.app"
    self.expanded = self.root / "expanded"
    self.content_id = "org.lyonbrown.cli.content.h" + "a" * 64
    self.retained = "Library/Application Support/Lyonbrown/Clients/cli/1.0.0-" + "a" * 64 + "/Lyonbrown CLI.app"
    self.components = {"org.lyonbrown.agentd": "daemon.pkg", "org.lyonbrown.cli": "cli.pkg",
      self.content_id: "cli-content.pkg"}
    self.write(self.source / "Contents/MacOS/agentd", b"daemon", 0o755)
    self.write(self.source / "Contents/MacOS/agent-cli", b"client", 0o755)
    self.write(self.source / "Contents/Resources/data", b"resource", 0o644)
    (self.source / "Contents/Resources/current").symlink_to("data")
    self.write(self.source / "Contents/Info.plist", plistlib.dumps({
      "CFBundleIdentifier": "org.lyonbrown.cli", "CFBundleVersion": "1.0.0",
      "CFBundleShortVersionString": "1.0.0", "CFBundleExecutable": "agent-cli"}), 0o644)
    for identifier, name in self.components.items():
      component = self.expanded / name
      (component / "Payload").mkdir(parents=True)
      self.write(component / "Bom", b"bom", 0o644)
      info = ET.Element("pkg-info", {"identifier": identifier, "version": "1.0.0",
        "install-location": "/", "relocatable": "false", "postinstall-action": "none"})
      ET.SubElement(info, "payload", {"numberOfFiles": "1", "installKBytes": "1"})
      if identifier == self.content_id:
        ET.SubElement(info, "bundle", {"id": "org.lyonbrown.cli", "path": "./" + self.retained})
      for tag in ("bundle-version", "upgrade-bundle", "update-bundle", "atomic-update-bundle",
        "strict-identifier", "relocate"):
        ET.SubElement(info, tag)
      ET.ElementTree(info).write(component / "PackageInfo")
    self.write(self.expanded / "daemon.pkg/Payload/Library/Application Support/Lyonbrown/agentd",
      b"daemon", 0o755)
    selector = self.expanded / "cli.pkg/Payload/usr/local/bin/agent"
    selector.parent.mkdir(parents=True)
    selector.symlink_to("/" + self.retained + "/Contents/MacOS/agent-cli")
    shutil.copytree(self.source, self.expanded / "cli-content.pkg/Payload" / self.retained,
      symlinks=True)
    self.distribution()

  def write(self, path: Path, data: bytes, mode: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    path.chmod(mode)

  def distribution(self) -> None:
    root = ET.Element("installer-gui-script", {"minSpecVersion": "2"})
    ET.SubElement(root, "title").text = "Lyonbrown cli"
    ET.SubElement(root, "options", {"customize": "never", "require-scripts": "false"})
    outline = ET.SubElement(root, "choices-outline")
    for index, (identifier, name) in enumerate(self.components.items()):
      choice_id = f"component{index}"
      ET.SubElement(outline, "line", {"choice": choice_id})
      choice = ET.SubElement(root, "choice", {"id": choice_id, "visible": "false"})
      ET.SubElement(choice, "pkg-ref", {"id": identifier})
      ET.SubElement(root, "pkg-ref", {"id": identifier, "version": "1.0.0"}).text = "#" + name
      ET.SubElement(ET.SubElement(root, "pkg-ref", {"id": identifier}), "bundle-version")
    ET.ElementTree(root).write(self.expanded / "Distribution")

  def verify(self) -> None:
    check_expanded_pkg(self.expanded, self.source, "cli", "1.0.0")

  def test_complete_expansion_passes(self) -> None:
    self.verify()

  def test_metadata_corruption_fails(self) -> None:
    path = self.expanded / "cli-content.pkg/PackageInfo"
    original = path.read_bytes()
    for defect in ("identifier", "version", "install-location", "relocatable", "relocate",
      "scripts", "missing payload", "duplicate relocate", "bundle escape", "unknown child"):
      with self.subTest(defect=defect):
        root = ET.fromstring(original)
        if defect in ("identifier", "version", "install-location", "relocatable"):
          root.set(defect, "wrong")
        elif defect == "relocate":
          ET.SubElement(root.find("relocate"), "bundle", {"id": "org.lyonbrown.cli"})
        elif defect == "missing payload":
          root.remove(root.find("payload"))
        elif defect == "duplicate relocate":
          ET.SubElement(root, "relocate")
        elif defect == "bundle escape":
          root.find("bundle").set("path", "./Applications/Lyonbrown.app")
        else:
          ET.SubElement(root, "scripts" if defect == "scripts" else "unexpected")
        ET.ElementTree(root).write(path)
        with self.assertRaises(SystemExit):
          self.verify()
    path.write_bytes(original)

  def test_distribution_corruption_fails(self) -> None:
    path = self.expanded / "Distribution"
    original = path.read_bytes()
    for defect in ("script", "options", "choice", "package path", "version", "duplicate", "missing"):
      with self.subTest(defect=defect):
        root = ET.fromstring(original)
        if defect == "script":
          ET.SubElement(root, "installation-check").text = "run()"
        elif defect == "options":
          root.find("options").set("require-scripts", "true")
        elif defect == "choice":
          root.find("choice/pkg-ref").set("id", "other")
        elif defect == "package path":
          root.find("pkg-ref").text = "#../daemon.pkg"
        elif defect == "version":
          root.find("pkg-ref").set("version", "2.0.0")
        elif defect == "duplicate":
          root.append(ET.fromstring(ET.tostring(root.find("pkg-ref"))))
        else:
          root.remove(root.find("pkg-ref"))
        ET.ElementTree(root).write(path)
        with self.assertRaises(SystemExit):
          self.verify()
    path.write_bytes(original)

  def test_content_inventory_corruption_fails(self) -> None:
    app = self.expanded / "cli-content.pkg/Payload" / self.retained
    data = app / "Contents/Resources/data"
    for defect in ("bytes", "mode", "missing", "extra", "link", "type"):
      with self.subTest(defect=defect):
        shutil.rmtree(app)
        shutil.copytree(self.source, app, symlinks=True)
        if defect == "bytes":
          data.write_bytes(b"modified")
        elif defect == "mode":
          data.chmod(0o755)
        elif defect == "missing":
          data.unlink()
        elif defect == "extra":
          (app / "unexpected").mkdir()
        elif defect == "link":
          (app / "Contents/Resources/current").unlink()
          (app / "Contents/Resources/current").symlink_to("../Info.plist")
        else:
          data.unlink()
          data.mkdir()
        with self.assertRaises(SystemExit):
          self.verify()

  def test_selector_daemon_and_component_boundaries_fail(self) -> None:
    for defect in ("selector", "daemon", "extra payload", "empty directory", "extra component", "scripts"):
      with self.subTest(defect=defect):
        backup = self.root / "backup"
        shutil.copytree(self.expanded, backup, symlinks=True)
        if defect == "selector":
          selector = self.expanded / "cli.pkg/Payload/usr/local/bin/agent"
          selector.unlink()
          selector.symlink_to("/Applications/other")
        elif defect == "daemon":
          (self.expanded / "daemon.pkg/Payload/Library/Application Support/Lyonbrown/agentd").write_bytes(b"wrong")
        elif defect == "extra payload":
          self.write(self.expanded / "cli-content.pkg/Payload/unexpected", b"extra", 0o644)
        elif defect == "empty directory":
          (self.expanded / "cli.pkg/Payload/empty").mkdir()
        elif defect == "extra component":
          (self.expanded / "other.pkg").mkdir()
        else:
          self.write(self.expanded / "cli.pkg/Scripts/postinstall", b"script", 0o755)
        with self.assertRaises(SystemExit):
          self.verify()
        shutil.rmtree(self.expanded)
        backup.rename(self.expanded)

  def test_host_mismatch_fails_instead_of_skipping_inspection(self) -> None:
    with patch("macos_pkg.sys.platform", "linux"), self.assertRaisesRegex(SystemExit, "requires macOS"):
      check_macos_pkg(self.root / "final.pkg", self.source, "cli", "1.0.0")

  @unittest.skipUnless(sys.platform == "darwin", "native pkgutil integration requires macOS")
  def test_real_final_pkg_expansion_adapter(self) -> None:
    # Apple tools construct a real product archive; pkgutil alone reads it, never installer.
    for identifier, name in self.components.items():
      command = ["/usr/bin/pkgbuild", "--root", str(self.expanded / name / "Payload"),
        "--identifier", identifier, "--version", "1.0.0", "--install-location", "/"]
      if identifier == self.content_id:
        component_plist = self.root / "components.plist"
        component_plist.write_bytes(plistlib.dumps([{"RootRelativeBundlePath": self.retained,
          "BundleIsRelocatable": False}]))
        command.extend(["--component-plist", str(component_plist)])
      subprocess.run([*command, str(self.root / name)], check=True, capture_output=True)
    distribution = ET.parse(self.expanded / "Distribution")
    for reference in distribution.getroot().findall("pkg-ref"):
      if list(reference):
        distribution.getroot().remove(reference)
      else:
        reference.text = reference.text.removeprefix("#")
    source_xml = self.root / "source.xml"
    distribution.write(source_xml)
    package = self.root / "final.pkg"
    subprocess.run(["/usr/bin/productbuild", "--distribution", str(source_xml),
      "--package-path", str(self.root), str(package)], check=True, capture_output=True)
    check_macos_pkg(package, self.source, "cli", "1.0.0")


if __name__ == "__main__":
  unittest.main()
