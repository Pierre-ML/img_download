# CLAUDE.md — Media Downloader

## Vue d'ensemble

Application web de téléchargement de médias (images Google, vidéos, Reddit).
- `app.py` : serveur Flask (routes REST + streaming SSE)
- `config.py` : toutes les constantes et variables d'environnement
- `workers/` : threads de scraping/téléchargement
- `static/` : frontend HTML/CSS/JS vanilla servi directement par Flask

## Architecture

```
Navigateur ──POST /api/queue/add──► app.py ──► worker (thread)
           ◄─SSE /api/stream──              (events via queue.Queue)
           ◄─GET /api/media/──────────────── fichiers locaux
```

Les événements passent du worker vers le frontend via une `queue.Queue` (thread-safe).
Flask lit la queue dans un générateur SSE (`Response(..., mimetype="text/event-stream")`).
Plusieurs jobs peuvent être mis en file — un seul tourne à la fois (`_run_queue`).

## Structure des fichiers

```
img_download/
├── app.py                          # Routes Flask + runner de file d'attente
├── config.py                       # Constantes : port, dossier, Chrome, délais…
├── workers/
│   ├── image_worker.py             # DownloadWorker : Google Images → scraping + DL
│   ├── video_worker.py             # VideoWorker : yt-dlp (YouTube, Twitter, Vimeo…)
│   ├── reddit_worker.py            # RedditWorker : posts + subreddits
│   └── image_utils.py             # Parsing image (magic bytes, dimensions) sans PIL
└── static/
    ├── index.html                  # Page d'accueil (choix du mode)
    ├── images.html / videos.html / reddit.html
    ├── css/
    │   ├── style.css               # Point d'entrée (@import uniquement)
    │   ├── var.css                 # Custom properties CSS (design tokens)
    │   ├── base.css                # Reset, body, scrollbar, @keyframes
    │   ├── layout.css              # .layout, .header, .sidebar, .layout__main
    │   ├── home.css                # .home, .mode-card
    │   ├── badge.css / btn.css / card.css / field.css
    │   ├── progress.css / queue.css / log.css
    │   ├── gallery.css             # .gallery, .gallery__grid, .thumb, .empty
    │   └── toast.css
    └── js/
        ├── common.js               # SSE, queue, thumbnails, virtual scroll, toast
        ├── images.js               # addToQueue pour images
        ├── videos.js               # addToQueue pour vidéos
        └── reddit.js               # addToQueue pour Reddit
```

## Conventions

- **Pas de framework JS** — vanilla uniquement.
- **Pas de build step** — Flask sert directement `static/`.
- **BEM** pour les classes CSS (`block__element--modifier`).
- **CSS splitté en composants** — un fichier par bloc, `style.css` ne fait qu'importer.
- Variables CSS préfixées : `--clr-*` (couleurs), `--radius-*`, `--font-*`, `--transition`.
- Accent violet `#7c6af7`, fond très sombre `#080808`.

## Lancer en développement

```bash
python app.py   # http://localhost:5000
```

Variables d'environnement optionnelles (voir `config.py`) :
```
PORT=5000  HOST=0.0.0.0  MEDIA_DEST=~/Pictures/MediaDownloader  CHROME_VERSION=146
```

## Ajouter une fonctionnalité

| Besoin | Fichier |
|---|---|
| Nouvelle route API | `app.py` |
| Nouvelle constante | `config.py` |
| Nouvelle logique de scraping images | `workers/image_worker.py` |
| Nouveau type de média | nouveau worker + route dans `app.py` |
| Nouvel élément UI | HTML + classe BEM dans le CSS correspondant + logique dans `common.js` ou le JS de page |

## Points d'attention

- `CHROME_VERSION` dans `config.py` doit correspondre à la version Chrome installée.
- `image_utils.py` parse les dimensions/format en pur Python (sans PIL) — magic bytes uniquement.
- La déduplication se fait sur deux niveaux : URL (`seen`) et contenu MD5 (`seen_hashes`).
- Le virtual scroll (`_thumbObserver` dans `common.js`) décharge les images hors viewport pour tenir à 250+ images sans lag.
- Ne pas modifier l'ordre des callbacks dans `_make_callback()` (`app.py`) — ils mettent à jour `job.status` en plus d'écrire dans la queue SSE.
- `match/case` dans `_build_worker()` requiert Python 3.10+.

## Dépendances

```
flask>=3.0.0
selenium>=4.18.0
undetected-chromedriver>=3.5.5
requests>=2.31.0
yt-dlp
```

Pas de Pillow — validation et dimensions images faites en pur Python (`workers/image_utils.py`).
