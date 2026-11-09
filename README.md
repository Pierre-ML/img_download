# Media Downloader

Application de bureau **Windows** (Tauri 2 + Astro + Rust) pour télécharger des images et vidéos depuis plusieurs sources, avec conversion automatique via ffmpeg.

## Fonctionnalités

- **Google Images** : vraie recherche Google dans une fenêtre cachée (onglet Images, défilement) ; en cas de captcha, la fenêtre s'affiche pour le résoudre.
- **Likes X** : médias des posts likés du compte connecté.
- **Enregistrés Reddit** : médias des posts enregistrés du compte connecté.
- **Conversion** : toutes les images en PNG (optimisation oxipng), vidéos et GIF animés en MP4 (H.264, faststart) via ffmpeg embarqué.
- **Connexion intégrée** : fenêtre de login X / Reddit dans l'app, aucune clé API à créer.
- File d'attente, déduplication entre les runs, galerie des médias récents.

## Direction artistique

Interface sombre reprise de [Lucania](https://github.com/Pierre-ML/Lucania) (daisyUI + Tailwind v4) : fonds `#0b0b0c` / `#141416` / `#1f1f22`, texte `#f2f2f3`, accent rouge `#e5383b` réservé aux signaux (erreurs, actions destructives). Les animaux low-poly (SVG en `currentColor`) illustrent chaque section ; la **pie**, l'oiseau qui collectionne ce qui brille, est le logo et la mascotte.

| Animal | Section |
|---|---|
| pie | logo, mascotte, états vides |
| hibou | Google Images |
| corbeau | Likes X |
| renard | Reddit enregistrés |
| aigle | Comptes |
| cerf | Réglages |

## Installation

Windows uniquement. Télécharger `MediaDownloader-Setup.exe` sur la [dernière release](https://github.com/Pierre-ML/img_download/releases/latest) et le lancer : installation pour l'utilisateur courant (sans droits administrateur) dans `%LOCALAPPDATA%\Programs\Media Downloader`.

## Mises à jour

Les mises à jour se font depuis l'application. Elle télécharge `MediaDownloader-Update.exe` (publié avec chaque release) et le lance : il attend la fermeture de l'app, copie les fichiers par-dessus l'installation existante (réglages et sessions conservés) puis relance l'app. Il peut aussi être lancé à la main (il refuse si l'app n'est pas installée ou si une version plus récente l'est).

## Prérequis (développement)

- Windows 10/11 avec WebView2 et les Build Tools MSVC
- Node 22+
- Rust stable

## Commandes

```bash
npm install
npm run fetch-ffmpeg -- --from-path   # copie le ffmpeg du PATH (sans option : téléchargement)
npm run desktop:dev                   # lance l'app en développement (équivalent : npm run tauri dev)
npm run desktop:build                 # build complet : produit les deux exe (voir ci-dessous)
```

`npm run desktop:build` enchaîne `tauri build` (installateur NSIS), `scripts/build-updater.mjs` (recompile le même script NSIS en mode mise à jour avec `/DMDL_UPDATER`) et `scripts/name-installers.mjs`. Résultat dans `src-tauri/target/release/bundle/release/` :

- `MediaDownloader-Setup.exe` : installateur complet (première installation) ;
- `MediaDownloader-Update.exe` : mise à jour, lancée par l'app avec `/MDL_WAITPID=<pid>`.

Le template NSIS personnalisé est dans `src-tauri/windows/` (`installer.nsi`, `hooks.nsh`).

## Release

Chaque push sur `main` dont la version (`src-tauri/tauri.conf.json`) n'a pas encore de tag `vX.Y.Z` déclenche le workflow `release.yml` (runner Windows) : vérification que toutes les versions concordent, build, puis release GitHub publiée avec `MediaDownloader-Setup.exe` et `MediaDownloader-Update.exe`. Si le tag existe déjà, rien n'est publié.

Pour publier :

```bash
npm run bump -- patch   # ou minor / major / x.y.z
git commit -am "vX.Y.Z" && git push origin main
```

## Données et sessions

Les sessions X / Reddit (cookies) sont stockées localement, **chiffrées** (AES-GCM) ; la clé est conservée dans le trousseau du système (Gestionnaire d'identifiants Windows). Rien n'est envoyé ailleurs que vers les sites concernés.

Les réglages sont dans le dossier de configuration de l'app (`settings.json`), les médias dans `<Images>/MediaDownloader` par défaut.
