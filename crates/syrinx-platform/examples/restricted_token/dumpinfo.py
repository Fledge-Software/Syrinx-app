# Diagnostic for #59: print each minidump's exception code, faulting module and WebView2 process type.
import pathlib
import re
import struct
import sys

# Minidump layout constants (MINIDUMP_HEADER, MINIDUMP_DIRECTORY, MINIDUMP_MODULE).
SIGNATURE = b"MDMP"
HEADER_STREAM_COUNT_OFFSET = 8
DIRECTORY_ENTRY_SIZE = 12
MODULE_LIST_STREAM = 4
EXCEPTION_STREAM = 6
MODULE_SIZE = 108
MODULE_NAME_OFFSET = 20
EXCEPTION_RECORD_OFFSET = 8


class DumpError(Exception):
    """A dump that is not a minidump or ends before a structure it points to."""


# Unpack a structure, naming a truncated dump instead of raising a bare struct.error.
def read_struct(layout, data, offset):
    try:
        return struct.unpack_from(layout, data, offset)
    except struct.error as error:
        raise DumpError(f"truncated at offset {offset}") from error


# Map each stream type to its (size, offset).
def stream_directory(data):
    if data[:4] != SIGNATURE:
        raise DumpError("not a minidump")
    count, directory = read_struct("<II", data, HEADER_STREAM_COUNT_OFFSET)
    streams = {}
    for index in range(count):
        kind, size, offset = read_struct("<III", data, directory + DIRECTORY_ENTRY_SIZE * index)
        streams[kind] = (size, offset)
    return streams


# Read a length-prefixed UTF-16 MINIDUMP_STRING.
def minidump_string(data, offset):
    (length,) = read_struct("<I", data, offset)
    raw = data[offset + 4 : offset + 4 + length]
    if len(raw) != length:
        raise DumpError(f"truncated string at offset {offset}")
    return raw.decode("utf-16-le", "replace")


# List (base, size, file name) for each loaded module.
def modules(data, streams):
    if MODULE_LIST_STREAM not in streams:
        return []
    _, offset = streams[MODULE_LIST_STREAM]
    (count,) = read_struct("<I", data, offset)
    loaded = []
    for index in range(count):
        entry = offset + 4 + MODULE_SIZE * index
        base, size = read_struct("<QI", data, entry)
        (name_offset,) = read_struct("<I", data, entry + MODULE_NAME_OFFSET)
        loaded.append((base, size, pathlib.PureWindowsPath(minidump_string(data, name_offset)).name))
    return loaded


# Return (code, address) from the exception stream, or None when the dump has none.
def exception(data, streams):
    if EXCEPTION_STREAM not in streams:
        return None
    _, offset = streams[EXCEPTION_STREAM]
    code, _flags, _record, address = read_struct("<IIQQ", data, offset + EXCEPTION_RECORD_OFFSET)
    return code, address


# Name the module and offset that contain an address.
def locate(address, loaded):
    for base, size, name in loaded:
        if base <= address < base + size:
            return f"{name}+0x{address - base:x}"
    return "?"


# Find the lowercase words that follow a prefix, in one text encoding of the raw bytes.
def scan_words(data, prefix, encoding):
    unit_padding = b"\x00" if encoding == "utf-16-le" else b""
    pattern = re.escape(prefix.encode(encoding)) + rb"((?:[a-z\-]" + unit_padding + rb"){3,30})"
    return {match.group(1).decode(encoding) for match in re.finditer(pattern, data)}


# Heuristic: Crashpad's "ptype" annotation is stored as plain text a few bytes after its key.
def annotated_process_type(data):
    match = re.search(rb"ptype[\x00-\x20]{0,16}([a-z\-]{3,30})", data)
    return match.group(1).decode() if match else "?"


# Describe one dump in a single line.
def describe(path):
    with open(path, "rb") as dump:
        data = dump.read()
    streams = stream_directory(data)
    raised = exception(data, streams)
    if raised is None:
        code_text, where = "?", "?"
    else:
        code, address = raised
        code_text, where = f"0x{code:08X}", locate(address, modules(data, streams))
    process_types = sorted(scan_words(data, "--type=", "ascii") | scan_words(data, "--type=", "utf-16-le"))
    return f"code={code_text} at={where} ptype={annotated_process_type(data)} types={process_types}"


# Print one line per dump; exit non-zero if any dump could not be read.
def main(paths):
    if not paths:
        print("usage: dumpinfo.py <minidump>...", file=sys.stderr)
        return 2
    failed = False
    for path in paths:
        name = pathlib.Path(path).name
        try:
            print(f"{name}: {describe(path)}")
        except (OSError, DumpError) as error:
            print(f"{name}: cannot read ({error})")
            failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
