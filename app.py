"""
Flask backend — Media Downloader (Images · Vidéos · Reddit)
Lancer : python app.py  →  http://localhost:5000
"""

import json
import logging
import uuid
import queue as _qmod
import threading
from dataclasses import dataclass, field
from pathlib import Path

from flask import Flask, Response, jsonify, request, send_file, send_from_directory

import accounts
import config
from workers.image_worker  import DownloadWorker
from workers.video_worker  import VideoWorker
from workers.reddit_worker import RedditWorker, RedditSavedWorker
from workers.x_worker      import XLikesWorker

# ── Logging ───────────────────────────────────────────────────────────────────
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s  %(levelname)-8s  %(message)s",
    datefmt="%H:%M:%S",
)
log = logging.getLogger(__name__)

# ── App Flask ─────────────────────────────────────────────────────────────────
app = Flask(__name__, static_folder="static", static_url_path="")


# ── Modèle ────────────────────────────────────────────────────────────────────
@dataclass
class Job:
    id:      str
    type:    str           # "images" | "video" | "reddit"
    label:   str           # affiché dans la file d'attente
    count:   int
    status:  str = "running"
    out_dir: Path | None = None
    worker:  object = field(default=None, repr=False)


# ── État global ───────────────────────────────────────────────────────────────
_jobs:         dict[str, Job]        = {}
_pending:      list[dict]            = []
_pending_lock                        = threading.Lock()
_current_job:  Job | None           = None
_subscribers:  list[_qmod.Queue]    = []
_subs_lock                           = threading.Lock()
_runner_thread: threading.Thread | None = None


# ── SSE — publication ─────────────────────────────────────────────────────────
def _publish(event: dict) -> None:
    with _subs_lock:
        for q in _subscribers:
            q.put(event)


def _publish_queue_state() -> None:
    with _pending_lock:
        snap = list(_pending)
    _publish({
        "type":    "queue_update",
        "current": _job_to_dict(_current_job),
        "pending": snap,
    })


def _job_to_dict(job: Job | None) -> dict | None:
    if not job:
        return None
    return {
        "jobId":  job.id,
        "type":   job.type,
        "label":  job.label,
        "count":  job.count,
        "status": job.status,
    }


# ── Queue runner ──────────────────────────────────────────────────────────────
def _build_worker(task: dict, job: Job) -> DownloadWorker | VideoWorker | RedditWorker:
    """Instancie le bon worker selon le type de tâche."""
    cb   = _make_callback(job)
    dest = task["dest"]

    match task["type"]:
        case "images":
            return DownloadWorker(
                job_id=job.id, keyword=task["keyword"],
                count=task["count"], dest=dest, event_cb=cb,
            )
        case "video":
            return VideoWorker(
                job_id=job.id, urls=task["urls"],
                dest=dest, event_cb=cb,
            )
        case "reddit":
            return RedditWorker(
                job_id=job.id, url=task["url"],
                count=task["count"], dest=dest, event_cb=cb,
            )
        case "reddit_saved":
            return RedditSavedWorker(
                job_id=job.id, count=task["count"], dest=dest,
                event_cb=cb, creds=accounts.load()["reddit"],
            )
        case "x_likes":
            return XLikesWorker(
                job_id=job.id, count=task["count"], dest=dest,
                event_cb=cb, creds=accounts.load()["x"],
            )
        case _:
            raise ValueError(f"Type de tâche inconnu : {task['type']!r}")


def _make_callback(job: Job):
    """Retourne le callback SSE associé à un job."""
    def event_cb(event: dict) -> None:
        ev = {**event, "jobId": job.id}
        _publish(ev)
        if ev.get("type") == "done":
            job.status = "done" if ev.get("success") else "error"
            _publish_queue_state()
    return event_cb


