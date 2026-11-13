<div align="center"><img src="app-icon.svg" alt="Lucascrap" width="96" /><h1>Lucascrap</h1><p>Récupérez vos images et vidéos depuis Google Images, X et Reddit, au format de votre choix.<br/>Grab your images and videos from Google Images, X and Reddit, in the format you choose.</p>
<a href="https://github.com/Pierre-ML/lucascrap/releases/latest/download/Lucascrap-Setup.exe"><b>⬇ Télécharger pour Windows · Download for Windows</b></a><br/><sub>setup pour installer · update pour mettre à jour / setup to install · update to update · <a href="https://github.com/Pierre-ML/lucascrap/releases/latest">releases</a></sub></div>

<p align="center">
<a href="https://github.com/Pierre-ML/lucascrap/releases/latest"><img src="https://img.shields.io/github/v/release/Pierre-ML/lucascrap" alt="Dernière release" /></a>
<a href="LICENSE"><img src="https://img.shields.io/badge/licence-MIT-green" alt="MIT" /></a>
<img src="https://img.shields.io/badge/plateforme-Windows-blue" alt="Windows" />
<a href="https://github.com/Pierre-ML/lucascrap/releases"><img src="https://img.shields.io/github/downloads/Pierre-ML/lucascrap/total" alt="Téléchargements" /></a>
</p>

<p align="center"><a href="#français">Français</a> · <a href="#english">English</a></p>

<!-- TODO: capture d'écran -->

---

# Français

## Présentation

Lucascrap est une application de bureau Windows qui télécharge des images et des vidéos depuis Google Images, vos likes X et vos posts enregistrés Reddit. Chaque média est converti au format choisi grâce à ffmpeg, embarqué dans l'application.

La mascotte est la pie, l'oiseau qui collectionne ce qui brille.

Vos sessions et vos médias restent sur votre machine. Aucune clé d'API n'est nécessaire.

## Fonctionnalités

