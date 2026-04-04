"""
Flask backend — Google Images Downloader
Lancer : python app.py
Ouvrir  : http://localhost:5000
"""

import json
import uuid
import queue as _qmod
import threading
from pathlib import Path
from dataclasses import dataclass
from flask import Flask, request, jsonify, Response, send_file, send_from_directory

from worker import DownloadWorker

app = Flask(__name__, static_folder="static", static_url_path="")


# ---------------------------------------------------------------------------
# Job dataclass
# ---------------------------------------------------------------------------
@dataclass
class Job:
    id:      str
    keyword: str
    count:   int
    out_dir: Path | None = None
    status:  str = "running"       # running | done | error | stopped
    worker:  DownloadWorker | None = None


# ---------------------------------------------------------------------------
# État global
# ---------------------------------------------------------------------------
_jobs: dict[str, Job] = {}

_pending: list[dict] = []          # [{keyword, count, dest}, ...]
_pending_lock = threading.Lock()

_current_job: Job | None = None

# SSE fan-out : liste de queues, une par client connecté
_subscribers: list[_qmod.Queue] = []
_subs_lock = threading.Lock()

_runner_thread: threading.Thread | None = None


# ---------------------------------------------------------------------------
# SSE helpers
# ---------------------------------------------------------------------------
def _publish(event: dict):
    """Envoie un événement à tous les clients SSE connectés."""
    with _subs_lock:
        for q in _subscribers:
            q.put(event)


def _publish_queue_state():
    with _pending_lock:
        pending_snapshot = list(_pending)
    _publish({
        "type": "queue_update",
        "current": {
            "jobId":   _current_job.id,
            "keyword": _current_job.keyword,
            "count":   _current_job.count,
            "status":  _current_job.status,
        } if _current_job else None,
        "pending": pending_snapshot,
    })


# ---------------------------------------------------------------------------
# Queue runner (thread unique)
# ---------------------------------------------------------------------------
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

        job_id = str(uuid.uuid4())
        job = Job(id=job_id, keyword=task["keyword"], count=task["count"])
        _jobs[job_id] = job
        _current_job = job
        _publish_queue_state()

        def make_cb(j: Job):
            def event_cb(event: dict):
                ev = dict(event)
                ev["jobId"] = j.id
                _publish(ev)
                if ev.get("type") == "done":
                    j.status = "done" if ev.get("success") else "error"
                    _publish_queue_state()
            return event_cb

        worker = DownloadWorker(
            job_id   = job_id,
            keyword  = task["keyword"],
            count    = task["count"],
            dest     = task["dest"],
            event_cb = make_cb(job),
        )
        job.worker = worker
        worker.start()
        worker.join()   # attend la fin du job avant de passer au suivant

        if job.status == "running":
            job.status = "done"


# ---------------------------------------------------------------------------
# Routes statiques
# ---------------------------------------------------------------------------
@app.route("/")
def index():
    return send_from_directory("static", "index.html")


# ---------------------------------------------------------------------------
# API — file d'attente
# ---------------------------------------------------------------------------
@app.route("/api/queue", methods=["GET"])
def queue_state():
    with _pending_lock:
        pending = list(_pending)
    return jsonify(
        current={
            "jobId":   _current_job.id,
            "keyword": _current_job.keyword,
            "count":   _current_job.count,
            "status":  _current_job.status,
        } if _current_job else None,
        pending=pending,
    )


@app.route("/api/queue/add", methods=["POST"])
def queue_add():
    global _runner_thread

    data    = request.get_json(force=True)
    keyword = (data.get("keyword") or "").strip()
    dest    = (data.get("dest")    or "").strip()
    try:
        count = int(data.get("count", 20))
        assert count > 0
    except (ValueError, AssertionError):
        return jsonify(error="Nombre d'images invalide"), 400

    if not keyword:
        return jsonify(error="Mot-clé manquant"), 400
    if not dest:
        dest = str(Path.home() / "Pictures" / "GoogleImages")

    with _pending_lock:
        _pending.append({"keyword": keyword, "count": count, "dest": dest})

    _publish_queue_state()
    return jsonify(ok=True)


@app.route("/api/queue/start", methods=["POST"])
def queue_start():
    global _runner_thread
    if _runner_thread is not None and _runner_thread.is_alive():
        return jsonify(ok=True)   # déjà en cours
    with _pending_lock:
        if not _pending and _current_job is None:
            return jsonify(error="File vide"), 400
    _runner_thread = threading.Thread(target=_run_queue, daemon=True)
    _runner_thread.start()
    return jsonify(ok=True)


@app.route("/api/queue/<int:idx>", methods=["DELETE"])
def queue_remove(idx: int):
    with _pending_lock:
        if idx < 0 or idx >= len(_pending):
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

    # Envoyer l'état courant immédiatement à la connexion
    with _pending_lock:
        pending_snapshot = list(_pending)
    q.put({
        "type": "queue_update",
        "current": {
            "jobId":   _current_job.id,
            "keyword": _current_job.keyword,
            "count":   _current_job.count,
            "status":  _current_job.status,
        } if _current_job else None,
        "pending": pending_snapshot,
    })

    def generate():
        try:
            while True:
                try:
                    event = q.get(timeout=15)
                    yield f"data: {json.dumps(event)}\n\n"
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
            "Cache-Control":     "no-cache",
            "X-Accel-Buffering": "no",
            "Connection":        "keep-alive",
        },
    )


# ---------------------------------------------------------------------------
# Thumbnails
# ---------------------------------------------------------------------------
def _resolve_thumb(job_id: str, filename: str):
    job = _jobs.get(job_id)
    if not job or not job.worker or not job.worker.out_dir:
        return None, None
    out_dir  = job.worker.out_dir.resolve()
    filepath = (out_dir / filename).resolve()
    if not str(filepath).startswith(str(out_dir)):
        return None, None
    return filepath, out_dir


@app.route("/api/thumb/<job_id>/<path:filename>")
def serve_thumb(job_id: str, filename: str):
    filepath, _ = _resolve_thumb(job_id, filename)
    if filepath is None:
        return "", 404
    if not filepath.exists():
        return "", 404
    return send_file(filepath)


@app.route("/api/thumb/<job_id>/<path:filename>", methods=["DELETE"])
def delete_thumb(job_id: str, filename: str):
    filepath, _ = _resolve_thumb(job_id, filename)
    if filepath is None:
        return jsonify(error="Introuvable"), 404
    try:
        filepath.unlink(missing_ok=True)
    except OSError as e:
        return jsonify(error=str(e)), 500
    return jsonify(ok=True)


# ---------------------------------------------------------------------------
if __name__ == "__main__":
    print("Serveur démarré → http://localhost:5000")
    app.run(host="0.0.0.0", port=5000, debug=False, threaded=True)
