"""Verify the Shell icon in a built application or installer on any host."""
from pathlib import Path
import struct
import sys


def resource_icon_groups(pe):
    """Read RT_GROUP_ICON and follow each entry to its RT_ICON image."""
    header = struct.unpack_from("<I", pe, 0x3C)[0]
    assert pe[header:header + 4] == b"PE\0\0"
    section_count = struct.unpack_from("<H", pe, header + 6)[0]
    optional_size = struct.unpack_from("<H", pe, header + 20)[0]
    optional = header + 24
    magic = struct.unpack_from("<H", pe, optional)[0]
    directories = optional + {0x10B: 96, 0x20B: 112}[magic]
    resource_rva = struct.unpack_from("<I", pe, directories + 16)[0]
    sections = [struct.unpack_from("<IIII", pe, optional + optional_size + 40 * i + 8)
                for i in range(section_count)]

    def offset(rva):
        for virtual_size, start, size, pointer in sections:
            if start <= rva < start + max(virtual_size, size):
                return pointer + rva - start
        raise ValueError("Resource outside PE sections")

    base = offset(resource_rva)

    def entries(directory):
        named, ids = struct.unpack_from("<HH", pe, base + directory + 12)
        return [struct.unpack_from("<II", pe, base + directory + 16 + 8 * i)
                for i in range(named + ids)]

    def images(kind):
        result = {}
        for key, directory in entries(0):
            if key != kind:
                continue
            for name, languages in entries(directory & 0x7fffffff):
                for _, leaf in entries(languages & 0x7fffffff):
                    rva, length = struct.unpack_from("<II", pe, base + leaf)
                    pos = offset(rva)
                    result[name] = pe[pos:pos + length]
        return result

    icons = images(3)
    for group in images(14).values():
        count = struct.unpack_from("<H", group, 4)[0]
        frames = {}
        for i in range(count):
            w, h, _, _, _, _, size, image_id = struct.unpack_from("<BBBBHHIH", group, 6 + i * 14)
            assert w == h
            frames[w or 256] = icons[image_id][:size]
        yield frames


def verify(paths):
    assets = Path(__file__).resolve().parents[1] / "assets"
    ico = (assets / "icon.ico").read_bytes()
    expected = {}
    for i in range(struct.unpack_from("<H", ico, 4)[0]):
        w, h, _, _, _, _, size, pos = struct.unpack_from("<BBBBHHII", ico, 6 + 16 * i)
        assert w == h
        expected[w or 256] = ico[pos:pos + size]
    assert {16, 20, 24, 32, 40, 48, 64, 128, 256} == set(expected)
    assert expected[256] == (assets / "icon.png").read_bytes()
    for path in paths:
        if expected not in resource_icon_groups(path.read_bytes()):
            raise ValueError(f"{path.name} is missing the NotchCalendar icon")
        print(f"{path.name}: verified all {len(expected)} product icon sizes")


if __name__ == "__main__":
    verify([Path(arg) for arg in sys.argv[1:]])