- 🦉 **Google Images** : une vraie recherche Google, menée dans une fenêtre cachée. Si un captcha apparaît, la fenêtre s'affiche pour que vous le résolviez.
- 🐦‍⬛ **Likes X** : les médias des posts likés par le compte connecté.
- 🦊 **Enregistrés Reddit** : les médias des posts enregistrés par le compte connecté.
- 🔐 **Connexion intégrée** : vous vous connectez à X et Reddit dans l'application. Les sessions sont chiffrées sur le disque.
- 🖼️ **Images** en PNG (sans perte, optimisation oxipng), JPG, WebP ou AVIF (le plus léger), avec une qualité réglable.
- 🎞️ **Vidéos et GIF** en MP4 H.264 (compatibilité maximale), MP4 H.265, WebM VP9 ou WebM AV1 (le plus léger).
- 🎯 Format choisi par défaut dans les Réglages, modifiable pour chaque tâche.
- 📋 **File d'attente** : les tâches passent une par une, avec un journal en direct et une limite facultative de médias.
- 🧬 **Pas de doublons** : un média déjà téléchargé est ignoré, même lors d'une session suivante (identifiant de la source, URL et empreinte SHA-256 du fichier).
- 🗂️ **Fichiers bien rangés** : `Google/<mot-clé>/`, `X/Likes/` et `Reddit/Saved/`, avec des noms lisibles (mot-clé, auteur, titre du post).
- 🖼️ **Galerie** des derniers médias téléchargés.
- ↔️ **Panneaux redimensionnables**, tailles mémorisées. Un double-clic sur une poignée rétablit la taille par défaut.
- 🔄 **Mise à jour** depuis l'application.
- 🌑 Interface sombre, reprise de [Lucania](https://github.com/Pierre-ML/Lucania).

## Installation

1. Pour une première installation, téléchargez [`Lucascrap-Setup.exe`](https://github.com/Pierre-ML/lucascrap/releases/latest/download/Lucascrap-Setup.exe) (lien direct vers la dernière release).
2. Lancez l'installateur. Lucascrap s'installe pour votre compte Windows, dans `%LOCALAPPDATA%\Programs\Lucascrap`. Aucune autorisation administrateur n'est demandée.

Prérequis : Windows 10 ou 11 (64 bits), avec WebView2 (déjà présent sur Windows 11).

> **Antivirus.** L'exécutable n'est pas encore signé. Windows SmartScreen ou votre antivirus peut donc l'analyser ou afficher un avertissement.

Le nom du fichier ne change jamais : une seule exception dans votre antivirus suffit pour toutes les versions.

## Premiers pas

1. **Google Images** : saisissez un mot-clé et le nombre d'images voulu, puis lancez la tâche.
2. **X / Reddit** : dans **Comptes**, connectez-vous une fois. Lancez ensuite la récupération depuis **Likes X** ou **Reddit enregistrés** (limite `0` = tout récupérer).
3. Les médias arrivent dans `Images\Lucascrap` (modifiable dans **Réglages**) et apparaissent dans la galerie.

## Mise à jour

Depuis l'application : le bandeau de mise à jour vérifie la dernière release GitHub, télécharge l'updater et l'installe après confirmation. Lucascrap se relance tout seul.

À la main : téléchargez [`Lucascrap-Update.exe`](https://github.com/Pierre-ML/lucascrap/releases/latest/download/Lucascrap-Update.exe) (lien direct vers la dernière release) et lancez-le. Il refuse de s'exécuter si Lucascrap n'est pas installé ou si une version plus récente l'est déjà.

Vos réglages, vos sessions et vos médias sont conservés.

**Vous aviez « Media Downloader » ?** C'est l'ancien nom de Lucascrap. L'ancienne version ne peut pas se mettre à jour toute seule vers la nouvelle. Lancez une fois `Lucascrap-Setup.exe` : vos réglages et vos sessions sont repris tels quels. Désinstallez ensuite « Media Downloader » depuis les Paramètres Windows. ⚠️ Ne cochez **pas** « supprimer les données de l'application » : ces données sont partagées avec Lucascrap.

## Développement

Prérequis : Windows 10 ou 11, Node 22 ou plus, Rust stable (MSVC, 1.80 minimum) et les Visual Studio Build Tools.

| Commande | Rôle |
| --- | --- |
| `npm install` | Installer les dépendances |
| `npm run fetch-ffmpeg -- --from-path` | Copier le ffmpeg du PATH comme sidecar (sans option : le télécharger). À faire une fois. |
| `npm run desktop:dev` | Lancer l'application en développement |
| `npm run desktop:build` | Produire les deux exe Windows (`Lucascrap-Setup.exe` et `Lucascrap-Update.exe`, dans `src-tauri/target/release/bundle/release/`) |
| `npm run check` | Vérifier les types du frontend |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Tests Rust |
| `npm run bump -- patch` | Monter la version partout (`minor`, `major` ou `x.y.z` acceptés) |

`npm run dev` seul affiche l'interface sur http://localhost:4321, sans le cœur Rust : utilisez `npm run desktop:dev`.

Version : source unique dans `src-tauri/tauri.conf.json`, identique dans `package.json`, `package-lock.json` et `src-tauri/Cargo.toml`. Le workflow `.github/workflows/ci.yml` vérifie chaque push. Si la version de `main` n'a pas encore de tag `vX.Y.Z`, il construit les installateurs sous Windows et publie la release.

## Stack

Astro 7 · TypeScript vanilla · Tailwind CSS v4 · daisyUI 5 · Tauri v2 · Rust (`tokio`, `reqwest`, `oxipng`, `aes-gcm`, `keyring`) · ffmpeg (sidecar).

## Structure

- `src` : frontend Astro (écrans, galerie, file d'attente, styles et animaux low-poly).
- `src-tauri/src` : cœur Rust (file d'attente, pipeline de téléchargement et de conversion, comptes, mise à jour).
- `src-tauri/src/sources` : une source par fichier (`google.rs`, `x.rs`, `reddit.rs`).
- `src-tauri/windows` : installateur NSIS (setup et update).
- `scripts` : ffmpeg, build des installateurs, gestion de la version.

## Confidentialité

Lucascrap ne contacte que :

- Google, X et Reddit (et leurs serveurs de médias), pour les tâches que vous lancez ;
- GitHub, pour vérifier et télécharger les mises à jour.

Les sessions X et Reddit (cookies) sont chiffrées en AES-GCM. La clé est rangée dans le Gestionnaire d'identifiants Windows. Les secrets ne quittent jamais le cœur de l'application.

Emplacements :

- réglages, sessions et historique des téléchargements : `%APPDATA%\com.mediadownloader.app` (identifiant hérité de l'ancien nom, conservé pour garder vos données) ;
- médias : `Images\Lucascrap` par défaut. La désinstallation ne les supprime jamais.

## Usage responsable

Lucascrap sert à récupérer vos propres contenus (likes, enregistrés) et des images publiques. Respectez les conditions d'utilisation des sites et les droits des auteurs.

## Licence

[MIT](LICENSE). Dépôt : https://github.com/Pierre-ML/lucascrap

---

# English

## Overview

Lucascrap is a Windows desktop app that downloads images and videos from Google Images, your X likes and your saved Reddit posts. Each file is converted to the format you choose with ffmpeg, bundled in the app.

Its mascot is the magpie, the bird that collects shiny things.

Your sessions and your media stay on your machine. No API key is needed.

## Features

- 🦉 **Google Images**: a real Google search, run in a hidden window. If a captcha shows up, the window appears so you can solve it.
- 🐦‍⬛ **X likes**: media from the posts liked by the signed-in account.
- 🦊 **Saved Reddit posts**: media from the posts saved by the signed-in account.
- 🔐 **Built-in sign-in**: you sign in to X and Reddit inside the app. Sessions are encrypted on disk.
- 🖼️ **Images** as PNG (lossless, oxipng optimization), JPG, WebP or AVIF (smallest), with adjustable quality.
- 🎞️ **Videos and GIFs** as MP4 H.264 (widest compatibility), MP4 H.265, WebM VP9 or WebM AV1 (smallest).
- 🎯 Default format set in Settings, can be changed for each job.
- 📋 **Queue**: jobs run one at a time, with a live log and an optional media limit.
- 🧬 **No duplicates**: a file already downloaded is skipped, even in a later session (source ID, URL and SHA-256 of the file).
- 🗂️ **Tidy folders**: `Google/<keyword>/`, `X/Likes/` and `Reddit/Saved/`, with readable names (keyword, author, post title).
- 🖼️ **Gallery** of the latest downloads.
- ↔️ **Resizable panels**, sizes remembered. Double-click a handle to restore the default size.
- 🔄 **In-app update**.
- 🌑 Dark interface, borrowed from [Lucania](https://github.com/Pierre-ML/Lucania).

The interface is in French.

## Installation

1. For a first install, download [`Lucascrap-Setup.exe`](https://github.com/Pierre-ML/lucascrap/releases/latest/download/Lucascrap-Setup.exe) (direct link, latest release).
2. Run the installer. Lucascrap installs for your Windows account, in `%LOCALAPPDATA%\Programs\Lucascrap`. No administrator permission is asked.

Requirements: Windows 10 or 11 (64-bit), with WebView2 (already present on Windows 11).

> **Antivirus.** The executable is not signed yet. Windows SmartScreen or your antivirus may scan it or show a warning.

The file name never changes: a single exception in your antivirus is enough for all versions.

## Getting started

1. **Google Images**: type a keyword and the number of images you want, then start the job.
2. **X / Reddit**: sign in once under **Comptes** (Accounts). Then start a job from **Likes X** or **Reddit enregistrés** (limit `0` = fetch everything).
3. Files land in `Pictures\Lucascrap` (configurable in **Réglages**, Settings) and show up in the gallery.

## Updating

From the app: the update banner checks the latest GitHub release, downloads the updater and installs it after confirmation. Lucascrap restarts on its own.

By hand: download [`Lucascrap-Update.exe`](https://github.com/Pierre-ML/lucascrap/releases/latest/download/Lucascrap-Update.exe) (direct link, latest release) and run it. It refuses to run if Lucascrap is not installed or if a newer version already is.

Your settings, sessions and media are kept.

**Had "Media Downloader"?** That is Lucascrap's former name. The old version cannot update itself to the new one. Run `Lucascrap-Setup.exe` once: your settings and sessions carry over as they are. Then uninstall "Media Downloader" from Windows Settings. ⚠️ Do **not** tick "delete the application data": that data is shared with Lucascrap.

## Development

Requirements: Windows 10 or 11, Node 22 or newer, stable Rust (MSVC, 1.80 minimum) and Visual Studio Build Tools.

| Command | Purpose |
| --- | --- |
| `npm install` | Install dependencies |
| `npm run fetch-ffmpeg -- --from-path` | Copy ffmpeg from PATH as the sidecar (no option: download it). Needed once. |
| `npm run desktop:dev` | Run the app in development |
| `npm run desktop:build` | Build both Windows exes (`Lucascrap-Setup.exe` and `Lucascrap-Update.exe`, in `src-tauri/target/release/bundle/release/`) |
| `npm run check` | Type check the frontend |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Rust tests |
| `npm run bump -- patch` | Bump the version everywhere (`minor`, `major` or `x.y.z` also work) |

`npm run dev` alone shows the interface on http://localhost:4321, without the Rust core: use `npm run desktop:dev`.

Version: single source in `src-tauri/tauri.conf.json`, identical in `package.json`, `package-lock.json` and `src-tauri/Cargo.toml`. The `.github/workflows/ci.yml` workflow checks every push. When the version on `main` has no `vX.Y.Z` tag yet, it builds the installers on Windows and publishes the release.

## Stack

Astro 7 · vanilla TypeScript · Tailwind CSS v4 · daisyUI 5 · Tauri v2 · Rust (`tokio`, `reqwest`, `oxipng`, `aes-gcm`, `keyring`) · ffmpeg (sidecar).

## Structure

- `src`: Astro frontend (screens, gallery, queue, styles and low-poly animals).
- `src-tauri/src`: Rust core (queue, download and conversion pipeline, accounts, updates).
- `src-tauri/src/sources`: one source per file (`google.rs`, `x.rs`, `reddit.rs`).
- `src-tauri/windows`: NSIS installer (setup and update).
- `scripts`: ffmpeg, installer builds, version management.

## Privacy

Lucascrap only contacts:

- Google, X and Reddit (and their media servers), for the jobs you start;
- GitHub, to check and download updates.

X and Reddit sessions (cookies) are encrypted with AES-GCM. The key is kept in Windows Credential Manager. Secrets never leave the app's core.

Locations:

- settings, sessions and download history: `%APPDATA%\com.mediadownloader.app` (identifier inherited from the former name, kept to preserve your data);
- media: `Pictures\Lucascrap` by default. Uninstalling never deletes them.

## Responsible use

Lucascrap is meant for fetching your own content (likes, saved posts) and public images. Respect the sites' terms of use and the authors' rights.

## License

[MIT](LICENSE). Repository: https://github.com/Pierre-ML/lucascrap
