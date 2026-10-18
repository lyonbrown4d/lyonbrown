"""Final macOS tar checks use intact synthetic staging, without SDKs or extraction."""

import io
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
from collections.abc import Callable

from verify import check_outputs
from archive_payload import member_path


class ArchivePathTest(unittest.TestCase):
  def test_unsafe_archive_paths_fail_on_every_host(self) -> None:
    for name in ("/bundle/file", "../bundle/file", "bundle/../file", "bundle//file",
      "bundle/./file", "other/file", "bundle\\file"):
      with self.subTest(name=name), self.assertRaises(SystemExit):
        member_path(name, "bundle")
    self.assertEqual(member_path("bundle/dir/", "bundle"), "bundle/dir")


@unittest.skipUnless(os.name == "posix", "requires POSIX modes and symlinks")
class MacArchiveTest(unittest.TestCase):
  def setUp(self) -> None:
    temporary = tempfile.TemporaryDirectory()
    self.addCleanup(temporary.cleanup)
    self.root = Path(temporary.name)
    # TAR fixtures intentionally contain fake PKGs; isolate only the external PKG inspection.
    self.pkg_gate = self.enterContext(patch("verify.check_macos_packages"))
    self.cli = self.root / "lyonbrown-cli-1-macos-aarch64"
    self.desktop = self.root / "Lyonbrown.app"
    for app, main in ((self.cli / "Lyonbrown CLI.app", "agent-cli"),
      (self.desktop, "agent-desktop")):
      for name in (main, "agentd"):
        self.write(app / "Contents/MacOS" / name, b"executable", 0o755)
      self.write(app / "Contents/Info.plist", b"plist", 0o644)
      framework = app / "Contents/Frameworks/Chromium Embedded Framework.framework"
      self.write(framework / "Versions/A/Chromium Embedded Framework", b"framework", 0o755)
      (framework / "Versions/Current").symlink_to("A")
      (framework / "Chromium Embedded Framework").symlink_to(
        "Versions/Current/Chromium Embedded Framework")
      for index in range(5):
        self.write(app / f"Contents/Frameworks/Helper{index}.app/Contents/MacOS/helper",
          b"helper", 0o755)
    self.write(self.cli / "bin/agentd", b"daemon", 0o755)
    (self.cli / "bin/agent").symlink_to("../Lyonbrown CLI.app/Contents/MacOS/agent-cli")
    for name in ("cli.pkg", "desktop.pkg", "desktop.dmg"):
      self.write(self.root / name, b"uninspected installer", 0o644)
    self.cli_archive = self.root / f"{self.cli.name}.tar.gz"
    self.desktop_archive = self.root / "Lyonbrown-app-1-macos-aarch64.tar.gz"
    self.package(self.cli, self.cli_archive)
    self.package(self.desktop, self.desktop_archive)

  def write(self, path: Path, data: bytes, mode: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    path.chmod(mode)

  def package(self, source: Path, archive: Path) -> None:
    with tarfile.open(archive, "w:gz") as bundle:
      bundle.add(source, arcname=source.name)

  def rewrite(self, archive: Path,
    change: Callable[[tarfile.TarInfo, bytes], tuple[tarfile.TarInfo, bytes] | None]) -> None:
    members: list[tuple[tarfile.TarInfo, bytes]] = []
    with tarfile.open(archive, "r:gz") as bundle:
      for member in bundle:
        stream = bundle.extractfile(member) if member.isfile() else None
        data = stream.read() if stream is not None else b""
        result = change(member, data)
        if result is not None:
          members.append(result)
    with tarfile.open(archive, "w:gz") as bundle:
      for member, data in members:
        bundle.addfile(member, io.BytesIO(data) if member.isfile() else None)

  def verify(self) -> None:
    self.assertEqual(len(check_outputs(self.root, "aarch64-apple-darwin")), 5)

  def test_complete_trees_and_legal_relative_links_pass(self) -> None:
    self.verify()
    self.pkg_gate.assert_called_once_with(self.root, self.cli / "Lyonbrown CLI.app", self.desktop)

  def test_archive_corruption_is_rejected_with_valid_staging(self) -> None:
    for defect in ("missing helper", "modified executable", "execute mode", "unsafe path"):
      with self.subTest(defect=defect):
        self.package(self.desktop, self.desktop_archive)
        def change(member: tarfile.TarInfo, data: bytes) -> tuple[tarfile.TarInfo, bytes] | None:
          if defect == "missing helper" and member.name.endswith("Helper0.app/Contents/MacOS/helper"):
            return None
          if member.name.endswith("/agent-desktop"):
            if defect == "modified executable":
              data = b"!" * len(data)
            elif defect == "execute mode":
              member.mode = 0o644
            elif defect == "unsafe path":
              member.name = "../agent-desktop"
          return member, data
        self.rewrite(self.desktop_archive, change)
        with self.assertRaises(SystemExit):
          self.verify()

  def test_cli_link_and_entry_boundaries_are_checked(self) -> None:
    for defect in ("absolute", "escape", "broken", "cycle", "changed target", "duplicate",
      "special", "extra", "kind"):
      with self.subTest(defect=defect):
        self.package(self.cli, self.cli_archive)
        def change(member: tarfile.TarInfo, data: bytes) -> tuple[tarfile.TarInfo, bytes]:
          if member.name.endswith("/bin/agent"):
            targets = {"absolute": "/tmp/agent", "escape": "../../outside",
              "broken": "missing", "cycle": "agent", "changed target": "agentd"}
            if defect in targets:
              member.linkname = targets[defect]
            elif defect == "special":
              member.type = tarfile.FIFOTYPE
            elif defect == "kind":
              member.type = tarfile.DIRTYPE
          return member, data
        self.rewrite(self.cli_archive, change)
        if defect in ("duplicate", "extra"):
          # Append through a rewritten tar so gzip append behavior is irrelevant.
          extra = tarfile.TarInfo(f"{self.cli.name}/bin/agentd" if defect == "duplicate"
            else f"{self.cli.name}/unexpected")
          original = self.cli_archive.read_bytes()
          with tarfile.open(fileobj=io.BytesIO(original), mode="r:gz") as source:
            with tarfile.open(self.cli_archive, "w:gz") as destination:
              for member in source:
                destination.addfile(member, source.extractfile(member) if member.isfile() else None)
              destination.addfile(extra, io.BytesIO(b""))
        with self.assertRaises(SystemExit):
          self.verify()

  def test_contained_hardlink_contents_are_checked(self) -> None:
    self.write(self.cli / "bin/agent-copy", b"daemon", 0o755)
    self.package(self.cli, self.cli_archive)
    def hardlink(member: tarfile.TarInfo, data: bytes) -> tuple[tarfile.TarInfo, bytes]:
      if member.name.endswith("/agent-copy"):
        member.type = tarfile.LNKTYPE
        member.linkname = f"{self.cli.name}/bin/agentd"
        member.size = 0
        data = b""
      return member, data
    self.rewrite(self.cli_archive, hardlink)
    self.verify()
    self.write(self.cli / "bin/agent-copy", b"wrong!", 0o755)
    with self.assertRaisesRegex(SystemExit, "differs from staging"):
      self.verify()

  def test_consistently_incomplete_staging_and_tar_are_rejected(self) -> None:
    main = self.desktop / "Contents/MacOS/agent-desktop"
    daemon = self.desktop / "Contents/MacOS/agentd"
    plist = self.desktop / "Contents/Info.plist"
    helper = self.desktop / "Contents/Frameworks/Helper0.app/Contents/MacOS/helper"
    for path, defect in ((main, "missing"), (daemon, "missing"), (plist, "missing"),
      (main, "empty"), (daemon, "mode"), (helper, "mode")):
      with self.subTest(path=path.name, defect=defect):
        contents, mode = path.read_bytes(), path.stat().st_mode & 0o7777
        if defect == "missing":
          path.unlink()
        elif defect == "empty":
          path.write_bytes(b"")
        else:
          path.chmod(0o644)
        self.package(self.desktop, self.desktop_archive)
        with self.assertRaises(SystemExit):
          self.verify()
        self.write(path, contents, mode)

  def test_non_tar_bytes_are_rejected(self) -> None:
    self.cli_archive.write_bytes(b"nonempty invalid archive")
    with self.assertRaises(SystemExit):
      self.verify()


if __name__ == "__main__":
  unittest.main()
