"""
accounts.py — stockage local des comptes (Reddit, X).
Les identifiants vivent dans accounts.json (gitignoré), jamais dans le code.
Les secrets ne sont jamais renvoyés au frontend : voir masked().
"""

import json
import threading

import config

_DEFAULTS: dict = {
    "reddit": {
        "client_id":     "",   # app "script" créée sur reddit.com/prefs/apps
        "client_secret": "",
        "username":      "",
        "password":      "",
    },
    "x": {
        "username":     "",    # pseudo X sans le @
        "browser":      "firefox",  # navigateur où la session X est ouverte
        "cookies_file": "",    # alternative : chemin d'un cookies.txt exporté
    },
}
_SECRET_KEYS = {"client_secret", "password"}
_lock = threading.Lock()


def load() -> dict:
    """Retourne les comptes, complétés par les valeurs par défaut."""
    with _lock:
        try:
            data = json.loads(config.ACCOUNTS_FILE.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            data = {}
    merged = {}
    for service, defaults in _DEFAULTS.items():
        merged[service] = {**defaults, **(data.get(service) or {})}
    return merged


def save(update: dict) -> dict:
    """Fusionne `update` dans accounts.json.
    Un champ secret envoyé vide est ignoré (on garde la valeur existante)."""
    current = load()
    for service, fields in (update or {}).items():
        if service not in _DEFAULTS or not isinstance(fields, dict):
            continue
        for key, value in fields.items():
            if key not in _DEFAULTS[service]:
                continue
            value = str(value or "").strip()
            if key in _SECRET_KEYS and not value:
                continue  # ne pas écraser un secret déjà enregistré
            if key == "username":
                value = value.lstrip("@").removeprefix("u/")
            current[service][key] = value
    with _lock:
        config.ACCOUNTS_FILE.write_text(
            json.dumps(current, indent=2, ensure_ascii=False),
            encoding="utf-8",
        )
    return current


def masked(data: dict | None = None) -> dict:
    """Version sans secrets pour le frontend : les champs secrets deviennent
    un booléen `<champ>_set`."""
    data = data or load()
    out: dict = {}
    for service, fields in data.items():
        out[service] = {}
        for key, value in fields.items():
            if key in _SECRET_KEYS:
                out[service][f"{key}_set"] = bool(value)
            else:
                out[service][key] = value
    return out


def reddit_ready(data: dict | None = None) -> bool:
    acc = (data or load())["reddit"]
    return all(acc.get(k) for k in ("client_id", "client_secret",
                                    "username", "password"))


def x_ready(data: dict | None = None) -> bool:
    return bool((data or load())["x"].get("username"))
