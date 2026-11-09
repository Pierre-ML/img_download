# CLAUDE.md — Media Downloader

## Vue d'ensemble

Application desktop **Windows uniquement** (Tauri 2) de téléchargement de médias : Google Images, likes X, posts enregistrés Reddit. Les images sont converties en PNG, les vidéos/GIF en MP4 via ffmpeg (sidecar).
- `src/` : frontend Astro + Tailwind v4, TypeScript vanilla (pas de framework JS)
- `src-tauri/` : cœur Rust (file d'attente, sources, pipeline, auth)
- `scripts/fetch-ffmpeg.mjs` : récupère le binaire ffmpeg sidecar
- `scripts/build-updater.mjs`, `name-installers.mjs` : produisent `MediaDownloader-Setup.exe` et `MediaDownloader-Update.exe`
- `src-tauri/windows/` : template NSIS personnalisé (setup + mode updater) et hooks

## Architecture

```
Frontend (Astro) ──invoke(commande)──► Rust (#[tauri::command])
                 ◄──events (app.emit)──  job-update / job-log / media-saved / account-changed
```

- Un seul job tourne à la fois (`queue.rs`), les autres restent `queued`.
- Chaque source (`sources/*.rs`) produit des `MediaCandidate` dans un `mpsc::Sender` ; `pipeline.rs` les télécharge en parallèle, dédoublonne, convertit (ffmpeg, oxipng) et écrit dans le dossier de destination.
- `limit` > 0 : quand `saved` atteint la limite, le core annule la source (drop du receiver + `CancellationToken`).
- Commandes : `get_settings`, `save_settings`, `queue_add`, `queue_list`, `queue_cancel`, `queue_clear_finished`, `recent_media`, `ffmpeg_status`, `account_status`, `account_login`, `account_logout`. Toutes renvoient `Result<T, String>`.
- Événements : `job-update` (throttlé ~10/s), `job-log`, `media-saved`, `account-changed`.

## Structure des fichiers

```
img_download/
├── package.json, astro.config.mjs, tsconfig.json
├── app-icon.svg / app-icon.png      # source des icônes (npx @tauri-apps/cli icon)
├── scripts/fetch-ffmpeg.mjs         # ffmpeg → src-tauri/binaries/ffmpeg-<triple>[.exe]
├── scripts/build-updater.mjs        # recompile le NSIS rendu avec /DMDL_UPDATER → exe de mise à jour
├── scripts/name-installers.mjs      # noms fixes → src-tauri/target/release/bundle/release/
├── scripts/bump-version.mjs         # version : package.json, package-lock.json, tauri.conf.json, Cargo.toml/lock
├── .github/workflows/               # release.yml (build Windows + release auto sur push main), check.yml (CI Windows)
├── src/                             # Frontend Astro (pages, composants, TS, styles Tailwind v4 + daisyUI)
│   ├── lib/animals.ts               # animaux low-poly (viewBox 0 0 64 64)
│   ├── components/Animal.astro      # rend un animal en SVG (currentColor)
│   └── styles/global.css            # thème daisyUI « lucania » + tokens
├── public/favicon.svg               # pie low-poly
└── src-tauri/
    ├── Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json
    ├── icons/, binaries/ (gitignoré)
    ├── windows/                     # installer.nsi (template NSIS : setup + updater), hooks.nsh
    └── src/
        ├── main.rs, lib.rs          # point d'entrée, enregistrement des commandes
        ├── settings.rs              # Settings (settings.json)
        ├── queue.rs                 # file d'attente, JobRequest/JobInfo, événements
        ├── pipeline.rs              # téléchargement, dédup, conversion, nommage
        ├── ffmpeg.rs                # localisation et appel de ffmpeg
        ├── auth.rs                  # login webview, sessions chiffrées, commandes account_*
        └── sources/
            ├── mod.rs               # MediaCandidate, SourceCtx, JobLogger
            ├── google.rs            # collect(ctx, query, tx)
            ├── x.rs                 # collect_likes(ctx, tx)
            └── reddit.rs            # collect_saved(ctx, tx)
```

## Conventions

- **Astro + Tailwind v4**, TypeScript vanilla côté front, Rust côté core.
- **camelCase** pour les arguments `invoke` et tout le JSON (`#[serde(rename_all = "camelCase")]`) ; `JobRequest` est taggé `kind`.
- **Un module par source** dans `src-tauri/src/sources/`.
- Erreurs Rust : `anyhow` en interne, `Result<T, String>` aux frontières des commandes.
- Messages utilisateur en français.
- Google Images : vraie recherche Google dans une fenêtre cachée (onglet Images, défilement) ; en cas de captcha, la fenêtre s'affiche pour le résoudre.

## Direction artistique

DA reprise de Lucania (même thème, mêmes polices système, mêmes rayons) : daisyUI + tokens dans `src/styles/global.css`.

- Thème `lucania` : fonds `#0b0b0c` / `#141416` / `#1f1f22`, texte `#f2f2f3`, primaire blanc cassé (boutons pleins `bg-fg text-bg`), accent rouge `#e5383b` **réservé aux signaux** (erreurs, actions destructives, états critiques). Aucun violet.
- Tokens Tailwind : `bg`, `fg`, `surface`, `border`, `muted`, `sidebar`, `code`, `danger` ; `success`/`warning` pour les états. Classes de composants : `panel`, `field`, `field-label`, `button-primary|ghost|danger`, `pill`, `toggle-switch`, `nav-item` (noms choisis pour ne pas entrer en collision avec les composants daisyUI : `card`, `btn`, `label`, `badge`, `switch`).
- **Animaux** : `src/lib/animals.ts` (viewBox 0 0 64 64, `facets` {points, opacity}, `eyes`, `accents`) + `src/components/Animal.astro` (props `id`, `class`). Règles : tout en `currentColor` (aucune couleur, pas de rouge), décoratif (`aria-hidden`), pas d'animation, un animal par section, le loup reste réservé à Lucania. Motif titre : `ScreenHeader.astro` (animal `size-7` + titre) ; état vide : animal `size-14 text-fg/25` + texte.
- **La pie** est le logo et la mascotte (l'oiseau qui collectionne ce qui brille) : sidebar, états vides (galerie, file), bandeau de mise à jour, favicon, icône de l'app (`app-icon.svg`, pie claire sur carré arrondi `#0b0b0c`).

| Animal | Section |
|---|---|
| pie | logo, mascotte, états vides, bandeau de mise à jour |
| hibou | Google Images |
| corbeau | Likes X |
| renard | Reddit enregistrés |
| aigle | Comptes |
| cerf | Réglages |

hibou, corbeau, renard, aigle et cerf sont copiés à l'identique de Lucania ; seule la pie est propre à ce logiciel.

## Lancer en développement

```bash
npm install
npm run fetch-ffmpeg -- --from-path   # une fois (ffmpeg du PATH), ou sans option pour télécharger
npm run desktop:dev                   # = tauri dev
npm run desktop:build                 # tauri build + build-updater + name-installers → 2 exe
cargo test --manifest-path src-tauri/Cargo.toml
```

Release : chaque push sur `main` dont la version (`tauri.conf.json`, source de vérité) n'a pas encore de tag `vX.Y.Z` déclenche `release.yml` (Windows) : build `npm run desktop:build` puis release GitHub avec `MediaDownloader-Setup.exe` et `MediaDownloader-Update.exe`. Pour publier : `npm run bump -- patch`, commit, push `main`. Le workflow échoue si les versions (tauri.conf.json, package.json, package-lock.json, Cargo.toml) divergent.

## Ajouter une source

1. Créer `src-tauri/src/sources/<nom>.rs` exposant `pub async fn collect_xxx(ctx: &SourceCtx, tx: Sender<MediaCandidate>) -> anyhow::Result<()>` et le déclarer dans `sources/mod.rs`.
2. Ajouter une variante à `JobRequest` (`queue.rs`) et l'aiguiller dans le runner.
3. Si un compte est requis : ajouter un `Provider` dans `auth.rs`.
4. Ajouter l'écran correspondant dans `src/` (formulaire → `queue_add`).

Règles d'une source : s'arrêter si `tx.send` échoue ou si `ctx.cancel` est levé, paginer jusqu'à épuisement avec petites pauses (300–800 ms), `ctx.log.info` à chaque page, `bail!("Compte … non connecté")` si la session manque. `MediaCandidate.key` doit être stable (`google:<url>`, `x:<tweetId>:<i>`, `reddit:<postId>:<i>`).

## Points d'attention

- **Installateurs Windows** : `src-tauri/windows/installer.nsi` est un template NSIS personnalisé (basé sur celui de tauri-bundler, modifications marquées `; MDL:`), installMode `currentUser`. Le même script produit deux exe : le SETUP (`tauri build`, première installation, refuse si déjà installé) et l'UPDATE (`build-updater.mjs`, `/DMDL_UPDATER` : copie par-dessus l'installation existante, relance l'app une fois). Contrat avec `updater.rs` : l'app télécharge `releases/latest/download/MediaDownloader-Update.exe` et le lance avec `/MDL_WAITPID=<pid>` ; le `.onInit` attend ce PID (15 s max). Noms d'assets à ne pas changer sans mettre à jour `updater.rs`. `mainBinaryName` = `media-downloader`. Le sidecar `ffmpeg.exe` est installé/supprimé via `externalBin`.
- **ffmpeg sidecar** : `bundle.externalBin = ["binaries/ffmpeg"]` ; le fichier `ffmpeg-<triple>[.exe]` doit exister avant `tauri dev/build` (`fetch-ffmpeg`). À l'exécution : `ffmpeg[.exe]` à côté de `current_exe()`, sinon le PATH.
- **`cookies_for_url`** (webview Tauri) : à n'appeler qu'en contexte **async** sur Windows, sinon deadlock.
- **X** : le `queryId` GraphQL périme régulièrement ; en cas d'erreur 400/404 sur les likes, le re-extraire du bundle JS de x.com.
- **Nommage** : Google `motclé_NNN_LARGExHAUT.ext` (numérotation continue) ; X `user_tweetId_NN` ; Reddit `Titre du post[_NN]`. Caractères interdits nettoyés, 120 car. max, suffixe `_2`, `_3`… en cas de collision. L'extension vient du contenu final (PNG/MP4).
- **Déduplication** à trois niveaux : `key` persistée dans `<app_data_dir>/downloaded.json` (→ `skipped`), URL dans le job, SHA-256 du contenu final dans le job.
- **Sessions** : cookies chiffrés (AES-GCM) sur disque, clé dans le trousseau OS (`keyring`). Les secrets ne passent jamais au frontend (`AccountStatus` = connecté + pseudo).
- **Assets locaux** : le frontend affiche les médias via le protocole `asset:` ; le scope est autorisé dynamiquement sur le dossier de destination.
- Dossiers de sortie : `<destDir>/Google/<motclé>/`, `<destDir>/X/Likes/`, `<destDir>/Reddit/Saved/`.
