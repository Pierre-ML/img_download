# CLAUDE.md — Image Downloader

## Vue d'ensemble

Application web de téléchargement d'images Google Images.
- `app.py` : serveur Flask (routes REST + streaming SSE)
- `worker.py` : thread de scraping/téléchargement (Selenium + requests)
- `static/` : frontend HTML/CSS/JS servi directement par Flask

## Architecture

```
Navigateur ──POST /api/start──► app.py ──► DownloadWorker (thread)
           ◄─SSE /api/stream──           (events via queue)
           ◄─GET /api/thumb/─────────── fichiers image locaux
```

Les événements passent du worker vers le frontend via une `queue.Queue` (thread-safe).
Flask lit la queue dans un générateur SSE (`Response(..., mimetype="text/event-stream")`).

## Fichiers clés

| Fichier               | Rôle                                                        |
|-----------------------|-------------------------------------------------------------|
| `app.py`              | Routes Flask : `/api/start`, `/api/stop`, `/api/stream`, `/api/thumb` |
| `worker.py`           | `DownloadWorker(Thread)` : Chrome → URLs → téléchargements  |
| `static/index.html`   | Structure HTML, imports CSS/JS                             |
| `static/css/style.css`| Tout le style (tokens CSS, layout, composants)             |
| `static/js/app.js`    | SSE client, fetch, manipulation DOM                        |

## Conventions

- **Pas de framework JS** côté frontend — vanilla uniquement.
- **Pas de build step** — Flask sert directement `static/`.
- Les styles utilisent des **custom properties CSS** (voir `:root` dans `style.css`).
- Le design suit l'esthétique du portfolio : fond très sombre, glassmorphism, bordures `rgba(255,255,255,0.08)`, accent violet `#7c6af7`.

## Lancer en développement

```bash
python app.py   # http://localhost:5000
```

## Ajouter une fonctionnalité

- **Nouvelle route API** → `app.py`
- **Nouvelle logique de scraping** → `worker.py` dans `_collect_urls()` ou `_download_one()`
- **Nouvel élément UI** → `static/index.html` + classe dans `style.css` + logique dans `app.js`

## Points d'attention

- `version_main=146` dans `worker.py` est fixé pour correspondre à la version Chrome installée. À mettre à jour si Chrome est mis à jour.
- La déduplication se fait par hash MD5 dans `_download_one()` via le set `seen_hashes`.
- Le set `seen` dans `_collect_urls()` déduplique les **URLs** ; `seen_hashes` déduplique le **contenu binaire**.
- Ne pas modifier l'ordre des callbacks dans `app.py` (`event_cb`) — ils mettent à jour `job.status` en plus d'écrire dans la queue.

## Dépendances

```
flask>=3.0.0
selenium>=4.18.0
undetected-chromedriver>=3.5.5
requests>=2.31.0
Pillow>=10.0.0
setuptools  # distutils shim pour Python 3.12+
```
