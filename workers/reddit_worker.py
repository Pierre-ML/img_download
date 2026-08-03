"""
RedditWorker — extraction de médias Reddit.
Supporte les posts individuels, les subreddits entiers et les posts
enregistrés du compte (RedditSavedWorker, OAuth « script app »).
Vidéos v.redd.it fusionnées audio+vidéo via yt-dlp.
Fichiers nommés d'après le titre du post ; images téléchargées
en parallèle (config.REDDIT_PARALLEL), vidéos en série.
"""

import hashlib
import re
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Callable

import requests
import yt_dlp

import config
from .image_utils import (detect_ext, image_dimensions,
                          sanitize_filename, unique_path)

# ── Constantes ────────────────────────────────────────────────────────────────
_HEADERS: dict[str, str] = {
    "User-Agent": "Mozilla/5.0 (compatible; MediaDownloader/1.0)",
    "Accept":     "application/json",
}
_VIDEO_DOMAINS = ("youtube", "youtu.be", "vimeo", "twitch", "v.redd.it")


class RedditWorker(threading.Thread):
    """Thread d'extraction de médias depuis un post ou un subreddit."""

    def __init__(self, job_id: str, url: str, count: int,
                 dest: str, event_cb: Callable[[dict], None]) -> None:
        super().__init__(daemon=True)
        self.job_id   = job_id
        self.url      = url.strip().rstrip("/")
        self.count    = count
        self.dest     = dest
        self._cb      = event_cb
        self._stop    = threading.Event()
        self.out_dir: Path | None = None
        self._session = requests.Session()
        # État partagé entre les threads de téléchargement
        self._dl_lock = threading.Lock()
        self._seen_hashes: set[str] = set()

    def stop(self) -> None:
        self._stop.set()

    # ── Émission d'événements ─────────────────────────────────────────────────
    def _log(self, text: str) -> None:
        self._cb({"type": "log", "text": text})

    def _progress(self, current: int, total: int) -> None:
        self._cb({"type": "progress", "current": current, "total": total})

    def _thumb(self, filename: str) -> None:
        self._cb({"type": "thumb", "filename": filename})

    # ── Détection du type d'URL ───────────────────────────────────────────────
    @staticmethod
    def _is_post(url: str) -> bool:
        return "/comments/" in url

    @staticmethod
    def _clean_url(url: str) -> str:
        return re.sub(r"\?.*$", "", url).replace("www.reddit.com", "reddit.com")

    # ── Requêtes JSON ─────────────────────────────────────────────────────────
    def _get_json(self, url: str, params: dict | None = None) -> dict | list:
        api_url = url if url.endswith(".json") else url + ".json"
        # raw_json=1 : Reddit renvoie les URLs sans échappement HTML (&amp;)
        resp = self._session.get(api_url, headers=_HEADERS,
                                 params={"raw_json": 1, **(params or {})},
                                 timeout=config.REDDIT_REQUEST_TIMEOUT)
        resp.raise_for_status()
        return resp.json()

    # ── Extraction des médias d'un post ───────────────────────────────────────
    def _media_from_post(self, post: dict) -> list[dict]:
        """Retourne la liste de médias (image ou vidéo) d'un post Reddit,
        chacun titré d'après le titre du post."""
        items: list[dict] = []
        title = post.get("title", "media")[:80]
        url   = post.get("url", "")

        # Vidéo Reddit native
        if post.get("is_video") and post.get("media"):
            permalink = "https://reddit.com" + post.get("permalink", "")
            return [{"type": "video", "url": permalink, "title": title}]

        # Galerie Reddit — on respecte l'ordre de gallery_data si présent
        if post.get("is_gallery") and post.get("media_metadata"):
            meta  = post["media_metadata"]
            order = [it.get("media_id")
                     for it in post.get("gallery_data", {}).get("items", [])]
            pos = 0
            for key in (order or list(meta)):
                val = meta.get(key) or {}
                if val.get("status") != "valid":
                    continue
                s = val.get("s", {})
                img_url = (s.get("u") or s.get("gif") or "").replace("&amp;", "&")
                if img_url:
                    pos += 1
                    items.append({"type": "image", "url": img_url,
                                  "title": f"{title}_{pos:02d}"})
            return items

        # Vidéo externe (YouTube, Vimeo…)
        if post.get("media") and any(d in post.get("domain", "")
                                     for d in _VIDEO_DOMAINS):
            return [{"type": "video", "url": url, "title": title}]

        # Image directe
        if (any(url.lower().split("?")[0].endswith(ext)
                for ext in (".jpg", ".jpeg", ".png", ".gif", ".webp"))
                or "i.redd.it" in url or "i.imgur.com" in url):
            return [{"type": "image", "url": url, "title": title}]

        # Imgur non-direct
        if "imgur.com" in url and not url.endswith(".gifv"):
            direct = re.sub(r"imgur\.com/", "i.imgur.com/", url) + ".jpg"
            return [{"type": "image", "url": direct, "title": title}]

        return items

    # ── Collecte depuis un post ───────────────────────────────────────────────
    def _collect_post(self, url: str) -> list[dict]:
        self._log(f"Post Reddit : {url}")
        data = self._get_json(url)
        post = data[0]["data"]["children"][0]["data"]
        return self._media_from_post(post)

    # ── Collecte depuis un subreddit ──────────────────────────────────────────
    def _collect_subreddit(self, url: str) -> list[dict]:
        m = re.search(r"reddit\.com/r/([^/?#]+)", url)
        if not m:
            raise ValueError("URL de subreddit invalide")
        sub = m.group(1)
        self._log(f"Subreddit : r/{sub} — collecte jusqu'à {self.count} médias…")

        media: list[dict]  = []
        after: str | None  = None

        while len(media) < self.count:
            if self._stop.is_set():
                break
            params: dict = {"limit": 100}
            if after:
                params["after"] = after

            data  = self._get_json(f"https://reddit.com/r/{sub}", params)
            posts = data["data"]["children"]
            if not posts:
                break

            for p in posts:
                media.extend(self._media_from_post(p["data"]))
                if len(media) >= self.count:
                    break

            after = data["data"].get("after")
            if not after:
                break
            time.sleep(config.REDDIT_DELAY)

        self._log(f"{len(media)} médias trouvés")
        return media[: self.count]

    # ── Téléchargement image ──────────────────────────────────────────────────
    def _download_image(self, item: dict, out_dir: Path) -> tuple[str, str | None]:
        """Télécharge, valide et sauvegarde une image.
        Retourne ("ok", nom_fichier) | ("skip", raison) | ("stopped", None)."""
        if self._stop.is_set():
            return ("stopped", None)

        resp = self._session.get(item["url"], headers=_HEADERS,
                                 timeout=config.REDDIT_REQUEST_TIMEOUT)
        resp.raise_for_status()
        raw = resp.content

        ext = detect_ext(raw)
        if ext is None:
            return ("skip", "format non reconnu")

        digest = hashlib.md5(raw, usedforsecurity=False).hexdigest()
        with self._dl_lock:
            if digest in self._seen_hashes:
                return ("skip", "doublon")
            self._seen_hashes.add(digest)

        w, h = image_dimensions(raw)
        if w is None or h is None:
            return ("skip", "dimensions illisibles")
        if w < config.REDDIT_MIN_SIZE or h < config.REDDIT_MIN_SIZE:
            return ("skip", f"trop petit ({w}×{h})")

        stem = sanitize_filename(item.get("title", "media"))
        # unique_path + écriture sous lock : pas de collision entre threads
        with self._dl_lock:
            fp = unique_path(out_dir, stem, ext)
            fp.write_bytes(raw)
        return ("ok", fp.name)

    # ── Téléchargement vidéo ──────────────────────────────────────────────────
    def _download_video(self, url: str, out_dir: Path,
                        title: str) -> str | None:
        """Télécharge une vidéo via yt-dlp. Retourne le nom du fichier ou None."""
        saved: list[str] = []

        def hook(d: dict) -> None:
            if d["status"] == "finished":
                saved.append(Path(d["filename"]).name)

        base = sanitize_filename(title, fallback="video")
        stem, n = base, 2
        existing = {f.stem for f in out_dir.iterdir()}
        while stem in existing:
            stem = f"{base}_{n}"
            n += 1

        ydl_opts = {
            "outtmpl":             str(out_dir / f"{stem}.%(ext)s"),
            "format":              "bestvideo+bestaudio/best",
            "merge_output_format": "mp4",
            "noplaylist":          True,
            "quiet":               True,
            "no_warnings":         True,
            "retries":             3,
            "concurrent_fragment_downloads": 4,
            "progress_hooks":      [hook],
        }
        with yt_dlp.YoutubeDL(ydl_opts) as ydl:
            ydl.download([url])
        return saved[0] if saved else None

    # ── Points d'extension (surclassés par RedditSavedWorker) ────────────────
    def _folder_name(self) -> str:
        sub_name = re.search(r"/r/([^/?#]+)", self._clean_url(self.url))
        return sub_name.group(1) if sub_name else "reddit"

    def _collect(self) -> list[dict]:
        clean = self._clean_url(self.url)
        return (self._collect_post(clean)
                if self._is_post(clean)
                else self._collect_subreddit(clean))

    # ── Thread principal ──────────────────────────────────────────────────────
    def run(self) -> None:
        out_dir = Path(self.dest) / self._folder_name()
        out_dir.mkdir(parents=True, exist_ok=True)
        self.out_dir = out_dir

        try:
            items = self._collect()
        except Exception as exc:
            self._log(f"[ERREUR] {exc}")
            self._cb({"type": "done", "success": False, "message": str(exc)})
            return

        if not items:
            self._cb({"type": "done", "success": False,
                      "message": "Aucun média trouvé."})
            return

        images = [it for it in items if it["type"] == "image"]
        videos = [it for it in items if it["type"] == "video"]
        total  = len(items)
        success = skipped = failed = 0
        self._log(f"{total} média(s) : {len(images)} image(s), "
                  f"{len(videos)} vidéo(s)")

        # Phase 1 — images en parallèle
        if images:
            with ThreadPoolExecutor(max_workers=config.REDDIT_PARALLEL) as pool:
                futures = {pool.submit(self._download_image, it, out_dir): it
                           for it in images}
                for fut in as_completed(futures):
                    try:
                        status, detail = fut.result()
                    except Exception as exc:
                        failed += 1
                        self._log(f"  ✗ {str(exc)[:100]}")
                        continue
                    if status == "ok":
                        success += 1
                        self._progress(success, total)
                        self._thumb(detail)
                        self._log(f"  ✓ {detail}")
                    elif status == "skip":
                        skipped += 1
                        self._log(f"  ⟳ {detail}, ignoré")

        # Phase 2 — vidéos en série (yt-dlp + fusion ffmpeg)
        for item in videos:
            if self._stop.is_set():
                break
            self._log(f"Vidéo : {item['url'][:80]}…")
            try:
                fname = self._download_video(
                    item["url"], out_dir, item.get("title", "video")
                )
                if fname:
                    success += 1
                    self._progress(success, total)
                    self._thumb(fname)
                    self._log(f"  ✓ {fname}")
                else:
                    failed += 1
            except Exception as exc:
                self._log(f"  ✗ {exc}")
                failed += 1

        if self._stop.is_set():
            self._log("Arrêté par l'utilisateur.")

        msg = (f"{success} média(s) téléchargé(s), "
               f"{skipped} ignoré(s), {failed} échec(s).")
        self._log(f"\n✅ {msg}")
        self._cb({"type": "done", "success": True, "message": msg})


