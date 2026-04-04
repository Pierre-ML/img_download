"""
Google Images Downloader
Interface graphique tkinter + Selenium (undetected-chromedriver) + requests
Compatible Windows 10/11, Python 3.10+
"""

import tkinter as tk
from tkinter import ttk, filedialog, scrolledtext
import threading
import os
import re
import time
import random
import hashlib
import requests
from pathlib import Path
from io import BytesIO
from PIL import Image, ImageTk
import undetected_chromedriver as uc
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.support.ui import WebDriverWait
from selenium.webdriver.support import expected_conditions as EC


# ---------------------------------------------------------------------------
# Constantes
# ---------------------------------------------------------------------------
SUPPORTED_EXTENSIONS = {
    "image/jpeg": ".jpg",
    "image/png":  ".png",
    "image/webp": ".webp",
    "image/gif":  ".jpg",  # on garde gif comme jpg
}
DEFAULT_EXT = ".jpg"
THUMBNAIL_SIZE = (90, 90)
DELAY_MIN, DELAY_MAX = 1.0, 3.0


# ---------------------------------------------------------------------------
# Logique de scraping / téléchargement (thread séparé)
# ---------------------------------------------------------------------------
class DownloadWorker(threading.Thread):
    """Thread qui pilote Chrome, scrape les URLs et télécharge les images."""

    def __init__(self, keyword: str, count: int, dest: str,
                 log_cb, progress_cb, thumb_cb, done_cb):
        super().__init__(daemon=True)
        self.keyword     = keyword
        self.count       = count
        self.dest        = dest
        self.log_cb      = log_cb        # callback(msg: str)
        self.progress_cb = progress_cb   # callback(current: int, total: int)
        self.thumb_cb    = thumb_cb      # callback(path: str)
        self.done_cb     = done_cb       # callback(success: bool, msg: str)
        self._stop_flag  = threading.Event()

    def stop(self):
        self._stop_flag.set()

    # ---- helpers -----------------------------------------------------------
    def _log(self, msg: str):
        self.log_cb(msg)

    def _sanitize(self, name: str) -> str:
        return re.sub(r'[\\/*?:"<>|]', "_", name)

    def _ext_from_url(self, url: str, content_type: str | None) -> str:
        if content_type:
            for mime, ext in SUPPORTED_EXTENSIONS.items():
                if mime in content_type:
                    return ext
        url_lower = url.lower().split("?")[0]
        for ext in (".jpg", ".jpeg", ".png", ".webp"):
            if url_lower.endswith(ext):
                return ".jpg" if ext == ".jpeg" else ext
        return DEFAULT_EXT

    # ---- scraping Google Images -------------------------------------------
    def _collect_urls(self, driver) -> list[str]:
        """Retourne une liste d'URLs d'images en faisant défiler la page."""
        urls: list[str] = []
        seen:  set[str] = set()

        search_url = (
            f"https://www.google.com/search?q={requests.utils.quote(self.keyword)}"
            "&tbm=isch&safe=off"
        )
        self._log(f"Navigation vers Google Images : {self.keyword!r}")
        driver.get(search_url)
        time.sleep(2)

        # Accepter les cookies si la bannière apparaît
        try:
            btn = driver.find_element(By.XPATH,
                '//button[contains(., "Tout accepter") or contains(., "Accept all")]')
            btn.click()
            time.sleep(1)
        except Exception:
            pass

        last_height = 0
        scroll_attempts = 0
        max_scrolls = 60

        while len(urls) < self.count and scroll_attempts < max_scrolls:
            if self._stop_flag.is_set():
                break

            # Collecte des thumbnails déjà chargés (src base64 ou https)
            thumbs = driver.find_elements(By.CSS_SELECTOR, "img.YQ4gaf")
            for img in thumbs:
                src = img.get_attribute("src") or ""
                if src and src not in seen and src.startswith("http"):
                    seen.add(src)
                    urls.append(src)

            # Essayons de cliquer sur les images pour obtenir l'URL haute résolution
            # (optionnel – on récupère au moins les miniatures)

            if len(urls) >= self.count:
                break

            # Scroll vers le bas
            driver.execute_script("window.scrollBy(0, 1200);")
            time.sleep(0.8)

            new_height = driver.execute_script("return document.body.scrollHeight")
            if new_height == last_height:
                # Chercher bouton "Voir plus de résultats"
                try:
                    btn = driver.find_element(By.CSS_SELECTOR,
                        'input[jsaction*="more"],'
                        'div[data-ved] input[type="button"]')
                    btn.click()
                    time.sleep(1.5)
                except Exception:
                    scroll_attempts += 5  # force la sortie
            last_height = new_height
            scroll_attempts += 1

        # ------------------------------------------------------------------
        # Tentative d'URLs haute résolution : on clique sur chaque miniature
        # et on récupère le panneau latéral
        # ------------------------------------------------------------------
        hires_urls: list[str] = []
        thumbs_all = driver.find_elements(By.CSS_SELECTOR, "img.YQ4gaf")
        target = min(self.count, len(thumbs_all))
        self._log(f"{len(thumbs_all)} miniatures trouvées, tentative haute résolution…")

        for i, thumb in enumerate(thumbs_all[:target * 2]):
            if self._stop_flag.is_set():
                break
            if len(hires_urls) >= self.count:
                break
            try:
                driver.execute_script("arguments[0].click();", thumb)
                time.sleep(0.8)
                # Cherche l'image principale dans le panneau
                panels = driver.find_elements(By.CSS_SELECTOR,
                    'img[jsname="kn3ccd"], img[class*="iPVvYb"],'
                    'a[jsname="sTFXNd"] img, div[jsname="figiqf"] img')
                for panel_img in panels:
                    src = panel_img.get_attribute("src") or ""
                    if src.startswith("http") and src not in seen:
                        seen.add(src)
                        hires_urls.append(src)
                        break
            except Exception:
                pass

        final = (hires_urls + urls)[:self.count]
        self._log(f"{len(final)} URL(s) collectée(s) (haute résolution + miniatures)")
        return final

    # ---- téléchargement ---------------------------------------------------
    def _download_image(self, url: str, filepath: Path, session: requests.Session) -> bool:
        headers = {
            "User-Agent": (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
                "AppleWebKit/537.36 (KHTML, like Gecko) "
                "Chrome/124.0.0.0 Safari/537.36"
            ),
            "Referer": "https://www.google.com/",
        }
        resp = session.get(url, headers=headers, timeout=15, stream=True)
        resp.raise_for_status()

        content_type = resp.headers.get("Content-Type", "")
        # Vérification que c'est bien une image
        if not any(m in content_type for m in ("image/", "application/octet")):
            return False

        raw = resp.content
        # Tentative de décodage pour valider l'image
        img = Image.open(BytesIO(raw))
        img.verify()  # lève une exception si corrompu

        # Re-ouvrir (verify() ferme l'objet)
        img = Image.open(BytesIO(raw))

        # Conversion RGBA → RGB si nécessaire pour JPEG
        if filepath.suffix.lower() in (".jpg", ".jpeg") and img.mode in ("RGBA", "P"):
            img = img.convert("RGB")

        img.save(filepath)
        return True

    # ---- run --------------------------------------------------------------
    def run(self):
        # Création du dossier de destination
        safe_kw  = self._sanitize(self.keyword)
        out_dir  = Path(self.dest) / safe_kw
        out_dir.mkdir(parents=True, exist_ok=True)
        self._log(f"Dossier de sortie : {out_dir}")

        # Lancement de Chrome (undetected)
        self._log("Démarrage de Chrome…")
        options = uc.ChromeOptions()
        options.add_argument("--window-size=1280,900")
        options.add_argument("--lang=fr-FR")
        # options.add_argument("--headless=new")  # décommenter pour mode headless
        driver = None

        try:
            driver = uc.Chrome(options=options, use_subprocess=True)
            urls = self._collect_urls(driver)
        except Exception as exc:
            self._log(f"[ERREUR] Chrome/scraping : {exc}")
            self.done_cb(False, str(exc))
            if driver:
                try:
                    driver.quit()
                except Exception:
                    pass
            return
        finally:
            if driver:
                try:
                    driver.quit()
                except Exception:
                    pass

        if not urls:
            self.done_cb(False, "Aucune URL collectée.")
            return

        # Téléchargement
        session   = requests.Session()
        success   = 0
        failed    = 0
        idx       = 0

        for url in urls:
            if self._stop_flag.is_set():
                self._log("Téléchargement annulé par l'utilisateur.")
                break

            idx += 1
            filename = f"{safe_kw}_{idx:03d}.jpg"  # extension provisoire
            filepath = out_dir / filename

            self._log(f"[{idx}/{len(urls)}] Téléchargement : {url[:80]}…")
            try:
                # Déterminer l'extension réelle
                head = session.head(url, timeout=8, allow_redirects=True,
                                    headers={"User-Agent": "Mozilla/5.0"})
                ct  = head.headers.get("Content-Type", "")
                ext = self._ext_from_url(url, ct)
                filepath = out_dir / f"{safe_kw}_{idx:03d}{ext}"

                ok = self._download_image(url, filepath, session)
                if ok:
                    success += 1
                    self.progress_cb(success, self.count)
                    self.thumb_cb(str(filepath))
                    self._log(f"  ✓ Sauvegardé : {filepath.name}")
                else:
                    failed += 1
                    self._log(f"  ✗ Type non image, ignoré.")

            except Exception as exc:
                failed += 1
                self._log(f"  ✗ Erreur : {exc}")

            # Délai aléatoire
            if success < self.count and idx < len(urls):
                delay = random.uniform(DELAY_MIN, DELAY_MAX)
                self._log(f"  … pause {delay:.1f}s")
                time.sleep(delay)

        msg = f"Terminé — {success} image(s) téléchargée(s), {failed} échec(s)."
        self._log(msg)
        self.done_cb(True, msg)


