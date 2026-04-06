"""
config.py — constantes et configuration de l'application.
Toutes les valeurs modifiables sont ici ; ne pas les disperser dans le code.
"""

import os
from pathlib import Path

# ── Serveur ──────────────────────────────────────────────────────────────────
HOST = os.getenv("HOST", "0.0.0.0")
PORT = int(os.getenv("PORT", "5000"))

# ── Dossier de sortie par défaut ──────────────────────────────────────────────
DEFAULT_DEST = os.getenv(
    "MEDIA_DEST",
    str(Path.home() / "Pictures" / "MediaDownloader"),
)

# ── Chrome (undetected-chromedriver) ─────────────────────────────────────────
# À mettre à jour si Chrome est mis à jour sur la machine.
CHROME_VERSION = int(os.getenv("CHROME_VERSION", "146"))

# ── Worker — images ──────────────────────────────────────────────────────────
IMG_DELAY_MIN    = 1.0    # secondes entre deux téléchargements (min)
IMG_DELAY_MAX    = 3.0    # secondes entre deux téléchargements (max)
IMG_MIN_SIZE     = 300    # pixels — images plus petites ignorées
IMG_SCROLL_MAX   = 60     # tentatives de scroll max avant abandon
IMG_REQUEST_TIMEOUT = 15  # secondes

# ── Worker — Reddit ──────────────────────────────────────────────────────────
REDDIT_DELAY         = 0.6   # secondes entre requêtes Reddit (rate-limit)
REDDIT_MIN_SIZE      = 100   # pixels — images plus petites ignorées
REDDIT_REQUEST_TIMEOUT = 15  # secondes
