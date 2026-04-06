"""
Utilitaires image sans PIL — parsing pur Python des magic bytes.
"""
import struct


def is_valid_image(raw: bytes) -> bool:
    """Vérifie les magic bytes pour JPEG, PNG, GIF et WebP."""
    if raw[:2] == b'\xff\xd8':
        return True
    if raw[:8] == b'\x89PNG\r\n\x1a\n':
        return True
    if raw[:6] in (b'GIF87a', b'GIF89a'):
        return True
    if raw[:4] == b'RIFF' and len(raw) > 12 and raw[8:12] == b'WEBP':
        return True
    return False


def image_dimensions(raw: bytes):
    """Retourne (width, height) depuis les octets bruts, sans PIL."""
    # PNG
    if raw[:8] == b'\x89PNG\r\n\x1a\n':
        w, h = struct.unpack('>II', raw[16:24])
        return w, h
    # JPEG — cherche un marqueur SOF
    if raw[:2] == b'\xff\xd8':
        i = 2
        while i + 4 < len(raw):
            if raw[i] != 0xff:
                break
            marker = raw[i + 1]
            if marker in (0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7):
                h, w = struct.unpack('>HH', raw[i + 5:i + 9])
                return w, h
            length = struct.unpack('>H', raw[i + 2:i + 4])[0]
            i += 2 + length
        return None, None
    # GIF
    if raw[:6] in (b'GIF87a', b'GIF89a'):
        w, h = struct.unpack('<HH', raw[6:10])
        return w, h
    # WebP
    if raw[:4] == b'RIFF' and len(raw) > 30 and raw[8:12] == b'WEBP':
        if raw[12:16] == b'VP8 ':
            w = struct.unpack('<H', raw[26:28])[0] & 0x3fff
            h = struct.unpack('<H', raw[28:30])[0] & 0x3fff
            return w, h
        if raw[12:16] == b'VP8L':
            bits = struct.unpack('<I', raw[21:25])[0]
            w = (bits & 0x3fff) + 1
            h = ((bits >> 14) & 0x3fff) + 1
            return w, h
        if raw[12:16] == b'VP8X':
            w = struct.unpack('<I', raw[24:27] + b'\x00')[0] + 1
            h = struct.unpack('<I', raw[27:30] + b'\x00')[0] + 1
            return w, h
    return None, None
