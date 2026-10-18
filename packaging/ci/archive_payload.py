"""Compare final tar payloads to staging without extracting untrusted members."""

from dataclasses import dataclass
import hashlib
import os
from pathlib import Path
import stat
import tarfile
from typing import BinaryIO, Literal, NoReturn


@dataclass(frozen=True)
class Entry:
  kind: Literal["directory", "file", "symlink", "hardlink"]
  mode: int
  size: int = 0
  digest: str = ""
  target: str = ""


def fail(message: str) -> NoReturn:
  raise SystemExit(f"Native tar payload: {message}")


def content_hash(stream: BinaryIO) -> str:
  checksum = hashlib.sha256()
  while chunk := stream.read(1024 * 1024):
    checksum.update(chunk)
  return checksum.hexdigest()


def member_path(name: str, prefix: str) -> str:
  # A trailing slash is tar's directory spelling, not another entry identity.
  name = name.rstrip("/")
  parts = name.split("/")
  if not name or any(part in ("", ".", "..") for part in parts):
    fail(f"unsafe member path {name!r}")
  if parts[0] != prefix or "\\" in name:
    fail(f"member is outside {prefix!r}: {name!r}")
  return name


def resolve_link(name: str, entries: dict[str, Entry], prefix: str) -> str:
  pending = name.split("/")
  resolved: list[str] = []
  expansions = 0
  while pending:
    part = pending.pop(0)
    if part in ("", "."):
      continue
    if part == "..":
      if len(resolved) <= 1:
        fail(f"escaping link {name!r}")
      resolved.pop()
      continue
    resolved.append(part)
    current = "/".join(resolved)
    entry = entries.get(current)
    if entry is None:
      fail(f"broken link {name!r} at {current!r}")
    if entry.kind in ("symlink", "hardlink"):
      expansions += 1
      if expansions > 100:
        fail(f"cyclic or excessive link chain {name!r}")
      if not entry.target or entry.target.startswith("/") or "\\" in entry.target:
        fail(f"unsafe link target for {current!r}: {entry.target!r}")
      target = entry.target.split("/")
      if entry.kind == "hardlink":
        member_path(entry.target, prefix)
        resolved = []
      else:
        resolved.pop()
      pending = target + pending
    elif pending and entry.kind != "directory":
      fail(f"link traverses a non-directory: {name!r}")
  return "/".join(resolved)


def check_links(entries: dict[str, Entry], prefix: str) -> None:
  for name, entry in entries.items():
    if entry.kind in ("symlink", "hardlink"):
      target = resolve_link(name, entries, prefix)
      if entry.kind == "hardlink" and entries[target].kind != "file":
        fail(f"hardlink does not resolve to a regular file: {name!r}")


def staged_entries(source: Path) -> dict[str, Entry]:
  entries: dict[str, Entry] = {}

  def visit(path: Path, name: str) -> None:
    metadata = path.lstat()
    mode = stat.S_IMODE(metadata.st_mode)
    if stat.S_ISLNK(metadata.st_mode):
      entries[name] = Entry("symlink", mode, target=os.readlink(path))
    elif stat.S_ISDIR(metadata.st_mode):
      entries[name] = Entry("directory", mode)
      # scandir batches metadata; file contents use bounded reads, once per file.
      with os.scandir(path) as children:
        for child in children:
          visit(Path(child.path), f"{name}/{child.name}")
    elif stat.S_ISREG(metadata.st_mode):
      with path.open("rb") as stream:
        entries[name] = Entry("file", mode, metadata.st_size, content_hash(stream))
    else:
      fail(f"unsupported staged entry {path}")

  visit(source, source.name)
  if entries[source.name].kind != "directory":
    fail(f"staged root must be a directory: {source}")
  check_links(entries, source.name)
  return entries


def tar_entries(archive: Path, prefix: str) -> dict[str, Entry]:
  entries: dict[str, Entry] = {}
  # One archive context and a sequential stream; never materialize file bodies.
  with tarfile.open(archive, "r|gz") as bundle:
    for member in bundle:
      name = member_path(member.name, prefix)
      if name in entries:
        fail(f"duplicate member {name!r}")
      mode = member.mode & 0o7777
      if member.isdir():
        entry = Entry("directory", mode)
      elif member.isfile():
        stream = bundle.extractfile(member)
        if stream is None:
          fail(f"unreadable file {name!r}")
        with stream:
          entry = Entry("file", mode, member.size, content_hash(stream))
      elif member.issym():
        entry = Entry("symlink", mode, target=member.linkname)
      elif member.islnk():
        entry = Entry("hardlink", mode, target=member.linkname)
      else:
        fail(f"unsupported archive entry {name!r}")
      entries[name] = entry
  check_links(entries, prefix)
  # Hardlinks are file content aliases, resolved only against checked members.
  for name, entry in tuple(entries.items()):
    if entry.kind == "hardlink":
      target = entries[resolve_link(name, entries, prefix)]
      entries[name] = Entry("file", entry.mode, target.size, target.digest)
  return entries


def check_archive_payload(archive: Path, source: Path) -> None:
  try:
    expected = staged_entries(source)
    actual = tar_entries(archive, source.name)
    if actual.keys() != expected.keys():
      missing = sorted(expected.keys() - actual.keys())[:5]
      extra = sorted(actual.keys() - expected.keys())[:5]
      fail(f"{archive.name} entries differ; missing={missing}, extra={extra}")
    for name, entry in expected.items():
      if actual[name] != entry:
        fail(f"{archive.name} differs from staging at {name!r}")
  except (OSError, tarfile.TarError, EOFError) as error:
    fail(f"cannot inspect {archive.name}: {error}")
