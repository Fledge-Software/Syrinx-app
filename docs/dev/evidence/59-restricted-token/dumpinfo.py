# Diagnostic for #59: print exception code, faulting module and Crashpad process type from a synthetic minidump.
import struct, sys, re

def streams(data):
    count, rva = struct.unpack_from("<II", data, 8)
    for i in range(count):
        kind, size, at = struct.unpack_from("<III", data, rva + 12 * i)
        yield kind, size, at

def utf16(data, rva):
    length = struct.unpack_from("<I", data, rva)[0]
    return data[rva + 4: rva + 4 + length].decode("utf-16-le", "replace")

def modules(data, at):
    count = struct.unpack_from("<I", data, at)[0]
    for i in range(count):
        base, size = struct.unpack_from("<QI", data, at + 4 + 108 * i)
        name_rva = struct.unpack_from("<I", data, at + 4 + 108 * i + 20)[0]
        yield base, size, utf16(data, name_rva).rsplit("\\", 1)[-1]

for path in sys.argv[1:]:
    data = open(path, "rb").read()
    found = dict((k, (s, a)) for k, s, a in streams(data))
    code, address = None, None
    if 6 in found:
        _, at = found[6]
        code, flags, record, address = struct.unpack_from("<IIQQ", data, at + 8)
    mods = list(modules(data, found[4][1])) if 4 in found else []
    owner = next((f"{name}+0x{address - base:x}" for base, size, name in mods if address and base <= address < base + size), "?")
    # Crashpad annotations carry the process type as plain text in the dump.
    ptype = re.search(rb"ptype[\x00-\x20]{0,16}([a-z\-]{3,30})", data)
    ascii_types = set(m.group(1).decode() for m in re.finditer(rb"--type=([a-z\-]+)", data))
    utf16_types = set(m.group(1).decode("utf-16-le") for m in re.finditer(rb"-\x00-\x00t\x00y\x00p\x00e\x00=\x00((?:[a-z\-]\x00){3,30})", data))
    print(f"{path.rsplit(chr(92),1)[-1]}: code=0x{code or 0:08X} at={owner} ptype={ptype.group(1).decode() if ptype else '?'} types={sorted(ascii_types | utf16_types)}")
