"""Packs PNG files into a Windows .ico whose entries stay PNG-compressed (Vista and later)."""

import struct

ICON_TYPE = 1
DIRECTORY_SIZE = 6
ENTRY_SIZE = 16


def pack(path, images):
    """Writes `images`, a list of (size, png_bytes) pairs, smallest first, as one .ico."""
    images = sorted(images)
    offset = DIRECTORY_SIZE + ENTRY_SIZE * len(images)
    directory = struct.pack("<HHH", 0, ICON_TYPE, len(images))
    entries = b""
    for size, data in images:
        entries += _entry(size, len(data), offset)
        offset += len(data)
    path.write_bytes(directory + entries + b"".join(data for _, data in images))


def sizes(path):
    """Reads back the entry sizes, so the build can check the .ico it just wrote."""
    data = path.read_bytes()
    reserved, kind, count = struct.unpack("<HHH", data[:DIRECTORY_SIZE])
    if reserved != 0 or kind != ICON_TYPE:
        raise ValueError(f"{path} is not an icon file")
    found = []
    for index in range(count):
        start = DIRECTORY_SIZE + ENTRY_SIZE * index
        width, height, _, _, planes, bits, length, offset = struct.unpack(
            "<BBBBHHII", data[start:start + ENTRY_SIZE]
        )
        if not data[offset:offset + length].startswith(b"\x89PNG"):
            raise ValueError(f"{path}: entry {index} is not PNG-compressed")
        if planes != 1 or bits != 32:
            raise ValueError(f"{path}: entry {index} is not 32-bit")
        found.append((width or 256, height or 256))
    return found


def _entry(size, length, offset):
    # A 256-pixel side is stored as 0; colour count and reserved are 0 for 32-bit images.
    if not 1 <= size <= 256:
        raise ValueError(f"icon entries must be 1 to 256 pixels, not {size}")
    side = 0 if size == 256 else size
    return struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, length, offset)