# ---------------------------------------------------------------------------
# Interface graphique
# ---------------------------------------------------------------------------
class App(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title("Google Images Downloader")
        self.resizable(True, True)
        self.minsize(780, 600)
        self.configure(bg="#1e1e2e")

        self._worker:   DownloadWorker | None = None
        self._thumb_images: list[ImageTk.PhotoImage] = []  # évite GC

        self._build_ui()

    # -----------------------------------------------------------------------
    # Construction de l'UI
    # -----------------------------------------------------------------------
    def _build_ui(self):
        PAD   = 12
        BG    = "#1e1e2e"
        CARD  = "#2a2a3e"
        ACCENT= "#7c6af7"
        FG    = "#cdd6f4"
        ENTRY_BG = "#313244"

        style = ttk.Style(self)
        style.theme_use("clam")
        style.configure("TProgressbar",
                         troughcolor=ENTRY_BG, background=ACCENT,
                         thickness=14, borderwidth=0)
        style.configure("TFrame", background=BG)

        # --- Panneau gauche (contrôles) ------------------------------------
        left = tk.Frame(self, bg=BG, padx=PAD, pady=PAD)
        left.pack(side=tk.LEFT, fill=tk.Y, padx=(PAD, 0), pady=PAD)

        def label(parent, text, **kw):
            return tk.Label(parent, text=text, bg=BG, fg=FG,
                            font=("Segoe UI", 9), **kw)

        def entry(parent, textvariable, width=28):
            return tk.Entry(parent, textvariable=textvariable, width=width,
                            bg=ENTRY_BG, fg=FG, insertbackground=FG,
                            relief=tk.FLAT, font=("Segoe UI", 10),
                            highlightthickness=1, highlightbackground=ACCENT)

        def btn(parent, text, cmd, color=ACCENT):
            return tk.Button(parent, text=text, command=cmd,
                             bg=color, fg="white",
                             activebackground="#5a4fcf", activeforeground="white",
                             relief=tk.FLAT, font=("Segoe UI", 9, "bold"),
                             cursor="hand2", padx=10, pady=5)

        # Titre
        tk.Label(left, text="Google Images Downloader",
                 bg=BG, fg=ACCENT, font=("Segoe UI", 13, "bold")).pack(pady=(0, PAD))

        # Mot-clé
        label(left, "Mot-clé de recherche :").pack(anchor=tk.W)
        self.var_keyword = tk.StringVar()
        entry(left, self.var_keyword).pack(fill=tk.X, pady=(2, 8))

        # Nombre d'images
        label(left, "Nombre d'images :").pack(anchor=tk.W)
        self.var_count = tk.StringVar(value="20")
        entry(left, self.var_count, width=10).pack(anchor=tk.W, pady=(2, 8))

        # Dossier destination
        label(left, "Dossier de destination :").pack(anchor=tk.W)
        dest_frame = tk.Frame(left, bg=BG)
        dest_frame.pack(fill=tk.X, pady=(2, 8))

        self.var_dest = tk.StringVar(
            value=str(Path.home() / "Pictures" / "GoogleImages"))
        tk.Entry(dest_frame, textvariable=self.var_dest,
                 bg=ENTRY_BG, fg=FG, insertbackground=FG,
                 relief=tk.FLAT, font=("Segoe UI", 9),
                 highlightthickness=1, highlightbackground=ACCENT).pack(
                     side=tk.LEFT, fill=tk.X, expand=True)
        btn(dest_frame, "…", self._choose_folder, color="#45475a").pack(
            side=tk.LEFT, padx=(4, 0))

        # Barre de progression
        label(left, "Progression :").pack(anchor=tk.W, pady=(4, 0))
        self.progress_var = tk.IntVar(value=0)
        self.progressbar  = ttk.Progressbar(
            left, variable=self.progress_var,
            maximum=100, mode="determinate", length=240)
        self.progressbar.pack(fill=tk.X, pady=(2, 2))
        self.lbl_progress = tk.Label(left, text="0 / 0",
                                     bg=BG, fg=FG, font=("Segoe UI", 8))
        self.lbl_progress.pack(anchor=tk.E)

        # Boutons action
        bf = tk.Frame(left, bg=BG)
        bf.pack(fill=tk.X, pady=(12, 0))
        self.btn_start = btn(bf, "▶  Rechercher & Télécharger", self._start)
        self.btn_start.pack(side=tk.LEFT, fill=tk.X, expand=True)
        self.btn_stop = btn(bf, "■  Stop", self._stop, color="#f38ba8")
        self.btn_stop.pack(side=tk.LEFT, padx=(6, 0))
        self.btn_stop.config(state=tk.DISABLED)

        # Log
        label(left, "Journal :").pack(anchor=tk.W, pady=(12, 0))
        self.log_box = scrolledtext.ScrolledText(
            left, width=38, height=14,
            bg=ENTRY_BG, fg="#a6e3a1",
            font=("Consolas", 8), relief=tk.FLAT,
            state=tk.DISABLED)
        self.log_box.pack(fill=tk.BOTH, expand=True, pady=(2, 0))

        # --- Panneau droit (aperçus) ---------------------------------------
        right = tk.Frame(self, bg=CARD, padx=PAD, pady=PAD)
        right.pack(side=tk.LEFT, fill=tk.BOTH, expand=True,
                   padx=PAD, pady=PAD)

        tk.Label(right, text="Aperçu des images téléchargées",
                 bg=CARD, fg=FG, font=("Segoe UI", 10, "bold")).pack(
                     anchor=tk.W, pady=(0, 6))

        # Canvas scrollable pour les miniatures
        canvas_frame = tk.Frame(right, bg=CARD)
        canvas_frame.pack(fill=tk.BOTH, expand=True)

        self.canvas = tk.Canvas(canvas_frame, bg=CARD,
                                highlightthickness=0, bd=0)
        scrollbar = tk.Scrollbar(canvas_frame, orient=tk.VERTICAL,
                                 command=self.canvas.yview)
        self.canvas.configure(yscrollcommand=scrollbar.set)
        scrollbar.pack(side=tk.RIGHT, fill=tk.Y)
        self.canvas.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)

        self.thumb_frame = tk.Frame(self.canvas, bg=CARD)
        self.canvas_window = self.canvas.create_window(
            (0, 0), window=self.thumb_frame, anchor=tk.NW)

        self.thumb_frame.bind("<Configure>", self._on_frame_configure)
        self.canvas.bind("<Configure>",      self._on_canvas_configure)
        self.canvas.bind("<MouseWheel>",     self._on_mousewheel)

        self._thumb_col = 0
        self._thumb_row = 0
        self._COLS      = 4

    # -----------------------------------------------------------------------
    # Événements canvas
    # -----------------------------------------------------------------------
    def _on_frame_configure(self, _event=None):
        self.canvas.configure(scrollregion=self.canvas.bbox("all"))

    def _on_canvas_configure(self, event):
        self.canvas.itemconfig(self.canvas_window, width=event.width)

    def _on_mousewheel(self, event):
        self.canvas.yview_scroll(int(-1 * (event.delta / 120)), "units")

    # -----------------------------------------------------------------------
    # Actions utilisateur
    # -----------------------------------------------------------------------
    def _choose_folder(self):
        folder = filedialog.askdirectory(title="Choisir le dossier de destination")
        if folder:
            self.var_dest.set(folder)

    def _start(self):
        keyword = self.var_keyword.get().strip()
        if not keyword:
            self._log("Veuillez entrer un mot-clé.")
            return
        try:
            count = int(self.var_count.get())
            if count <= 0:
                raise ValueError
        except ValueError:
            self._log("Nombre d'images invalide.")
            return

        dest = self.var_dest.get().strip()
        if not dest:
            self._log("Veuillez choisir un dossier.")
            return

        # Réinitialisation
        self._clear_thumbs()
        self._set_log("")
        self.progress_var.set(0)
        self.lbl_progress.config(text=f"0 / {count}")
        self._total = count

        self.btn_start.config(state=tk.DISABLED)
        self.btn_stop.config(state=tk.NORMAL)

        self._worker = DownloadWorker(
            keyword=keyword,
            count=count,
            dest=dest,
            log_cb      = lambda m: self.after(0, self._log, m),
            progress_cb = lambda c, t: self.after(0, self._update_progress, c, t),
            thumb_cb    = lambda p: self.after(0, self._add_thumb, p),
            done_cb     = lambda ok, m: self.after(0, self._on_done, ok, m),
        )
        self._worker.start()

    def _stop(self):
        if self._worker:
            self._worker.stop()
        self.btn_stop.config(state=tk.DISABLED)

    # -----------------------------------------------------------------------
    # Callbacks thread → UI
    # -----------------------------------------------------------------------
    def _log(self, msg: str):
        self.log_box.config(state=tk.NORMAL)
        self.log_box.insert(tk.END, msg + "\n")
        self.log_box.see(tk.END)
        self.log_box.config(state=tk.DISABLED)

    def _set_log(self, msg: str):
        self.log_box.config(state=tk.NORMAL)
        self.log_box.delete("1.0", tk.END)
        if msg:
            self.log_box.insert(tk.END, msg + "\n")
        self.log_box.config(state=tk.DISABLED)

    def _update_progress(self, current: int, total: int):
        pct = int(current / total * 100) if total else 0
        self.progress_var.set(pct)
        self.lbl_progress.config(text=f"{current} / {total}")

    def _add_thumb(self, path: str):
        try:
            img  = Image.open(path)
            img.thumbnail(THUMBNAIL_SIZE, Image.LANCZOS)
            # Fond pour les images transparentes
            bg = Image.new("RGB", THUMBNAIL_SIZE, (42, 42, 62))
            offset = ((THUMBNAIL_SIZE[0] - img.width)  // 2,
                      (THUMBNAIL_SIZE[1] - img.height) // 2)
            bg.paste(img, offset)
            photo = ImageTk.PhotoImage(bg)
            self._thumb_images.append(photo)  # garder référence

            cell = tk.Label(self.thumb_frame, image=photo,
                            bg="#2a2a3e", cursor="hand2",
                            relief=tk.FLAT, bd=2)
            cell.grid(row=self._thumb_row, column=self._thumb_col,
                      padx=3, pady=3)
            cell.bind("<Button-1>", lambda e, p=path: self._open_image(p))

            self._thumb_col += 1
            if self._thumb_col >= self._COLS:
                self._thumb_col = 0
                self._thumb_row += 1
        except Exception:
            pass  # miniature non affichable, on ignore

    def _clear_thumbs(self):
        for widget in self.thumb_frame.winfo_children():
            widget.destroy()
        self._thumb_images.clear()
        self._thumb_col = 0
        self._thumb_row = 0

    def _open_image(self, path: str):
        """Ouvre l'image dans la visionneuse par défaut du système."""
        import subprocess
        try:
            os.startfile(path)
        except Exception:
            subprocess.Popen(["explorer", path])

    def _on_done(self, success: bool, msg: str):
        self.btn_start.config(state=tk.NORMAL)
        self.btn_stop.config(state=tk.DISABLED)
        self._log(f"\n{'✅' if success else '❌'} {msg}")


# ---------------------------------------------------------------------------
# Point d'entrée
# ---------------------------------------------------------------------------
if __name__ == "__main__":
    app = App()
    app.mainloop()
