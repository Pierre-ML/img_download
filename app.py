"""
Flask backend — Media Downloader (Images · Vidéos · Reddit)
Lancer : python app.py  →  http://localhost:5000
"""

import json
import uuid
import queue as _qmod
import threading
from pathlib import Path
from dataclasses import dataclass
from flask import Flask, request, jsonify, Response, send_file, send_from_directory

from workers.image_worker  import DownloadWorker
from workers.video_worker  import VideoWorker
from workers.reddit_worker import RedditWorker

app = Flask(__name__, static_folder="static", static_url_path="")

# ---------------------------------------------------------------------------
# Job
# ---------------------------------------------------------------------------
@dataclass
class Job:
    id:      str
    type:    str          # "images" | "video" | "reddit"
    label:   str          # mot-clé ou URL courte pour l'affichage
    count:   int
    out_dir: Path | None = None
    status:  str = "running"
    worker:  object = None


# ---------------------------------------------------------------------------
# État global
# ---------------------------------------------------------------------------
_jobs: dict[str, Job] = {}

_pending: list[dict] = []
_pending_lock = threading.Lock()

_current_job: Job | None = None

_subscribers: list[_qmod.Queue] = []
_subs_lock = threading.Lock()

_runner_thread: threading.Thread | None = None


# ---------------------------------------------------------------------------
# SSE helpers
# ---------------------------------------------------------------------------
def _publish(event: dict):
    with _subs_lock:
        for q in _subscribers:
            q.put(event)


def _publish_queue_state():
    with _pending_lock:
        snap = list(_pending)
    _publish({
        "type": "queue_update",
        "current": _job_summary(_current_job),
        "pending": snap,
    })


def _job_summary(job: Job | None) -> dict | None:
    if not job:
        return None
    return {"jobId": job.id, "type": job.type,
            "label": job.label, "count": job.count, "status": job.status}


# ---------------------------------------------------------------------------
# Queue runner
# ---------------------------------------------------------------------------
def _make_worker(task: dict, job: Job):
    t = task["type"]
    cb = _make_cb(job)
    dest = task["dest"]

    if t == "images":
        return DownloadWorker(
            job_id   = job.id,
            keyword  = task["keyword"],
            count    = task["count"],
            dest     = dest,
            event_cb = cb,
        )
    if t == "video":
        return VideoWorker(
            job_id   = job.id,
            urls     = task["urls"],
            dest     = dest,
            event_cb = cb,
        )
    if t == "reddit":
        return RedditWorker(
            job_id   = job.id,
            url      = task["url"],
            count    = task["count"],
            dest     = dest,
            event_cb = cb,
        )
    raise ValueError(f"Type inconnu : {t}")


def _make_cb(job: Job):
    def event_cb(event: dict):
        ev = dict(event)
        ev["jobId"] = job.id
        _publish(ev)
        if ev.get("type") == "done":
            job.status = "done" if ev.get("success") else "error"
            _publish_queue_state()
    return event_cb


def _run_queue():
    global _current_job, _runner_thread

    while True:
        with _pending_lock:
            if not _pending:
                _current_job = None
                _publish_queue_state()
                _runner_thread = None
                return
            task = _pending.pop(0)

        job = Job(id=str(uuid.uuid4()), type=task["type"],
                  label=task["label"], count=task["count"])
        _jobs[job.id] = job
        _current_job  = job
        _publish_queue_state()

        worker = _make_worker(task, job)
        job.worker = worker

        # Exposer out_dir pour les thumbnails après démarrage
        worker.start()
        worker.join()

        # Synchroniser out_dir sur le job
        if hasattr(worker, "out_dir"):
            job.out_dir = worker.out_dir
        if job.status == "running":
            job.status = "done"


# ---------------------------------------------------------------------------
# Pages HTML
# ---------------------------------------------------------------------------
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


# ---------------------------------------------------------------------------
# API — file d'attente
# ---------------------------------------------------------------------------
def _default_dest():
    return str(Path.home() / "Pictures" / "MediaDownloader")


