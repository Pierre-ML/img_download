"""
DownloadWorker — scraping Google Images + téléchargement.
Communique via callbacks SSE (thread-safe).
Téléchargements en parallèle (config.IMG_PARALLEL) ; nommage
`motclé_NNN_LARGExHAUT.ext` avec numérotation continue entre les runs.
"""

import hashlib
import re
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Callable

import requests
import undetected_chromedriver as uc
from selenium.webdriver.common.by import By
from selenium.webdriver.support.ui import WebDriverWait

import config
from .image_utils import detect_ext, image_dimensions, sanitize_filename

# ── Constantes ────────────────────────────────────────────────────────────────
_PANEL_CSS    = (
    'img[jsname="kn3ccd"],img[class*="iPVvYb"],'
    'a[jsname="sTFXNd"] img,div[jsname="figiqf"] img'
)
_GOOGLE_IMG_CSS = "img.YQ4gaf"
_HEADERS = {
    "User-Agent": (
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
        "AppleWebKit/537.36 (KHTML, like Gecko) "
        "Chrome/124.0.0.0 Safari/537.36"
    ),
    "Referer": "https://www.google.com/",
}


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
        # État partagé entre les threads de téléchargement
        self._dl_lock     = threading.Lock()
        self._seen_hashes: set[str] = set()
        self._file_num    = 0

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
    def _download_one(self, url: str, session: requests.Session,
                      out_dir: Path, safe_kw: str) -> tuple[str, str | None]:
        """Télécharge, valide et sauvegarde une image.
        Retourne ("ok", nom_fichier) | ("skip", raison) | ("stopped", None)."""
        if self._stop.is_set():
            return ("stopped", None)

        resp = session.get(url, headers=_HEADERS,
                           timeout=config.IMG_REQUEST_TIMEOUT)
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
        if w < config.IMG_MIN_SIZE or h < config.IMG_MIN_SIZE:
            return ("skip", f"trop petit ({w}×{h})")

        # Réservation du numéro + écriture sous lock : pas de collision de noms
        with self._dl_lock:
            self._file_num += 1
            fp = out_dir / f"{safe_kw}_{self._file_num:03d}_{w}x{h}{ext}"
            fp.write_bytes(raw)
        return ("ok", fp.name)

    # ── Thread principal ──────────────────────────────────────────────────────
    def run(self) -> None:
        safe_kw = sanitize_filename(self.keyword, fallback="images")
        out_dir = Path(self.dest) / safe_kw
        out_dir.mkdir(parents=True, exist_ok=True)
        self.out_dir = out_dir
        self._log(f"Dossier : {out_dir}")

        # Reprend la numérotation là où le run précédent s'était arrêté
        num_re = re.compile(re.escape(safe_kw) + r"_(\d+)")
        for f in out_dir.iterdir():
            m = num_re.match(f.stem)
            if m:
                self._file_num = max(self._file_num, int(m.group(1)))

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

        session = requests.Session()
        adapter = requests.adapters.HTTPAdapter(
            pool_connections=config.IMG_PARALLEL,
            pool_maxsize=config.IMG_PARALLEL * 2,
        )
        session.mount("https://", adapter)
        session.mount("http://", adapter)

        success = skipped = failed = 0
        self._log(f"Téléchargement de {len(urls)} image(s) "
                  f"({config.IMG_PARALLEL} en parallèle)…")

        with ThreadPoolExecutor(max_workers=config.IMG_PARALLEL) as pool:
            futures = {pool.submit(self._download_one, url, session,
                                   out_dir, safe_kw): url
                       for url in urls}
            for fut in as_completed(futures):
                try:
                    status, detail = fut.result()
                except Exception as exc:
                    failed += 1
                    self._log(f"  ✗ {str(exc)[:100]}")
                    continue
                if status == "ok":
                    success += 1
                    self._progress(success, self.count)
                    self._thumb(detail)
                    self._log(f"  ✓ {detail}")
                elif status == "skip":
                    skipped += 1
                    self._log(f"  ⟳ {detail}, ignoré")

        if self._stop.is_set():
            self._log("Arrêté par l'utilisateur.")

        msg = (f"{success} image(s) téléchargée(s), "
               f"{skipped} ignorée(s), {failed} échec(s).")
        self._log(f"\n✅ {msg}")
        self._emit("done", success=True, message=msg)
