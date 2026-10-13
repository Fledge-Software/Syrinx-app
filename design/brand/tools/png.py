"""Reads and writes 8-bit RGBA PNGs with the standard library, so the brand tools need no packages."""

import struct
import zlib

SIGNATURE = b"\x89PNG\r\n\x1a\n"
RGBA = 6
RGB = 2


class Image:
    """A decoded image: `pixels` holds `width * height` RGBA tuples, row by row."""

    def __init__(self, width, height, pixels):
        self.width = width
        self.height = height
        self.pixels = pixels

    def pixel(self, x, y):
        return self.pixels[y * self.width + x]


def read(path):
    """Decodes a non-interlaced 8-bit RGB or RGBA PNG, the only kinds Edge writes."""
    data = path.read_bytes()
    if not data.startswith(SIGNATURE):
        raise ValueError(f"{path} is not a PNG")
    header, compressed = _read_chunks(data, path)
    width, height, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", header)
    if depth != 8 or colour not in (RGB, RGBA) or interlace != 0:
        raise ValueError(f"{path}: unsupported PNG format (depth {depth}, colour {colour})")
    channels = 4 if colour == RGBA else 3
    rows = _unfilter(zlib.decompress(compressed), width * channels, height, channels)
    return Image(width, height, _to_rgba(rows, width, channels))


def write(path, image):
    """Encodes RGBA with no ancillary chunks, so the same pixels always give the same bytes."""
    raw = bytearray()
    for y in range(image.height):
        raw.append(0)
        for pixel in image.pixels[y * image.width:(y + 1) * image.width]:
            raw.extend(pixel)
    header = struct.pack(">IIBBBBB", image.width, image.height, 8, RGBA, 0, 0, 0)
    path.write_bytes(
        SIGNATURE
        + _chunk(b"IHDR", header)
        + _chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + _chunk(b"IEND", b"")
    )


def _read_chunks(data, path):
    # Collect the header and the concatenated image data; ancillary chunks are ignored.
    offset = len(SIGNATURE)
    header = None
    compressed = bytearray()
    while offset < len(data):
        length, kind = struct.unpack(">I4s", data[offset:offset + 8])
        body = data[offset + 8:offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            header = body
        elif kind == b"IDAT":
            compressed.extend(body)
        elif kind == b"IEND":
            break
    if header is None:
        raise ValueError(f"{path} has no IHDR chunk")
    return header, bytes(compressed)


def _unfilter(raw, stride, height, channels):
    # Undo each row's PNG filter (none, sub, up, average, Paeth).
    rows = []
    previous = bytearray(stride)
    offset = 0
    for _ in range(height):
        kind = raw[offset]
        row = bytearray(raw[offset + 1:offset + 1 + stride])
        offset += 1 + stride
        for index in range(stride):
            left = row[index - channels] if index >= channels else 0
            up = previous[index]
            up_left = previous[index - channels] if index >= channels else 0
            row[index] = (row[index] + _predict(kind, left, up, up_left)) & 0xFF
        rows.append(row)
        previous = row
    return rows


def _predict(kind, left, up, up_left):
    if kind == 0:
        return 0
    if kind == 1:
        return left
    if kind == 2:
        return up
    if kind == 3:
        return (left + up) // 2
    if kind == 4:
        return _paeth(left, up, up_left)
    raise ValueError(f"unknown PNG filter {kind}")


def _paeth(left, up, up_left):
    estimate = left + up - up_left
    distances = (abs(estimate - left), abs(estimate - up), abs(estimate - up_left))
    if distances[0] <= distances[1] and distances[0] <= distances[2]:
        return left
    if distances[1] <= distances[2]:
        return up
    return up_left


def _to_rgba(rows, width, channels):
    pixels = []
    for row in rows:
        for x in range(width):
            values = tuple(row[x * channels:(x + 1) * channels])
            pixels.append(values if channels == 4 else values + (255,))
    return pixels


def _chunk(kind, body):
    checksum = zlib.crc32(kind + body) & 0xFFFFFFFF
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", checksum)
