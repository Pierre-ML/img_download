"""
RedditWorker — extraction de médias Reddit.
Supporte les posts individuels et les subreddits entiers.
Vidéos v.redd.it fusionnées audio+vidéo via yt-dlp.
"""

import hashlib
import re
import threading
import time
from pathlib import Path
from typing import Callable

import requests
import yt_dlp

import config
from .image_utils import image_dimensions, is_valid_image

# ── Constantes ────────────────────────────────────────────────────────────────
_HEADERS: dict[str, str] = {
    "User-Agent": "Mozilla/5.0 (compatible; MediaDownloader/1.0)",
    "Accept":     "application/json",
}
_IMG_EXTS  = {".jpg", ".jpeg", ".png", ".gif", ".webp"}
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
        resp    = requests.get(api_url, headers=_HEADERS,
                               params=params,
                               timeout=config.REDDIT_REQUEST_TIMEOUT)
        resp.raise_for_status()
        return resp.json()

    # ── Extraction des médias d'un post ───────────────────────────────────────
    def _media_from_post(self, post: dict) -> list[dict]:
        """Retourne la liste de médias (image ou vidéo) d'un post Reddit."""
        items: list[dict] = []
        title = post.get("title", "media")[:60]
        url   = post.get("url", "")

        # Vidéo Reddit native
        if post.get("is_video") and post.get("media"):
            permalink = "https://reddit.com" + post.get("permalink", "")
            return [{"type": "video", "url": permalink, "title": title}]

        # Galerie Reddit
        if post.get("is_gallery") and post.get("media_metadata"):
            for key, val in post["media_metadata"].items():
                if val.get("status") != "valid":
                    continue
                s = val.get("s", {})
                img_url = (s.get("u") or s.get("gif") or "").replace("&amp;", "&")
                if img_url:
                    items.append({"type": "image", "url": img_url, "title": key})
            return items

        # Vidéo externe (YouTube, Vimeo…)
        if post.get("media") and any(d in post.get("domain", "")
                                     for d in _VIDEO_DOMAINS):
            return [{"type": "video", "url": url, "title": title}]

        # Image directe
        if (any(url.lower().endswith(ext) for ext in _IMG_EXTS)
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
    def _download_image(self, url: str, filepath: Path,
                        seen: set[str]) -> bool:
        """Télécharge et valide une image. Retourne True si sauvegardée."""
        resp = requests.get(url, headers=_HEADERS,
                            timeout=config.REDDIT_REQUEST_TIMEOUT)
        resp.raise_for_status()
        raw = resp.content

        h = hashlib.md5(raw, usedforsecurity=False).hexdigest()
        if h in seen:
            return False
        seen.add(h)

        if not is_valid_image(raw):
            return False

        w, h = image_dimensions(raw)
        if w is None or h is None:
            return False
        if w < config.REDDIT_MIN_SIZE or h < config.REDDIT_MIN_SIZE:
            return False

        filepath.write_bytes(raw)
        return True

    # ── Téléchargement vidéo ──────────────────────────────────────────────────
    def _download_video(self, url: str, out_dir: Path,
                        title: str) -> str | None:
        """Télécharge une vidéo via yt-dlp. Retourne le nom du fichier ou None."""
        saved: list[str] = []

        def hook(d: dict) -> None:
            if d["status"] == "finished":
                saved.append(Path(d["filename"]).name)

        safe_title = re.sub(r'[\\/:*?"<>|]', "_", title)[:60] or "video"
        ydl_opts = {
            "outtmpl":             str(out_dir / f"{safe_title}.%(ext)s"),
            "format":              "bestvideo+bestaudio/best",
            "merge_output_format": "mp4",
            "noplaylist":          True,
            "quiet":               True,
            "no_warnings":         True,
            "progress_hooks":      [hook],
        }
        with yt_dlp.YoutubeDL(ydl_opts) as ydl:
            ydl.download([url])
        return saved[0] if saved else None

    # ── Thread principal ──────────────────────────────────────────────────────
    def run(self) -> None:
        clean    = self._clean_url(self.url)
        sub_name = re.search(r"/r/([^/?#]+)", clean)
        folder   = sub_name.group(1) if sub_name else "reddit"
        out_dir  = Path(self.dest) / folder
        out_dir.mkdir(parents=True, exist_ok=True)
        self.out_dir = out_dir

        try:
            items = (self._collect_post(clean)
                     if self._is_post(clean)
                     else self._collect_subreddit(clean))
        except Exception as exc:
            self._log(f"[ERREUR] {exc}")
            self._cb({"type": "done", "success": False, "message": str(exc)})
            return

        if not items:
            self._cb({"type": "done", "success": False,
                      "message": "Aucun média trouvé."})
            return

        total   = len(items)
        success = failed = 0
        seen_hashes: set[str] = set()
        self._log(f"{total} média(s) à télécharger")

        for idx, item in enumerate(items, 1):
            if self._stop.is_set():
                self._log("Arrêté par l'utilisateur.")
                break

            self._log(f"[{idx}/{total}] {item['type']} — {item['url'][:80]}…")

            try:
                if item["type"] == "image":
                    ext  = "." + item["url"].split("?")[0].rsplit(".", 1)[-1].lower()
                    ext  = ext if ext in _IMG_EXTS else ".jpg"
                    safe = re.sub(r'[\\/:*?"<>|]', "_", item.get("title", str(idx)))[:40]
                    fp   = out_dir / f"{safe}_{idx:04d}{ext}"

                    if self._download_image(item["url"], fp, seen_hashes):
                        success += 1
                        self._progress(success, total)
                        self._thumb(fp.name)
                        self._log(f"  ✓ {fp.name}")
                    else:
                        self._log("  ⟳ Doublon/trop petit, ignoré")

                elif item["type"] == "video":
                    fname = self._download_video(
                        item["url"], out_dir, item.get("title", str(idx))
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

        msg = f"{success} média(s) téléchargé(s), {failed} échec(s)."
        self._log(f"\n✅ {msg}")
        self._cb({"type": "done", "success": True, "message": msg})
