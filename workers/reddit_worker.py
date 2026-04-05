"""
RedditWorker — extraction de médias Reddit
- URL de post   → images/vidéos de ce post (galerie incluse)
- URL subreddit → parcourt les posts et collecte les médias
Vidéos v.redd.it fusionnées audio+vidéo via yt-dlp.
"""

import re
import threading
import hashlib
import time
from pathlib import Path
from io import BytesIO

import requests
import yt_dlp
from PIL import Image


HEADERS = {
    "User-Agent": "Mozilla/5.0 (compatible; MediaDownloader/1.0)",
    "Accept":     "application/json",
}
IMG_EXTS  = {".jpg", ".jpeg", ".png", ".gif", ".webp"}
DELAY     = 0.6   # secondes entre requêtes Reddit


class RedditWorker(threading.Thread):

    def __init__(self, job_id: str, url: str, count: int,
                 dest: str, event_cb):
        super().__init__(daemon=True)
        self.job_id   = job_id
        self.url      = url.strip().rstrip("/")
        self.count    = count
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

    # --------------------------------------------------------- URL detection
    @staticmethod
    def _is_post(url: str) -> bool:
        return "/comments/" in url

    @staticmethod
    def _clean_url(url: str) -> str:
        """Supprime les paramètres de tracking et normalise l'URL."""
        return re.sub(r"\?.*$", "", url).replace("www.reddit.com", "reddit.com")

    # --------------------------------------------------------- JSON fetching
    def _get_json(self, url: str, params: dict | None = None):
        api_url = url if url.endswith(".json") else url + ".json"
        resp = requests.get(api_url, headers=HEADERS,
                            params=params, timeout=10)
        resp.raise_for_status()
        return resp.json()

    # ----------------------------------------------- media extraction
    def _media_from_post(self, post: dict) -> list[dict]:
        """Extrait la liste de médias d'un post Reddit."""
        items: list[dict] = []
        title = post.get("title", "media")[:60]
        url   = post.get("url", "")

        # Vidéo Reddit native
        if post.get("is_video") and post.get("media"):
            permalink = "https://reddit.com" + post.get("permalink", "")
            items.append({"type": "video", "url": permalink, "title": title})
            return items

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

        # Lien externe vidéo (YouTube, etc. embarqué dans le post)
        if post.get("media") and post.get("domain") not in ("self", "reddit.com"):
            if any(d in post.get("domain", "") for d in
                   ("youtube", "youtu.be", "vimeo", "twitch", "v.redd.it")):
                items.append({"type": "video", "url": url, "title": title})
                return items

        # Image directe
        if any(url.lower().endswith(ext) for ext in IMG_EXTS):
            items.append({"type": "image", "url": url, "title": title})
            return items

        # i.redd.it ou preview
        if "i.redd.it" in url or "i.imgur.com" in url:
            items.append({"type": "image", "url": url, "title": title})
            return items

        # Imgur non-direct
        if "imgur.com" in url and not url.endswith((".gifv",)):
            direct = re.sub(r"imgur\.com/", "i.imgur.com/", url) + ".jpg"
            items.append({"type": "image", "url": direct, "title": title})
            return items

        return items

    # ---------------------------------------------------- collect from post
    def _collect_post(self, url: str) -> list[dict]:
        self._log(f"Post Reddit : {url}")
        data = self._get_json(url)
        post = data[0]["data"]["children"][0]["data"]
        return self._media_from_post(post)

    # ----------------------------------------------- collect from subreddit
    def _collect_subreddit(self, url: str) -> list[dict]:
        m = re.search(r"reddit\.com/r/([^/?#]+)", url)
        if not m:
            raise ValueError("URL de subreddit invalide")
        sub = m.group(1)
        self._log(f"Subreddit : r/{sub} — collecte jusqu'à {self.count} médias…")

        media: list[dict] = []
        after: str | None = None

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
                items = self._media_from_post(p["data"])
                media.extend(items)
                if len(media) >= self.count:
                    break

            after = data["data"].get("after")
            if not after:
                break
            time.sleep(DELAY)

        self._log(f"{len(media)} médias trouvés")
        return media[:self.count]

    # ----------------------------------------------- download helpers
    def _download_image(self, url: str, filepath: Path,
                        seen: set[str]) -> bool:
        resp = requests.get(url, headers=HEADERS, timeout=15)
        resp.raise_for_status()
        raw = resp.content
        h = hashlib.md5(raw, usedforsecurity=False).hexdigest()
        if h in seen:
            return False
        seen.add(h)
        img = Image.open(BytesIO(raw))
        img.verify()
        img = Image.open(BytesIO(raw))
        if img.width < 100 or img.height < 100:
            return False
        if filepath.suffix.lower() in (".jpg", ".jpeg") and img.mode in ("RGBA", "P"):
            img = img.convert("RGB")
        kw = {"quality": 95, "subsampling": 0} if filepath.suffix.lower() in (".jpg", ".jpeg") else {}
        img.save(filepath, **kw)
        return True

    def _download_video(self, url: str, out_dir: Path, title: str) -> str | None:
        saved: list[str] = []

        def hook(d, _s=saved):
            if d["status"] == "finished":
                _s.append(Path(d["filename"]).name)

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

    # ------------------------------------------------------------------ run
    def run(self):
        clean  = self._clean_url(self.url)
        sub_name = re.search(r"/r/([^/?#]+)", clean)
        folder = sub_name.group(1) if sub_name else "reddit"
        out_dir = Path(self.dest) / folder
        out_dir.mkdir(parents=True, exist_ok=True)
        self.out_dir = out_dir

        try:
            if self._is_post(clean):
                items = self._collect_post(clean)
            else:
                items = self._collect_subreddit(clean)
        except Exception as e:
            self._log(f"[ERREUR] {e}")
            self.event_cb({"type": "done", "success": False, "message": str(e)})
            return

        if not items:
            self.event_cb({"type": "done", "success": False,
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
                    ext      = "." + item["url"].split("?")[0].rsplit(".", 1)[-1].lower()
                    if ext not in IMG_EXTS:
                        ext = ".jpg"
                    safe  = re.sub(r'[\\/:*?"<>|]', "_", item.get("title", str(idx)))[:40]
                    fp    = out_dir / f"{safe}_{idx:04d}{ext}"
                    if self._download_image(item["url"], fp, seen_hashes):
                        success += 1
                        self._progress(success, total)
                        self._thumb(fp.name)
                        self._log(f"  ✓ {fp.name}")
                    else:
                        self._log("  ⟳ Doublon/trop petit, ignoré")

                elif item["type"] == "video":
                    fname = self._download_video(item["url"], out_dir,
                                                 item.get("title", str(idx)))
                    if fname:
                        success += 1
                        self._progress(success, total)
                        self._thumb(fname)
                        self._log(f"  ✓ {fname}")
                    else:
                        failed += 1

            except Exception as e:
                self._log(f"  ✗ {e}")
                failed += 1

        msg = f"{success} média(s) téléchargé(s), {failed} échec(s)."
        self._log(f"\n✅ {msg}")
        self.event_cb({"type": "done", "success": True, "message": msg})
