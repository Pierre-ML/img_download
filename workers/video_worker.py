"""
VideoWorker — téléchargement vidéo via yt-dlp
Supporte YouTube, Twitter/X, Reddit, Vimeo, liens directs…
"""

import threading
import re
from pathlib import Path

import yt_dlp


class VideoWorker(threading.Thread):

    def __init__(self, job_id: str, urls: list[str], dest: str, event_cb):
        super().__init__(daemon=True)
        self.job_id   = job_id
        self.urls     = [u.strip() for u in urls if u.strip()]
        self.dest     = dest
        self.event_cb = event_cb
        self._stop    = threading.Event()
        self.out_dir  = None

    # ------------------------------------------------------------------ API
    def stop(self):
        self._stop.set()

    # ---------------------------------------------------------------- events
    def _log(self, text: str):
        self.event_cb({"type": "log", "text": text})

    def _progress(self, current: int, total: int):
        self.event_cb({"type": "progress", "current": current, "total": total})

    def _thumb(self, filename: str):
        self.event_cb({"type": "thumb", "filename": filename})

    # ------------------------------------------------------------------ run
    def run(self):
        out_dir = Path(self.dest) / "videos"
        out_dir.mkdir(parents=True, exist_ok=True)
        self.out_dir = out_dir

        total   = len(self.urls)
        success = 0
        failed  = 0

        for idx, url in enumerate(self.urls, 1):
            if self._stop.is_set():
                self._log("Arrêté par l'utilisateur.")
                break

            self._log(f"[{idx}/{total}] {url[:90]}…")

            saved_files: list[str] = []

            def hook(d, _files=saved_files):
                status = d.get("status")
                if status == "downloading":
                    pct   = d.get("_percent_str", "").strip()
                    speed = d.get("_speed_str",   "").strip()
                    eta   = d.get("_eta_str",     "").strip()
                    if pct:
                        self._log(f"  ↓ {pct}  {speed}  ETA {eta}")
                elif status == "finished":
                    fname = Path(d["filename"]).name
                    _files.append(fname)
                    self._log(f"  ✓ {fname}")

            ydl_opts = {
                "outtmpl":              str(out_dir / "%(title).80s.%(ext)s"),
                "format":               "bestvideo+bestaudio/best",
                "merge_output_format":  "mp4",
                "noplaylist":           True,
                "quiet":                True,
                "no_warnings":          True,
                "progress_hooks":       [hook],
                "retries":              3,
            }

            try:
                with yt_dlp.YoutubeDL(ydl_opts) as ydl:
                    ydl.download([url])
                success += 1
                self._progress(success, total)
                for fname in saved_files:
                    self._thumb(fname)
            except yt_dlp.utils.DownloadError as e:
                msg = re.sub(r"\x1b\[[0-9;]*m", "", str(e))   # strip ANSI
                self._log(f"  ✗ {msg[:120]}")
                failed += 1
            except Exception as e:
                self._log(f"  ✗ {e}")
                failed += 1

        msg = f"{success} vidéo(s) téléchargée(s), {failed} échec(s)."
        self._log(f"\n✅ {msg}")
        self.event_cb({"type": "done", "success": True, "message": msg})
