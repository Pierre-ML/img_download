# CLAUDE.md — Media Downloader

## Vue d'ensemble

Application web de téléchargement de médias (images Google, vidéos, Reddit, likes X).
- `app.py` : serveur Flask (routes REST + streaming SSE)
- `config.py` : toutes les constantes et variables d'environnement
- `accounts.py` : comptes utilisateur (Reddit, X) — stockés dans `accounts.json` (gitignoré)
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
├── accounts.py                     # Comptes Reddit/X → accounts.json (gitignoré, secrets jamais renvoyés au front)
├── workers/
│   ├── image_worker.py             # DownloadWorker : Google Images → scraping + DL
│   ├── video_worker.py             # VideoWorker : yt-dlp (YouTube, Twitter, Vimeo…)
│   ├── reddit_worker.py            # RedditWorker : posts + subreddits · RedditSavedWorker : posts enregistrés (OAuth script app)
│   ├── x_worker.py                 # XLikesWorker : likes X via gallery-dl (cookies navigateur)
│   └── image_utils.py             # Parsing image (magic bytes, dimensions, extension) + noms de fichiers sûrs, sans PIL
└── static/
    ├── index.html                  # Page d'accueil (choix du mode)
    ├── images.html / videos.html / reddit.html / x.html
    ├── comptes.html                # Configuration des comptes Reddit/X
    ├── css/
    │   ├── style.css               # Point d'entrée (@import uniquement)
    │   ├── var.css                 # Custom properties CSS (design tokens)
    │   ├── base.css                # Reset, body, scrollbar, @keyframes
    │   ├── layout.css              # .layout, .header, .sidebar, .layout__main
    │   ├── home.css                # .home, .mode-card, .home__accounts-link
    │   ├── accounts.css            # .accounts (page Comptes)
    │   ├── badge.css / btn.css / card.css / field.css
    │   ├── progress.css / queue.css / log.css
    │   ├── gallery.css             # .gallery, .gallery__grid, .thumb, .empty
    │   └── toast.css
    └── js/
        ├── common.js               # SSE, queue, thumbnails, virtual scroll, toast
        ├── images.js               # addToQueue pour images
        ├── videos.js               # addToQueue pour vidéos
        ├── reddit.js               # addToQueue pour Reddit (+ mes enregistrés)
        ├── x.js                    # addToQueue pour les likes X
        └── comptes.js              # Chargement/sauvegarde des comptes
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
- `image_utils.py` parse les dimensions/format/extension en pur Python (sans PIL) — magic bytes uniquement.
- La déduplication se fait sur deux niveaux : URL (`seen`) et contenu MD5 (`seen_hashes`).
- **Nommage des fichiers** : images Google `motclé_NNN_LARGExHAUT.ext` (numérotation continue entre les runs) ; Reddit `Titre du post.ext` (`_NN` pour les galeries, suffixe `_2` en cas de collision via `unique_path`) ; vidéos `Titre [id].mp4`. L'extension vient des magic bytes, pas de l'URL.
- Les téléchargements d'images sont **parallèles** (`IMG_PARALLEL` / `REDDIT_PARALLEL` dans `config.py`) — l'état partagé (`seen_hashes`, numérotation) est protégé par un lock ; les vidéos restent en série (yt-dlp + ffmpeg).
- **Comptes** : `accounts.json` est gitignoré ; les secrets ne transitent jamais vers le frontend (`accounts.masked()` renvoie des booléens `*_set`). Reddit = OAuth « script app » (grant password, 2FA → `motdepasse:code2FA`). X = cookies du navigateur via gallery-dl (`--cookies-from-browser`, Firefox recommandé — Chrome récent bloque la lecture des cookies) ou `cookies.txt`.
- `XLikesWorker` lance gallery-dl en **sous-processus** (`sys.executable -m gallery_dl`) — gallery-dl doit être installé dans le même environnement Python que Flask.
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
gallery-dl
```

Pas de Pillow — validation et dimensions images faites en pur Python (`workers/image_utils.py`).
