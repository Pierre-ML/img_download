"""
Utilitaires image sans PIL — parsing pur Python des magic bytes
(format, dimensions, extension) + noms de fichiers sûrs.
"""
import re
import struct
from pathlib import Path

# Caractères interdits dans un nom de fichier Windows + caractères de contrôle
_FORBIDDEN_RE = re.compile(r'[\\/:*?"<>|\x00-\x1f]')


def detect_ext(raw: bytes) -> str | None:
    """Retourne l'extension réelle d'après les magic bytes, ou None si inconnu."""
    if raw[:2] == b'\xff\xd8':
        return ".jpg"
    if raw[:8] == b'\x89PNG\r\n\x1a\n':
        return ".png"
    if raw[:6] in (b'GIF87a', b'GIF89a'):
        return ".gif"
    if raw[:4] == b'RIFF' and len(raw) > 12 and raw[8:12] == b'WEBP':
        return ".webp"
    return None


def is_valid_image(raw: bytes) -> bool:
    """Vérifie les magic bytes pour JPEG, PNG, GIF et WebP."""
    return detect_ext(raw) is not None


def sanitize_filename(name: str, max_len: int = 60, fallback: str = "media") -> str:
    """Nettoie un titre pour en faire un nom de fichier valide (Windows inclus)."""
    name = _FORBIDDEN_RE.sub("_", name)
    name = re.sub(r"\s+", " ", name).strip()
    # Windows refuse les noms finissant par un point ou un espace
    name = name[:max_len].rstrip(" ._")
    return name or fallback


def unique_path(directory: Path, stem: str, ext: str) -> Path:
    """Retourne un chemin qui n'écrase aucun fichier existant (suffixe _2, _3…)."""
    fp = directory / f"{stem}{ext}"
    n = 2
    while fp.exists():
        fp = directory / f"{stem}_{n}{ext}"
        n += 1
    return fp


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
