"""
XLikesWorker — téléchargement des likes du compte X (Twitter) via gallery-dl.
Auth par cookies de navigateur (session X ouverte) ou fichier cookies.txt :
X ne permet plus de voir les likes que de son propre compte connecté.
Nommage : `auteur_AAAA-MM-JJ_tweetid_num.ext`.
"""

import os
import subprocess
import sys
import threading
from pathlib import Path
from typing import Callable

_FILENAME_FMT = "{author[name]}_{date:%Y-%m-%d}_{tweet_id}_{num}.{extension}"


class XLikesWorker(threading.Thread):
    """Thread de téléchargement des likes X via gallery-dl (sous-processus)."""

    def __init__(self, job_id: str, count: int, dest: str,
                 event_cb: Callable[[dict], None], creds: dict) -> None:
        super().__init__(daemon=True)
        self.job_id = job_id
        self.count  = count
        self.dest   = dest
        self._cb    = event_cb
        self._creds = creds
        self._stop  = threading.Event()
        self._proc: subprocess.Popen | None = None
        self.out_dir: Path | None = None

    def stop(self) -> None:
        self._stop.set()
        if self._proc and self._proc.poll() is None:
            self._proc.terminate()

    # ── Émission d'événements ─────────────────────────────────────────────────
    def _log(self, text: str) -> None:
        self._cb({"type": "log", "text": text})

    def _progress(self, current: int, total: int) -> None:
        self._cb({"type": "progress", "current": current, "total": total})

    def _thumb(self, filename: str) -> None:
        self._cb({"type": "thumb", "filename": filename})

    def _fail(self, message: str) -> None:
        self._log(f"[ERREUR] {message}")
        self._cb({"type": "done", "success": False, "message": message})

    # ── Commande gallery-dl ───────────────────────────────────────────────────
    def _build_cmd(self, username: str, out_dir: Path) -> list[str]:
        cmd = [
            sys.executable, "-m", "gallery_dl",
            f"https://x.com/{username}/likes",
            "-D", str(out_dir),
            "--range", f"1-{self.count}",
            "-o", f"filename={_FILENAME_FMT}",
        ]
        cookies_file = (self._creds.get("cookies_file") or "").strip()
        if cookies_file:
            cmd += ["--cookies", cookies_file]
        else:
            cmd += ["--cookies-from-browser",
                    self._creds.get("browser") or "firefox"]
        return cmd

    # ── Thread principal ──────────────────────────────────────────────────────
    def run(self) -> None:
        username = (self._creds.get("username") or "").strip().lstrip("@")
        if not username:
            self._fail("Compte X non configuré — renseigne ton pseudo "
                       "sur la page Comptes.")
            return

        try:
            import gallery_dl  # noqa: F401 — vérifie seulement la présence
        except ImportError:
            self._fail("gallery-dl n'est pas installé : pip install gallery-dl")
            return

        out_dir = Path(self.dest) / f"x_likes_{username}"
        out_dir.mkdir(parents=True, exist_ok=True)
        self.out_dir = out_dir
        self._log(f"Dossier : {out_dir}")
        self._log(f"Likes de @{username} — jusqu'à {self.count} fichiers…")

        env = {**os.environ, "PYTHONIOENCODING": "utf-8"}
        try:
            self._proc = subprocess.Popen(
                self._build_cmd(username, out_dir),
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                text=True, encoding="utf-8", errors="replace",
                env=env,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
        except OSError as exc:
            self._fail(f"Impossible de lancer gallery-dl : {exc}")
            return

        success = skipped = 0
        for line in self._proc.stdout:
            line = line.strip()
            if not line:
                continue
            if line.startswith("# "):          # fichier déjà présent
                skipped += 1
                continue
            fp = Path(line)
            if fp.is_file():                   # gallery-dl affiche chaque fichier sauvé
                success += 1
                self._progress(success, self.count)
                self._thumb(fp.name)
                self._log(f"  ✓ {fp.name}")
            else:                              # message d'info / d'erreur gallery-dl
                self._log(f"  {line[:160]}")

        rc = self._proc.wait()

        if self._stop.is_set():
            self._log("Arrêté par l'utilisateur.")

        if success == 0 and not self._stop.is_set():
            hint = ("Aucun fichier téléchargé. Vérifie que tu es connecté à X "
                    "dans le navigateur choisi (ou fournis un cookies.txt) — "
                    "les likes ne sont visibles que par ton propre compte.")
            self._fail(hint if rc != 0 else hint + f" (code {rc})")
            return

        msg = f"{success} fichier(s) téléchargé(s), {skipped} déjà présent(s)."
        self._log(f"\n✅ {msg}")
        self._cb({"type": "done", "success": True, "message": msg})
