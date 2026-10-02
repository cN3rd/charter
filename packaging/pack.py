"""Packs a release folder into a .tar.gz or .zip that is the same byte for byte wherever and
whenever it is built from the same files: entries sorted by name, every file's time the commit's
($SOURCE_DATE_EPOCH), owners and permissions fixed, and nothing of the machine recorded.

    python3 packaging/pack.py dist/charter-0.0.0-linux-x86_64 dist/charter-0.0.0-linux-x86_64.tar.gz
    python3 packaging/pack.py selftest
"""

import gzip
import io
import os
import sys
import tarfile
import tempfile
import time
import zipfile

# A zip file's dates start in 1980.
ZIP_EPOCH = 315532800


def entries(folder):
    """The folder's directories and files, folder first, as (archive name, path, is_dir), sorted."""
    root = os.path.basename(os.path.normpath(folder))
    found = [(root, folder, True)]
    for here, dirs, files in os.walk(folder):
        relative = os.path.relpath(here, folder)
        for name in dirs + files:
            path = os.path.join(here, name)
            parts = [root] if relative == "." else [root, *relative.split(os.sep)]
            found.append(("/".join([*parts, name]), path, os.path.isdir(path)))
    return sorted(found)


def mode(name, is_dir):
    """The permissions an entry is packed with: executables and folders 755, the rest 644."""
    leaf = name.rsplit("/", 1)[-1]
    return 0o755 if is_dir or leaf in ("charter", "charter.exe") else 0o644


def pack(folder, archive, epoch):
    """Writes `folder` to `archive`, a .tar.gz or a .zip, stamped with `epoch`."""
    listed = entries(folder)
    if archive.endswith(".tar.gz"):
        with open(archive, "wb") as out, gzip.GzipFile(
            filename="", fileobj=out, mode="wb", compresslevel=9, mtime=epoch
        ) as packed, tarfile.open(fileobj=packed, mode="w", format=tarfile.GNU_FORMAT) as tar:
            for name, path, is_dir in listed:
                info = tarfile.TarInfo(name)
                info.mtime, info.mode = epoch, mode(name, is_dir)
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                if is_dir:
                    info.type = tarfile.DIRTYPE
                    tar.addfile(info)
                else:
                    with open(path, "rb") as source:
                        data = source.read()
                    info.size = len(data)
                    tar.addfile(info, io.BytesIO(data))
    elif archive.endswith(".zip"):
        stamp = time.gmtime(max(epoch, ZIP_EPOCH))[:6]
        with zipfile.ZipFile(archive, "w") as packed:
            for name, path, is_dir in listed:
                info = zipfile.ZipInfo(name + "/" if is_dir else name, date_time=stamp)
                info.create_system = 3  # Unix, so the permissions below are read
                kind = 0o040000 if is_dir else 0o100000
                info.external_attr = (mode(name, is_dir) | kind) << 16
                if is_dir:
                    packed.writestr(info, b"")
                else:
                    info.compress_type = zipfile.ZIP_DEFLATED
                    with open(path, "rb") as source:
                        packed.writestr(info, source.read(), compresslevel=9)
    else:
        sys.exit(f"{archive}: not a .tar.gz or a .zip")


def selftest():
    """Packs the same files made at different times, in a different order, and compares."""
    for suffix in (".tar.gz", ".zip"):
        made = []
        for run, order in enumerate((("b.txt", "a.txt"), ("a.txt", "b.txt"))):
            with tempfile.TemporaryDirectory() as scratch:
                folder = os.path.join(scratch, "charter-test")
                os.makedirs(os.path.join(folder, "licenses"))
                for name in order:
                    path = os.path.join(folder, "licenses", name)
                    with open(path, "w") as file:
                        file.write(name)
                    os.utime(path, (run * 1000, run * 1000))
                with open(os.path.join(folder, "charter"), "wb") as file:
                    file.write(b"\x7fELF")
                archive = os.path.join(scratch, "out" + suffix)
                pack(folder, archive, 1_700_000_000)
                with open(archive, "rb") as file:
                    made.append(file.read())
        assert made[0] == made[1], f"{suffix} differs between runs"
    print("ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["selftest"]:
        selftest()
    elif len(sys.argv) == 3:
        pack(sys.argv[1], sys.argv[2], int(os.environ.get("SOURCE_DATE_EPOCH") or 0))
    else:
        sys.exit(__doc__)
