"""Offline regression checks for the CI integrity gate; no SDK is downloaded."""

import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from verify import Archive, SdkArchive, check_metadata, check_outputs, check_sdk


class IntegrityGateTest(unittest.TestCase):
  def setUp(self) -> None:
    self.directory = tempfile.TemporaryDirectory()
    self.addCleanup(self.directory.cleanup)
    self.root = Path(self.directory.name)
    self.sdk = self.root / "sdk"
    self.sdk.mkdir()
    self.data = b"synthetic CI integrity fixture"
    self.archive: Archive = {"name": "cef_fixture.tar.bz2", "size": len(self.data),
      "sha1": hashlib.sha1(self.data).hexdigest(), "type": "minimal"}
    # download-cef's CefFile serializes these three fields to archive.json.
    self.metadata: SdkArchive = {"name": self.archive["name"], "sha1": self.archive["sha1"],
      "type": self.archive["type"]}
    (self.sdk / "archive.json").write_text(json.dumps(self.metadata))
    self.download = self.root / self.archive["name"]
    self.download.write_bytes(self.data)

  def test_verified_download_is_accepted_then_removed(self) -> None:
    check_sdk(self.sdk, self.archive)
    self.assertFalse(self.download.exists())
    self.assertTrue((self.sdk / "archive.json").is_file())

  def test_same_size_corruption_fails_without_removing_evidence(self) -> None:
    self.download.write_bytes(b"!" * len(self.data))
    with self.assertRaisesRegex(SystemExit, "SHA-1"):
      check_sdk(self.sdk, self.archive)
    self.assertTrue(self.download.exists())

  def test_changed_download_size_fails_without_removing_evidence(self) -> None:
    self.download.write_bytes(self.data + b"!")
    with self.assertRaisesRegex(SystemExit, "size differs from pin"):
      check_sdk(self.sdk, self.archive)
    self.assertTrue(self.download.exists())

  def test_optional_sdk_size_is_validated_when_present(self) -> None:
    for size in (len(self.data) + 1, "invalid", None, True):
      with self.subTest(size=size):
        (self.sdk / "archive.json").write_text(json.dumps({**self.metadata, "size": size}))
        with self.assertRaises(SystemExit):
          check_sdk(self.sdk, self.archive)
        self.assertTrue(self.download.exists())
    (self.sdk / "archive.json").write_text(json.dumps({**self.archive, "extra": "ignored"}))
    check_sdk(self.sdk, self.archive)
    self.assertFalse(self.download.exists())

  def test_invalid_sdk_identity_preserves_download(self) -> None:
    for key, value in (("name", "other.tar.bz2"), ("sha1", "0" * 40),
      ("type", "standard"), ("sha1", None)):
      with self.subTest(key=key, value=value):
        (self.sdk / "archive.json").write_text(json.dumps({**self.metadata, key: value}))
        with self.assertRaises(SystemExit):
          check_sdk(self.sdk, self.archive)
        self.assertTrue(self.download.exists())

  def test_vendor_index_still_requires_size(self) -> None:
    with self.assertRaisesRegex(SystemExit, "invalid field types"):
      check_metadata(self.metadata, self.archive)

  def test_changed_vendor_metadata_cannot_replace_the_pin(self) -> None:
    changed = {**self.archive, "name": "cef_newer.tar.bz2"}
    with self.assertRaisesRegex(SystemExit, "differs from pin"):
      check_metadata(changed, self.archive)

  def test_real_pin_shape_ignores_vendor_timestamp(self) -> None:
    pin = json.loads(Path(__file__).with_name("cef-sdk.json").read_text())
    for selected in pin["targets"].values():
      expected: Archive = selected["archive"]
      actual = {**expected, "last_modified": "different vendor index timestamp"}
      check_metadata(actual, expected)

  def test_empty_msi_files_do_not_satisfy_native_payload_acceptance(self) -> None:
    for name in ("cli.msi", "desktop.msi"):
      (self.root / name).touch()
    with self.assertRaisesRegex(SystemExit, "Windows native payload"):
      check_outputs(self.root, "x86_64-pc-windows-msvc")


if __name__ == "__main__":
  unittest.main()
