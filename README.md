# Media Downloader

Application de bureau (Tauri 2 + Astro + Rust) pour télécharger des images et vidéos depuis plusieurs sources, avec conversion automatique via ffmpeg.

## Fonctionnalités

- **Google Images** : téléchargement par mot-clé.
- **Likes X** : médias des posts likés du compte connecté.
- **Enregistrés Reddit** : médias des posts enregistrés du compte connecté.
- **Conversion** : toutes les images en PNG (optimisation oxipng), vidéos et GIF animés en MP4 (H.264, faststart) via ffmpeg embarqué.
- **Connexion intégrée** : fenêtre de login X / Reddit dans l'app, aucune clé API à créer.
- File d'attente, déduplication entre les runs, galerie des médias récents.

## Prérequis (développement)

- Node 22+
- Rust stable
- Windows : WebView2 (inclus dans Windows 11) et Build Tools MSVC
- Linux : `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libdbus-1-dev pkg-config libssl-dev`
- macOS : Xcode Command Line Tools

## Commandes

```bash
npm install
npm run fetch-ffmpeg -- --from-path   # copie le ffmpeg du PATH (sans option : téléchargement)
npm run tauri dev                     # lance l'app en développement
npm run tauri build                   # produit l'installeur dans src-tauri/target/release/bundle
```

## Release

Pousser un tag `vX.Y.Z` déclenche le workflow GitHub Actions `build.yml` : build Windows, Linux et macOS (arm64 et x64), puis création d'une release brouillon avec les installeurs.

```bash
git tag v1.0.0 && git push origin v1.0.0
```

## Données et sessions

Les sessions X / Reddit (cookies) sont stockées localement, **chiffrées** (AES-GCM) ; la clé est conservée dans le trousseau du système (Gestionnaire d'identifiants Windows, Keychain macOS, Secret Service Linux). Rien n'est envoyé ailleurs que vers les sites concernés.

Les réglages sont dans le dossier de configuration de l'app (`settings.json`), les médias dans `<Images>/MediaDownloader` par défaut.
