"""
VideoWorker — téléchargement vidéo via yt-dlp.
Supporte YouTube, Twitter/X, Reddit, Vimeo, liens directs…
Fichiers nommés `Titre [id].mp4` — l'id évite qu'une vidéo
homonyme écrase ou bloque la précédente.
"""

import re
import threading
from pathlib import Path
from typing import Callable

import yt_dlp

# ── Constantes ────────────────────────────────────────────────────────────────
_ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")   # pour nettoyer les codes couleur ANSI
_YDL_OPTS_BASE: dict = {
    "format":              "bestvideo+bestaudio/best",
    "merge_output_format": "mp4",
    "noplaylist":          True,
    "quiet":               True,
    "no_warnings":         True,
    "retries":             3,
    "concurrent_fragment_downloads": 4,
}
_LOG_STEP_PCT = 5.0   # granularité des logs de progression


class VideoWorker(threading.Thread):
    """Thread de téléchargement vidéo via yt-dlp."""

    def __init__(self, job_id: str, urls: list[str],
                 dest: str, event_cb: Callable[[dict], None]) -> None:
        super().__init__(daemon=True)
        self.job_id  = job_id
        self.urls    = [u.strip() for u in urls if u.strip()]
        self.dest    = dest
        self._cb     = event_cb
        self._stop   = threading.Event()
        self.out_dir: Path | None = None

    def stop(self) -> None:
        self._stop.set()

    # ── Émission d'événements ─────────────────────────────────────────────────
    def _log(self, text: str) -> None:
        self._cb({"type": "log", "text": text})

    def _progress(self, current: int, total: int) -> None:
        self._cb({"type": "progress", "current": current, "total": total})

    def _thumb(self, filename: str) -> None:
        self._cb({"type": "thumb", "filename": filename})

    # ── Thread principal ──────────────────────────────────────────────────────
    def run(self) -> None:
        out_dir = Path(self.dest) / "videos"
        out_dir.mkdir(parents=True, exist_ok=True)
        self.out_dir = out_dir

        total   = len(self.urls)
        success = failed = 0

        for idx, url in enumerate(self.urls, 1):
            if self._stop.is_set():
                self._log("Arrêté par l'utilisateur.")
                break

            self._log(f"[{idx}/{total}] {url[:90]}…")
            saved:    list[str]   = []
            last_pct: list[float] = [-_LOG_STEP_PCT]

            def hook(d: dict, _saved: list[str] = saved,
                     _last: list[float] = last_pct) -> None:
                if d.get("status") == "downloading":
                    raw_pct = _ANSI_RE.sub("", d.get("_percent_str") or "")
                    try:
                        pct = float(raw_pct.strip().rstrip("%"))
                    except ValueError:
                        return
                    if pct - _last[0] >= _LOG_STEP_PCT:
                        _last[0] = pct
                        speed = _ANSI_RE.sub("", d.get("_speed_str") or "").strip()
                        eta   = _ANSI_RE.sub("", d.get("_eta_str")   or "").strip()
                        self._log(f"  ↓ {pct:.0f}%  {speed}  ETA {eta}")
                elif d.get("status") == "finished":
                    fname = Path(d["filename"]).name
                    _saved.append(fname)
                    _last[0] = -_LOG_STEP_PCT
                    self._log(f"  ✓ {fname}")

            ydl_opts = {
                **_YDL_OPTS_BASE,
                "outtmpl":        str(out_dir / "%(title).80s [%(id)s].%(ext)s"),
                "progress_hooks": [hook],
            }

            try:
                with yt_dlp.YoutubeDL(ydl_opts) as ydl:
                    ydl.download([url])
                success += 1
                self._progress(success, total)
                for fname in saved:
                    self._thumb(fname)
            except yt_dlp.utils.DownloadError as exc:
                self._log(f"  ✗ {_ANSI_RE.sub('', str(exc))[:120]}")
                failed += 1
            except Exception as exc:
                self._log(f"  ✗ {exc}")
                failed += 1

        msg = f"{success} vidéo(s) téléchargée(s), {failed} échec(s)."
        self._log(f"\n✅ {msg}")
        self._cb({"type": "done", "success": True, "message": msg})