@app.route("/api/queue", methods=["GET"])
def queue_state():
    with _pending_lock:
        snap = list(_pending)
    return jsonify(current=_job_summary(_current_job), pending=snap)


@app.route("/api/queue/add", methods=["POST"])
def queue_add():
    data = request.get_json(force=True)
    t    = data.get("type", "")
    dest = (data.get("dest") or "").strip() or _default_dest()

    # --- Images ---
    if t == "images":
        keyword = (data.get("keyword") or "").strip()
        if not keyword:
            return jsonify(error="Mot-clé manquant"), 400
        try:
            count = int(data.get("count", 20)); assert count > 0
        except Exception:
            return jsonify(error="Nombre invalide"), 400
        task = {"type": "images", "label": keyword,
                "keyword": keyword, "count": count, "dest": dest}

    # --- Vidéos ---
    elif t == "video":
        raw  = data.get("urls") or []
        urls = [u.strip() for u in (raw if isinstance(raw, list)
                else raw.splitlines()) if u.strip()]
        if not urls:
            return jsonify(error="Aucune URL fournie"), 400
        task = {"type": "video", "label": f"{len(urls)} lien(s)",
                "urls": urls, "count": len(urls), "dest": dest}

    # --- Reddit ---
    elif t == "reddit":
        url = (data.get("url") or "").strip()
        if not url:
            return jsonify(error="URL manquante"), 400
        try:
            count = int(data.get("count", 50)); assert count > 0
        except Exception:
            return jsonify(error="Nombre invalide"), 400
        task = {"type": "reddit", "label": url.split("reddit.com")[-1][:40],
                "url": url, "count": count, "dest": dest}

    else:
        return jsonify(error="Type invalide"), 400

    with _pending_lock:
        _pending.append(task)
    _publish_queue_state()
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
    return jsonify(ok=True)


@app.route("/api/queue/<int:idx>", methods=["DELETE"])
def queue_remove(idx: int):
    with _pending_lock:
        if not (0 <= idx < len(_pending)):
            return jsonify(error="Index invalide"), 404
        _pending.pop(idx)
    _publish_queue_state()
    return jsonify(ok=True)


@app.route("/api/stop", methods=["POST"])
def stop():
    if _current_job and _current_job.worker:
        _current_job.worker.stop()
        _current_job.status = "stopped"
    return jsonify(ok=True)


# ---------------------------------------------------------------------------
# SSE global
# ---------------------------------------------------------------------------
@app.route("/api/stream")
def stream():
    q: _qmod.Queue = _qmod.Queue()
    with _subs_lock:
        _subscribers.append(q)

    # État immédiat à la connexion
    with _pending_lock:
        snap = list(_pending)
    q.put({"type": "queue_update",
           "current": _job_summary(_current_job), "pending": snap})

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

    return Response(generate(), mimetype="text/event-stream",
                    headers={"Cache-Control": "no-cache",
                             "X-Accel-Buffering": "no",
                             "Connection": "keep-alive"})


# ---------------------------------------------------------------------------
# Fichiers médias (images + vidéos)
# ---------------------------------------------------------------------------
def _resolve_media(job_id: str, filename: str):
    job = _jobs.get(job_id)
    if not job:
        return None
    # out_dir peut être sur worker ou sur job
    out_dir = job.out_dir
    if out_dir is None and job.worker and hasattr(job.worker, "out_dir"):
        out_dir = job.worker.out_dir
    if not out_dir:
        return None
    out_dir  = out_dir.resolve()
    filepath = (out_dir / filename).resolve()
    if not str(filepath).startswith(str(out_dir)):
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
    except OSError as e:
        return jsonify(error=str(e)), 500
    return jsonify(ok=True)


# ---------------------------------------------------------------------------
if __name__ == "__main__":
    print("Serveur démarré → http://localhost:5000")
    app.run(host="0.0.0.0", port=5000, debug=False, threaded=True)