def _run_queue() -> None:
    """Boucle principale du runner — s'exécute dans un thread dédié."""
    global _current_job, _runner_thread

    while True:
        with _pending_lock:
            if not _pending:
                _current_job  = None
                _runner_thread = None
                _publish_queue_state()
                return
            task = _pending.pop(0)

        job = Job(id=str(uuid.uuid4()), type=task["type"],
                  label=task["label"], count=task["count"])
        _jobs[job.id] = job
        _current_job  = job
        _publish_queue_state()
        log.info("Job démarré : %s [%s]", job.label, job.type)

        worker     = _build_worker(task, job)
        job.worker = worker
        worker.start()
        worker.join()

        if hasattr(worker, "out_dir"):
            job.out_dir = worker.out_dir
        if job.status == "running":
            job.status = "done"

        log.info("Job terminé : %s — statut=%s", job.label, job.status)


# ── Pages HTML ────────────────────────────────────────────────────────────────
@app.route("/")
def home():
    return send_from_directory("static", "index.html")

@app.route("/images")
def page_images():
    return send_from_directory("static", "images.html")

@app.route("/videos")
def page_videos():
    return send_from_directory("static", "videos.html")

@app.route("/reddit")
def page_reddit():
    return send_from_directory("static", "reddit.html")

@app.route("/x")
def page_x():
    return send_from_directory("static", "x.html")

@app.route("/comptes")
def page_comptes():
    return send_from_directory("static", "comptes.html")


# ── API — comptes ─────────────────────────────────────────────────────────────
@app.route("/api/accounts", methods=["GET"])
def accounts_get():
    return jsonify(accounts.masked())


@app.route("/api/accounts", methods=["POST"])
def accounts_save():
    data = request.get_json(force=True)
    saved = accounts.save(data)
    log.info("Comptes mis à jour")
    return jsonify(accounts.masked(saved))


# ── API — file d'attente ──────────────────────────────────────────────────────
def _parse_count(data: dict, default: int) -> int:
    """Extrait et valide le champ count d'une requête JSON."""
    count = int(data.get("count", default))
    if count < 1:
        raise ValueError("count doit être >= 1")
    return count


def _resolve_dest(data: dict) -> str:
    return (data.get("dest") or "").strip() or config.DEFAULT_DEST


@app.route("/api/queue", methods=["GET"])
def queue_state():
    with _pending_lock:
        snap = list(_pending)
    return jsonify(current=_job_to_dict(_current_job), pending=snap)


@app.route("/api/queue/add", methods=["POST"])
def queue_add():
    data = request.get_json(force=True)
    kind = data.get("type", "")
    dest = _resolve_dest(data)

    try:
        if kind == "images":
            keyword = (data.get("keyword") or "").strip()
            if not keyword:
                return jsonify(error="Mot-clé manquant"), 400
            count = _parse_count(data, default=20)
            task  = {"type": "images", "label": keyword,
                     "keyword": keyword, "count": count, "dest": dest}

        elif kind == "video":
            raw  = data.get("urls") or []
            urls = [u.strip() for u in
                    (raw if isinstance(raw, list) else raw.splitlines())
                    if u.strip()]
            if not urls:
                return jsonify(error="Aucune URL fournie"), 400
            task = {"type": "video", "label": f"{len(urls)} lien(s)",
                    "urls": urls, "count": len(urls), "dest": dest}

        elif kind == "reddit":
            url = (data.get("url") or "").strip()
            if not url:
                return jsonify(error="URL manquante"), 400
            count = _parse_count(data, default=50)
            task  = {"type": "reddit",
                     "label": url.split("reddit.com")[-1][:40],
                     "url": url, "count": count, "dest": dest}

        elif kind == "reddit_saved":
            acc = accounts.load()
            if not accounts.reddit_ready(acc):
                return jsonify(error="Compte Reddit non configuré "
                                     "(page Comptes)"), 400
            count = _parse_count(data, default=50)
            task  = {"type": "reddit_saved",
                     "label": f"u/{acc['reddit']['username']} — enregistrés",
                     "count": count, "dest": dest}

        elif kind == "x_likes":
            acc = accounts.load()
            if not accounts.x_ready(acc):
                return jsonify(error="Compte X non configuré "
                                     "(page Comptes)"), 400
            count = _parse_count(data, default=50)
            task  = {"type": "x_likes",
                     "label": f"@{acc['x']['username']} — likes",
                     "count": count, "dest": dest}

        else:
            return jsonify(error="Type invalide"), 400

    except (ValueError, TypeError) as exc:
        return jsonify(error=str(exc)), 400

    with _pending_lock:
        _pending.append(task)
    _publish_queue_state()
    log.info("Ajouté à la file : %s [%s]", task["label"], kind)
    return jsonify(ok=True)


