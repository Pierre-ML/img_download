# Media Downloader

Application web de téléchargement de médias : images Google, vidéos (YouTube, Twitter, Vimeo…) et médias Reddit.
Backend Flask + Selenium, frontend HTML/CSS/JS vanilla.

---

## Stack

| Couche      | Technologie                           |
|-------------|---------------------------------------|
| Backend     | Python 3.10+ / Flask                  |
| Scraping    | Selenium + undetected-chromedriver    |
| Vidéos      | yt-dlp                                |
| Validation  | pur Python (magic bytes, sans PIL)    |
| Frontend    | HTML · CSS (BEM) · JS vanilla         |

---

## Prérequis

- Python 3.10+
- Google Chrome installé
- Node.js **non requis**

---

## Installation

```bash
pip install flask selenium undetected-chromedriver requests yt-dlp
```

---

## Lancement

```bash
python app.py
```

Ouvrir : [http://localhost:5000](http://localhost:5000)

Variables d'environnement optionnelles :

```bash
PORT=5000
MEDIA_DEST=~/Pictures/MediaDownloader
CHROME_VERSION=146   # à ajuster selon votre version Chrome
```

---

## Modes

### Images
Recherche Google Images par mot-clé. Scraping automatique + tentative haute résolution via le panneau latéral Google.

```
~/Pictures/MediaDownloader/
└── paysage montagne/
    ├── paysage montagne_001.jpg
    ├── paysage montagne_002.png
    └── ...
```

### Vidéos
Paste d'URLs (une par ligne) depuis YouTube, Twitter/X, Vimeo, Reddit ou tout lien direct. Téléchargement en meilleure qualité disponible, rendu en MP4.

### Reddit
URL d'un post ou d'un subreddit entier. Extrait toutes les images et vidéos, y compris les galeries Reddit et les vidéos natives v.redd.it.

---

## Fonctionnalités

- File d'attente multi-jobs (les jobs s'enchaînent automatiquement)
- Logs en temps réel via SSE (Server-Sent Events)
- Barre de progression live
- Aperçu miniature cliquable dans l'interface
- Virtual scroll : 250+ images sans lag (images hors écran déchargées de la mémoire)
- Déduplication par hash MD5 (évite les doublons de contenu)
- Délais aléatoires entre téléchargements (rate-limit friendly)
- Bouton Stop pour interrompre le job en cours
- Bouton Réinitialiser pour tout effacer et repartir à zéro
- Console masquable (bouton Masquer/Afficher)

---

## Structure du projet

```
img_download/
├── app.py              # Serveur Flask — routes API + runner de file
├── config.py           # Constantes centralisées (port, délais, tailles…)
├── workers/
│   ├── image_worker.py # Scraping Google Images
│   ├── video_worker.py # Téléchargement vidéo via yt-dlp
│   ├── reddit_worker.py# Extraction médias Reddit
│   └── image_utils.py  # Parsing image pur Python (sans PIL)
└── static/
    ├── index.html / images.html / videos.html / reddit.html
    ├── css/            # CSS splitté en composants (BEM)
    │   ├── style.css   # Point d'entrée (@import)
    │   ├── var.css     # Design tokens
    │   └── ...         # Un fichier par composant
    └── js/
        ├── common.js   # Logique partagée (SSE, thumbnails, virtual scroll)
        ├── images.js / videos.js / reddit.js
```

---

## Notes

- `undetected-chromedriver` gère le téléchargement automatique du ChromeDriver.
- Si Chrome est mis à jour, changer `CHROME_VERSION` dans `config.py` ou via la variable d'environnement.
- Pas de Pillow : la validation et les dimensions des images sont lues directement depuis les octets (magic bytes).
