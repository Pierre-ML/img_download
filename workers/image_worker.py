"""
DownloadWorker — scraping Google Images + téléchargement.
Communique via callbacks SSE (thread-safe).
"""

import hashlib
import random
import re
import threading
import time
from pathlib import Path
from typing import Callable

import requests
import undetected_chromedriver as uc
from selenium.webdriver.common.by import By
from selenium.webdriver.support.ui import WebDriverWait

import config
from .image_utils import image_dimensions, is_valid_image

# ── Constantes ────────────────────────────────────────────────────────────────
_MIME_TO_EXT: dict[str, str] = {
    "image/jpeg": ".jpg",
    "image/png":  ".png",
    "image/webp": ".webp",
    "image/gif":  ".gif",
}
_DEFAULT_EXT  = ".jpg"
_PANEL_CSS    = (
    'img[jsname="kn3ccd"],img[class*="iPVvYb"],'
    'a[jsname="sTFXNd"] img,div[jsname="figiqf"] img'
)
_GOOGLE_IMG_CSS = "img.YQ4gaf"


class DownloadWorker(threading.Thread):
    """Thread de scraping Google Images et de téléchargement."""

    def __init__(self, job_id: str, keyword: str, count: int,
                 dest: str, event_cb: Callable[[dict], None]) -> None:
        super().__init__(daemon=True)
        self.job_id  = job_id
        self.keyword = keyword
        self.count   = count
        self.dest    = dest
        self._cb     = event_cb
        self._stop   = threading.Event()
        self.out_dir: Path | None = None

    def stop(self) -> None:
        self._stop.set()

    # ── Émission d'événements ─────────────────────────────────────────────────
    def _emit(self, type_: str, **kwargs) -> None:
        self._cb({"type": type_, **kwargs})

    def _log(self, text: str) -> None:
        self._emit("log", text=text)

    def _progress(self, current: int, total: int) -> None:
        self._emit("progress", current=current, total=total)

    def _thumb(self, filename: str) -> None:
        self._emit("thumb", filename=filename)

    # ── Utilitaires ───────────────────────────────────────────────────────────
    @staticmethod
    def _sanitize(name: str) -> str:
        """Supprime les caractères invalides pour un nom de dossier."""
        return re.sub(r'[\\/*?:"<>|]', "_", name)

    def _ext_from(self, url: str, content_type: str | None) -> str:
        """Détermine l'extension à partir du Content-Type ou de l'URL."""
        if content_type:
            for mime, ext in _MIME_TO_EXT.items():
                if mime in content_type:
                    return ext
        for ext in (".jpg", ".jpeg", ".png", ".webp"):
            if url.lower().split("?")[0].endswith(ext):
                return ".jpg" if ext == ".jpeg" else ext
        return _DEFAULT_EXT

    # ── Collecte des URLs ─────────────────────────────────────────────────────
    def _collect_urls(self, driver) -> list[str]:
        """Scrape Google Images et retourne les URLs haute résolution."""
        urls: list[str] = []
        seen: set[str]  = set()

        search_url = (
            f"https://www.google.com/search"
            f"?q={requests.utils.quote(self.keyword)}&tbm=isch&safe=off"
        )
        self._log(f"Recherche Google Images : {self.keyword!r}")
        driver.get(search_url)
        time.sleep(2)

        # Accepter le bandeau de cookies si présent
        try:
            btn = driver.find_element(
                By.XPATH,
                '//button[contains(., "Tout accepter") or contains(., "Accept all")]',
            )
            btn.click()
            time.sleep(1.5)
        except Exception:
            pass

        driver.execute_script("window.scrollTo(0, 0);")

        # Attendre que les images aient un vrai src HTTP
        try:
            WebDriverWait(driver, 6).until(
                lambda d: any(
                    (img.get_attribute("src") or "").startswith("http")
                    for img in d.find_elements(By.CSS_SELECTOR, _GOOGLE_IMG_CSS)
                )
            )
        except Exception:
            pass

        last_height    = 0
        scroll_attempts = 0

        while len(urls) < self.count and scroll_attempts < config.IMG_SCROLL_MAX:
            if self._stop.is_set():
                break
            for img in driver.find_elements(By.CSS_SELECTOR, _GOOGLE_IMG_CSS):
                src = img.get_attribute("src") or ""
                if src.startswith("http") and src not in seen:
                    seen.add(src)
                    urls.append(src)
            if len(urls) >= self.count:
                break
            driver.execute_script("window.scrollBy(0, 1200);")
            time.sleep(0.8)
            new_height = driver.execute_script("return document.body.scrollHeight")
            scroll_attempts += 5 if new_height == last_height else 1
            last_height = new_height

        # Retour en haut pour déclencher le rechargement des miniatures
        driver.execute_script("window.scrollTo(0, 0);")
        time.sleep(2.5)

        # Tentative de récupération haute résolution via le panel latéral
        hires: list[str]        = []
        replaced: set[str]      = set()
        thumbs = driver.find_elements(By.CSS_SELECTOR, _GOOGLE_IMG_CSS)
        self._log(f"{len(thumbs)} miniatures — tentative haute résolution…")

        for thumb in thumbs[: self.count * 2]:
            if self._stop.is_set() or len(hires) >= self.count:
                break
            thumb_src = thumb.get_attribute("src") or ""
            try:
                driver.execute_script("arguments[0].click();", thumb)
                best_src = None
                for _ in range(6):
                    time.sleep(0.5)
                    for panel in driver.find_elements(By.CSS_SELECTOR, _PANEL_CSS):
                        src = panel.get_attribute("src") or ""
                        if src.startswith("http") and "encrypted-tbn" not in src:
                            best_src = src
                            break
                    if best_src:
                        break
                # Fallback : accepter un tbn si rien de mieux
                if not best_src:
                    for panel in driver.find_elements(By.CSS_SELECTOR, _PANEL_CSS):
                        src = panel.get_attribute("src") or ""
                        if src.startswith("http"):
                            best_src = src
                            break
                if best_src and best_src not in seen:
                    seen.add(best_src)
                    hires.append(best_src)
                    replaced.add(thumb_src)
            except Exception:
                pass

        remaining = [u for u in urls if u not in replaced]
        final     = (hires + remaining)[: self.count]
        self._log(f"{len(final)} URL(s) collectée(s)")
        return final

    # ── Téléchargement ────────────────────────────────────────────────────────
    def _download_one(self, url: str, filepath: Path,
                      session: requests.Session,
                      seen_hashes: set[str]) -> bool:
        """Télécharge, valide et sauvegarde une image. Retourne True si réussi."""
        headers = {
            "User-Agent": (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
                "AppleWebKit/537.36 (KHTML, like Gecko) "
                "Chrome/124.0.0.0 Safari/537.36"
            ),
            "Referer": "https://www.google.com/",
        }
        resp = session.get(url, headers=headers,
                           timeout=config.IMG_REQUEST_TIMEOUT)
        resp.raise_for_status()

        ct = resp.headers.get("Content-Type", "")
        if not any(m in ct for m in ("image/", "application/octet")):
            return False

        raw = resp.content

        # Déduplication par hash MD5
        h = hashlib.md5(raw, usedforsecurity=False).hexdigest()
        if h in seen_hashes:
            self._log("  ⟳ Doublon, ignoré")
            return False
        seen_hashes.add(h)

        if not is_valid_image(raw):
            self._log("  ✗ Format non reconnu, ignoré")
            return False

        w, h = image_dimensions(raw)
        if w is None or h is None:
            self._log("  ✗ Dimensions illisibles, ignoré")
            return False

        if w < config.IMG_MIN_SIZE or h < config.IMG_MIN_SIZE:
            self._log(f"  ⟳ Trop petit ({w}×{h}), ignoré")
            return False

        filepath.write_bytes(raw)
        return True

    # ── Thread principal ──────────────────────────────────────────────────────
    def run(self) -> None:
        safe_kw = self._sanitize(self.keyword)
        out_dir = Path(self.dest) / safe_kw
        out_dir.mkdir(parents=True, exist_ok=True)
        self.out_dir = out_dir
        self._log(f"Dossier : {out_dir}")

        self._log("Démarrage de Chrome…")
        opts = uc.ChromeOptions()
        opts.add_argument("--window-size=1280,900")
        opts.add_argument("--lang=fr-FR")
        driver = None

        try:
            driver = uc.Chrome(
                options=opts,
                use_subprocess=True,
                version_main=config.CHROME_VERSION,
            )
            urls = self._collect_urls(driver)
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
                ext = self._ext_from(url, head.headers.get("Content-Type"))
                fp  = out_dir / f"{safe_kw}_{idx:03d}{ext}"

                if self._download_one(url, fp, session, seen_hashes):
                    success += 1
                    self._progress(success, self.count)
                    self._thumb(fp.name)
                    self._log(f"  ✓ {fp.name}")
                else:
                    failed += 1
            except Exception as exc:
                failed += 1
                self._log(f"  ✗ {exc}")

            if success < self.count and idx < len(urls):
                delay = random.uniform(config.IMG_DELAY_MIN, config.IMG_DELAY_MAX)
                self._log(f"  … {delay:.1f}s")
                time.sleep(delay)

        msg = f"{success} image(s) téléchargée(s), {failed} échec(s)."
        self._log(f"\n✅ {msg}")
        self._emit("done", success=True, message=msg)
