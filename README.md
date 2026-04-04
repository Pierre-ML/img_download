# Image Downloader

Application web pour télécharger des images depuis Google Images.
Backend Flask + Selenium, frontend HTML/CSS/JS vanilla avec design glassmorphism.

---

## Stack

| Couche     | Technologie                          |
|------------|--------------------------------------|
| Backend    | Python / Flask                       |
| Scraping   | Selenium + undetected-chromedriver   |
| Download   | requests                             |
| Images     | Pillow                               |
| Frontend   | HTML · CSS · JS vanilla              |

---

## Prérequis

- Python 3.10+
- Google Chrome installé
- Node.js **non requis**

---

## Installation

```bash
pip install flask selenium undetected-chromedriver requests Pillow setuptools
```

> `setuptools` est nécessaire sur Python 3.12+ car `distutils` a été retiré de la stdlib.

---

## Lancement

```bash
python app.py
```

Ouvrir ensuite : [http://localhost:5000](http://localhost:5000)

---

## Utilisation

1. Entrer un **mot-clé** de recherche
2. Choisir le **nombre d'images**
3. Optionnel : renseigner un **dossier de destination** (défaut : `~/Pictures/GoogleImages`)
4. Cliquer sur **Démarrer**

Les images sont sauvegardées dans un sous-dossier portant le nom de la recherche :

```
~/Pictures/GoogleImages/
└── nezuko/
    ├── nezuko_001.jpg
    ├── nezuko_002.png
    └── ...
```

---

## Structure du projet

```
bot_net/
├── app.py                  # Serveur Flask — routes API + SSE
├── worker.py               # Thread de scraping et téléchargement
├── static/
│   ├── index.html          # Page principale
│   ├── css/
│   │   └── style.css       # Styles (dark theme / glassmorphism)
│   └── js/
│       └── app.js          # Logique frontend (SSE, DOM)
├── requirements_downloader.txt
└── README.md
```

---

## Fonctionnalités

- Scraping Google Images via Chrome headful (contourne les protections)
- Tentative de récupération des images haute résolution via le panneau latéral Google
- Déduplication par hash MD5 du contenu (évite les doublons)
- Délais aléatoires entre chaque téléchargement (1 – 3 s)
- Logs en temps réel via SSE (Server-Sent Events)
- Barre de progression live
- Aperçu miniature cliquable dans l'interface
- Gestion silencieuse des erreurs (image corrompue → suivante)
- Nommage automatique : `motcle_001.jpg`, `motcle_002.png`…
- Formats supportés : jpg, png, webp

---

## Notes

- Chrome doit être installé. `undetected-chromedriver` gère le téléchargement du ChromeDriver automatiquement.
- Si le ChromeDriver ne correspond pas à la version de Chrome, passer `version_main=<version>` dans `worker.py` ligne 185.
- Le mode headless est disponible (décommenter `--headless=new` dans `worker.py`).