@app.route("/api/queue/start", methods=["POST"])
def queue_start():
    global _runner_thread
    if _runner_thread and _runner_thread.is_alive():
        return jsonify(ok=True)
    with _pending_lock:
        has_work = bool(_pending) or _current_job is not None
    if not has_work:
        return jsonify(error="File vide"), 400
    _runner_thread = threading.Thread(target=_run_queue, daemon=True)
    _runner_thread.start()
    log.info("Runner démarré")
    return jsonify(ok=True)


@app.route("/api/queue/<int:idx>", methods=["DELETE"])
def queue_remove(idx: int):
    with _pending_lock:
        if not (0 <= idx < len(_pending)):
            return jsonify(error="Index invalide"), 404
        removed = _pending.pop(idx)
    _publish_queue_state()
    log.info("Job retiré de la file : %s", removed.get("label"))
    return jsonify(ok=True)


@app.route("/api/stop", methods=["POST"])
def stop():
    if _current_job and _current_job.worker:
        _current_job.worker.stop()
        _current_job.status = "stopped"
        log.info("Job arrêté : %s", _current_job.label)
    return jsonify(ok=True)


@app.route("/api/reset", methods=["POST"])
def reset_all():
    global _current_job
    if _current_job and _current_job.worker:
        _current_job.worker.stop()
        _current_job.status = "stopped"
    _current_job = None
    with _pending_lock:
        _pending.clear()
    _publish_queue_state()
    log.info("Réinitialisation complète")
    return jsonify(ok=True)


# ── SSE ───────────────────────────────────────────────────────────────────────
@app.route("/api/stream")
def stream():
    q: _qmod.Queue = _qmod.Queue()
    with _subs_lock:
        _subscribers.append(q)

    with _pending_lock:
        snap = list(_pending)
    q.put({"type": "queue_update",
           "current": _job_to_dict(_current_job), "pending": snap})

    def generate():
        try:
            while True:
                try:
                    yield f"data: {json.dumps(q.get(timeout=20))}\n\n"
                except _qmod.Empty:
                    yield ": heartbeat\n\n"
        finally:
            with _subs_lock:
                try:
                    _subscribers.remove(q)
                except ValueError:
                    pass

    return Response(
        generate(),
        mimetype="text/event-stream",
        headers={
            "Cache-Control":    "no-cache",
            "X-Accel-Buffering": "no",
            "Connection":       "keep-alive",
        },
    )


# ── Médias (serve + delete) ───────────────────────────────────────────────────
def _resolve_media(job_id: str, filename: str) -> Path | None:
    """Résout le chemin d'un fichier média de façon sécurisée (path traversal)."""
    job = _jobs.get(job_id)
    if not job:
        return None
    out_dir = job.out_dir or (
        getattr(job.worker, "out_dir", None) if job.worker else None
    )
    if not out_dir:
        return None
    out_dir  = Path(out_dir).resolve()
    filepath = (out_dir / filename).resolve()
    if not filepath.is_relative_to(out_dir):
        return None
    return filepath


@app.route("/api/media/<job_id>/<path:filename>")
def serve_media(job_id: str, filename: str):
    fp = _resolve_media(job_id, filename)
    if not fp or not fp.exists():
        return "", 404
    return send_file(fp)


@app.route("/api/media/<job_id>/<path:filename>", methods=["DELETE"])
def delete_media(job_id: str, filename: str):
    fp = _resolve_media(job_id, filename)
    if not fp:
        return jsonify(error="Introuvable"), 404
    try:
        fp.unlink(missing_ok=True)
    except OSError as exc:
        log.error("Suppression échouée : %s — %s", fp, exc)
        return jsonify(error=str(exc)), 500
    return jsonify(ok=True)


# ── Entrée ────────────────────────────────────────────────────────────────────
if __name__ == "__main__":
    log.info("Serveur démarré → http://%s:%s", config.HOST, config.PORT)
    app.run(host=config.HOST, port=config.PORT, debug=False, threaded=True)
