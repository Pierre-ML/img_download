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

# ── Comptes (Reddit, X) — fichier local gitignoré ────────────────────────────
ACCOUNTS_FILE = Path(__file__).parent / "accounts.json"

# ── Chrome (undetected-chromedriver) ─────────────────────────────────────────
# À mettre à jour si Chrome est mis à jour sur la machine.
CHROME_VERSION = int(os.getenv("CHROME_VERSION", "146"))

# ── Worker — images ──────────────────────────────────────────────────────────
IMG_PARALLEL     = int(os.getenv("IMG_PARALLEL", "4"))  # téléchargements simultanés
IMG_MIN_SIZE     = 300    # pixels — images plus petites ignorées
IMG_SCROLL_MAX   = 60     # tentatives de scroll max avant abandon
IMG_REQUEST_TIMEOUT = 15  # secondes

# ── Worker — Reddit ──────────────────────────────────────────────────────────
REDDIT_PARALLEL      = int(os.getenv("REDDIT_PARALLEL", "4"))  # images simultanées
REDDIT_DELAY         = 0.6   # secondes entre requêtes de listing Reddit (rate-limit)
REDDIT_MIN_SIZE      = 100   # pixels — images plus petites ignorées
REDDIT_REQUEST_TIMEOUT = 15  # secondes
