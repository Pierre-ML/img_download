"""
DownloadWorker — thread de scraping/téléchargement
Callbacks via queue (SSE-friendly, pas de dépendance tkinter)
"""

import threading
import os
import re
import time
import random
import hashlib
import requests
from pathlib import Path
from io import BytesIO
from PIL import Image
import undetected_chromedriver as uc
from selenium.webdriver.common.by import By
from selenium.webdriver.support.ui import WebDriverWait


SUPPORTED_MIME = {
    "image/jpeg": ".jpg",
    "image/png":  ".png",
    "image/webp": ".webp",
    "image/gif":  ".jpg",
}
DEFAULT_EXT  = ".jpg"
DELAY_MIN    = 1.0
DELAY_MAX    = 3.0


class DownloadWorker(threading.Thread):
    def __init__(self, job_id: str, keyword: str, count: int, dest: str,
                 event_cb):  # event_cb(dict) → mis sur la queue par app.py
        super().__init__(daemon=True)
        self.job_id   = job_id
        self.keyword  = keyword
        self.count    = count
        self.dest     = dest
        self._cb      = event_cb
        self._stop    = threading.Event()
        self.out_dir: Path | None = None

    def stop(self):
        self._stop.set()

    # -----------------------------------------------------------------------
    def _emit(self, type_: str, **kwargs):
        self._cb({"type": type_, **kwargs})

    def _log(self, text: str):
        self._emit("log", text=text)

    def _progress(self, current: int, total: int):
        self._emit("progress", current=current, total=total)

    def _thumb(self, filename: str):
        self._emit("thumb", filename=filename)

    # -----------------------------------------------------------------------
    @staticmethod
    def _sanitize(name: str) -> str:
        return re.sub(r'[\\/*?:"<>|]', "_", name)

    def _ext_from(self, url: str, content_type: str | None) -> str:
        if content_type:
            for mime, ext in SUPPORTED_MIME.items():
                if mime in content_type:
                    return ext
        for ext in (".jpg", ".jpeg", ".png", ".webp"):
            if url.lower().split("?")[0].endswith(ext):
                return ".jpg" if ext == ".jpeg" else ext
        return DEFAULT_EXT

    # -----------------------------------------------------------------------
    def _collect_urls(self, driver) -> list[str]:
        urls: list[str] = []
        seen: set[str]  = set()

        search_url = (
            f"https://www.google.com/search"
            f"?q={requests.utils.quote(self.keyword)}&tbm=isch&safe=off"
        )
        self._log(f"Recherche Google Images : {self.keyword!r}")
        driver.get(search_url)
        time.sleep(2)

        # Accepter les cookies
        try:
            btn = driver.find_element(By.XPATH,
                '//button[contains(., "Tout accepter") or contains(., "Accept all")]')
            btn.click()
            time.sleep(1.5)
        except Exception:
            pass

        # Revenir en haut — Google peut scroller après le cookie banner
        driver.execute_script("window.scrollTo(0, 0);")

        # Attendre que les premières images aient un vrai src http (lazy-load déclenché)
        try:
            WebDriverWait(driver, 6).until(
                lambda d: any(
                    (img.get_attribute("src") or "").startswith("http")
                    for img in d.find_elements(By.CSS_SELECTOR, "img.YQ4gaf")
                )
            )
        except Exception:
            pass

        last_height    = 0
        scroll_attempts = 0

        while len(urls) < self.count and scroll_attempts < 60:
            if self._stop.is_set():
                break
            for img in driver.find_elements(By.CSS_SELECTOR, "img.YQ4gaf"):
                src = img.get_attribute("src") or ""
                if src.startswith("http") and src not in seen:
                    seen.add(src)
                    urls.append(src)
            if len(urls) >= self.count:
                break
            driver.execute_script("window.scrollBy(0, 1200);")
            time.sleep(0.8)
            new_height = driver.execute_script("return document.body.scrollHeight")
            if new_height == last_height:
                scroll_attempts += 5
            last_height     = new_height
            scroll_attempts += 1

        # Revenir en haut : les premières miniatures rechargent leur src en étant en vue
        driver.execute_script("window.scrollTo(0, 0);")
        time.sleep(2.5)

        # Tentative haute résolution — itération en ordre DOM (préserve l'ordre des résultats)
        hires: list[str] = []
        replaced_srcs: set[str] = set()
        thumbs = driver.find_elements(By.CSS_SELECTOR, "img.YQ4gaf")
        self._log(f"{len(thumbs)} miniatures — tentative haute résolution…")

        PANEL_CSS = ('img[jsname="kn3ccd"],img[class*="iPVvYb"],'
                     'a[jsname="sTFXNd"] img,div[jsname="figiqf"] img')

        for thumb in thumbs[:self.count * 2]:
            if self._stop.is_set() or len(hires) >= self.count:
                break
            thumb_src = thumb.get_attribute("src") or ""
            try:
                driver.execute_script("arguments[0].click();", thumb)

                # Attendre qu'une image source (non-tbn) soit chargée dans le panel
                best_src = None
                for _ in range(6):               # jusqu'à ~3s
                    time.sleep(0.5)
                    for panel in driver.find_elements(By.CSS_SELECTOR, PANEL_CSS):
                        src = panel.get_attribute("src") or ""
                        if src.startswith("http") and "encrypted-tbn" not in src:
                            best_src = src
                            break
                    if best_src:
                        break

                # Fallback : accepter un tbn si rien de mieux n'est apparu
                if not best_src:
                    for panel in driver.find_elements(By.CSS_SELECTOR, PANEL_CSS):
                        src = panel.get_attribute("src") or ""
                        if src.startswith("http"):
                            best_src = src
                            break

                if best_src and best_src not in seen:
                    seen.add(best_src)
                    hires.append(best_src)
                    replaced_srcs.add(thumb_src)
            except Exception:
                pass

        # Exclure les thumbnails déjà couverts par une version hires
        remaining = [u for u in urls if u not in replaced_srcs]
        final = (hires + remaining)[:self.count]
        self._log(f"{len(final)} URL(s) collectée(s)")
        return final

    def _download_one(self, url: str, filepath: Path,
                      session: requests.Session,
                      seen_hashes: set[str]) -> bool:
        headers = {
            "User-Agent": (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
                "AppleWebKit/537.36 (KHTML, like Gecko) "
                "Chrome/124.0.0.0 Safari/537.36"
            ),
            "Referer": "https://www.google.com/",
        }
        resp = session.get(url, headers=headers, timeout=15)
        resp.raise_for_status()

        ct = resp.headers.get("Content-Type", "")
        if not any(m in ct for m in ("image/", "application/octet")):
            return False

        raw = resp.content

        # Déduplication par hash du contenu
        h = hashlib.md5(raw, usedforsecurity=False).hexdigest()
        if h in seen_hashes:
            self._log("  ⟳ Doublon détecté, ignoré")
            return False
        seen_hashes.add(h)

        img = Image.open(BytesIO(raw))
        img.verify()
        img = Image.open(BytesIO(raw))

        # Rejeter les images trop petites (thumbnails qui ont passé le filtre URL)
        if img.width < 300 or img.height < 300:
            self._log(f"  ⟳ Trop petit ({img.width}×{img.height}), ignoré")
            return False

        if filepath.suffix.lower() in (".jpg", ".jpeg") and img.mode in ("RGBA", "P"):
            img = img.convert("RGB")

        save_kwargs = {"quality": 95, "subsampling": 0} if filepath.suffix.lower() in (".jpg", ".jpeg") else {}
        img.save(filepath, **save_kwargs)
        return True

    # -----------------------------------------------------------------------
    def run(self):
        safe_kw  = self._sanitize(self.keyword)
        out_dir  = Path(self.dest) / safe_kw
        out_dir.mkdir(parents=True, exist_ok=True)
        self.out_dir = out_dir
        self._log(f"Dossier : {out_dir}")

        # Chrome
        self._log("Démarrage de Chrome…")
        opts = uc.ChromeOptions()
        opts.add_argument("--window-size=1280,900")
        opts.add_argument("--lang=fr-FR")
        driver = None

        try:
            driver = uc.Chrome(options=opts, use_subprocess=True, version_main=146)
            urls   = self._collect_urls(driver)
        except Exception as exc:
            self._log(f"[ERREUR] {exc}")
            self._emit("done", success=False, message=str(exc))
            return
        finally:
            if driver:
                try:
                    driver.quit()
                except Exception:
                    pass

        if not urls:
            self._emit("done", success=False, message="Aucune URL collectée.")
            return

        session      = requests.Session()
        seen_hashes: set[str] = set()
        success = failed = 0

        for idx, url in enumerate(urls, 1):
            if self._stop.is_set():
                self._log("Arrêté par l'utilisateur.")
                break

            self._log(f"[{idx}/{len(urls)}] {url[:80]}…")
            try:
                head = session.head(url, timeout=8, allow_redirects=True,
                                    headers={"User-Agent": "Mozilla/5.0"})
                ext  = self._ext_from(url, head.headers.get("Content-Type"))
                fp   = out_dir / f"{safe_kw}_{idx:03d}{ext}"

                if self._download_one(url, fp, session, seen_hashes):
                    success += 1
                    self._progress(success, self.count)
                    self._thumb(fp.name)
                    self._log(f"  ✓ {fp.name}")
                else:
                    failed += 1
                    self._log("  ✗ Non-image, ignoré")
            except Exception as exc:
                failed += 1
                self._log(f"  ✗ {exc}")

            if success < self.count and idx < len(urls):
                delay = random.uniform(DELAY_MIN, DELAY_MAX)
                self._log(f"  … {delay:.1f}s")
                time.sleep(delay)

        msg = f"{success} image(s) téléchargée(s), {failed} échec(s)."
        self._log(f"\n✅ {msg}")
        self._emit("done", success=True, message=msg)