class RedditSavedWorker(RedditWorker):
    """Télécharge les posts enregistrés du compte connecté.
    Auth : app Reddit de type « script » (reddit.com/prefs/apps) —
    grant_type=password. Avec 2FA : mot de passe = `motdepasse:code2FA`."""

    _OAUTH_UA = "windows:MediaDownloader:1.0 (script personnel)"

    def __init__(self, job_id: str, count: int, dest: str,
                 event_cb: Callable[[dict], None], creds: dict) -> None:
        super().__init__(job_id=job_id, url="https://reddit.com/saved",
                         count=count, dest=dest, event_cb=event_cb)
        self._creds = creds

    def _folder_name(self) -> str:
        user = self._creds.get("username") or "reddit"
        return f"saved_{user}"

    def _authenticate(self) -> str:
        c = self._creds
        missing = [k for k in ("client_id", "client_secret",
                               "username", "password") if not c.get(k)]
        if missing:
            raise ValueError(
                "Compte Reddit incomplet (page Comptes) — champs manquants : "
                + ", ".join(missing)
            )
        resp = self._session.post(
            "https://www.reddit.com/api/v1/access_token",
            auth=(c["client_id"], c["client_secret"]),
            data={"grant_type": "password",
                  "username": c["username"], "password": c["password"]},
            headers={"User-Agent": self._OAUTH_UA},
            timeout=config.REDDIT_REQUEST_TIMEOUT,
        )
        resp.raise_for_status()
        body = resp.json()
        token = body.get("access_token")
        if not token:
            raise ValueError(
                f"Authentification Reddit refusée : {body.get('error', '?')} "
                "(vérifie l'app script et le mot de passe ; "
                "avec 2FA : motdepasse:code2FA)"
            )
        return token

    def _collect(self) -> list[dict]:
        token   = self._authenticate()
        user    = self._creds["username"]
        headers = {"Authorization": f"bearer {token}",
                   "User-Agent": self._OAUTH_UA}
        self._log(f"Connecté à Reddit : u/{user} — "
                  f"collecte jusqu'à {self.count} posts enregistrés…")

        media: list[dict] = []
        after: str | None = None

        while len(media) < self.count:
            if self._stop.is_set():
                break
            params: dict = {"limit": 100, "raw_json": 1, "type": "links"}
            if after:
                params["after"] = after

            resp = self._session.get(
                f"https://oauth.reddit.com/user/{user}/saved",
                headers=headers, params=params,
                timeout=config.REDDIT_REQUEST_TIMEOUT,
            )
            resp.raise_for_status()
            data  = resp.json()
            posts = data["data"]["children"]
            if not posts:
                break

            for p in posts:
                if p.get("kind") != "t3":   # ignore les commentaires enregistrés
                    continue
                media.extend(self._media_from_post(p["data"]))
                if len(media) >= self.count:
                    break

            after = data["data"].get("after")
            if not after:
                break
            time.sleep(config.REDDIT_DELAY)

        self._log(f"{len(media)} médias trouvés dans les enregistrés")
        return media[: self.count]
