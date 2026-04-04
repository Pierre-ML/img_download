"""
Flask backend — Google Images Downloader
Lancer : python app.py
Ouvrir  : http://localhost:5000
"""

import json
import uuid
import queue
from pathlib import Path
from dataclasses import dataclass, field
from flask import Flask, request, jsonify, Response, send_file, send_from_directory

from worker import DownloadWorker

app = Flask(__name__, static_folder="static", static_url_path="")


# ---------------------------------------------------------------------------
# Gestion des jobs
# ---------------------------------------------------------------------------
@dataclass
class Job:
    id:      str
    keyword: str
    out_dir: Path | None = None
    status:  str = "running"          # running | done | error | stopped
    queue:   queue.Queue = field(default_factory=queue.Queue)
    worker:  DownloadWorker | None = None


_jobs: dict[str, Job] = {}


def _get_job(job_id: str) -> Job | None:
    return _jobs.get(job_id)


# ---------------------------------------------------------------------------
# Routes statiques
# ---------------------------------------------------------------------------
@app.route("/")
def index():
    return send_from_directory("static", "index.html")


# ---------------------------------------------------------------------------
# API
# ---------------------------------------------------------------------------
@app.route("/api/start", methods=["POST"])
def start():
    data    = request.get_json(force=True)
    keyword = (data.get("keyword") or "").strip()
    dest    = (data.get("dest") or "").strip()
    try:
        count = int(data.get("count", 20))
        assert count > 0
    except (ValueError, AssertionError):
        return jsonify(error="Nombre d'images invalide"), 400

    if not keyword:
        return jsonify(error="Mot-clé manquant"), 400
    if not dest:
        dest = str(Path.home() / "Pictures" / "GoogleImages")

    job_id = str(uuid.uuid4())
    job    = Job(id=job_id, keyword=keyword)
    _jobs[job_id] = job

    def event_cb(event: dict):
        job.queue.put(event)
        if event.get("type") == "done":
            job.status = "done" if event.get("success") else "error"

    worker = DownloadWorker(
        job_id    = job_id,
        keyword   = keyword,
        count     = count,
        dest      = dest,
        event_cb  = event_cb,
    )
    job.worker = worker
    worker.start()

    return jsonify(jobId=job_id)


@app.route("/api/stop/<job_id>", methods=["POST"])
def stop(job_id: str):
    job = _get_job(job_id)
    if not job:
        return jsonify(error="Job introuvable"), 404
    if job.worker:
        job.worker.stop()
    job.status = "stopped"
    return jsonify(ok=True)


@app.route("/api/stream/<job_id>")
def stream(job_id: str):
    job = _get_job(job_id)
    if not job:
        return Response("data: {\"type\":\"error\",\"text\":\"Job introuvable\"}\n\n",
                        mimetype="text/event-stream")

    def generate():
        while job.status == "running":
            try:
                event = job.queue.get(timeout=1)
                yield f"data: {json.dumps(event)}\n\n"
            except queue.Empty:
                yield ": heartbeat\n\n"
        # Vider la queue après la fin
        while not job.queue.empty():
            try:
                yield f"data: {json.dumps(job.queue.get_nowait())}\n\n"
            except queue.Empty:
                break

    return Response(
        generate(),
        mimetype="text/event-stream",
        headers={
            "Cache-Control":     "no-cache",
            "X-Accel-Buffering": "no",
            "Connection":        "keep-alive",
        },
    )


@app.route("/api/thumb/<job_id>/<path:filename>")
def serve_thumb(job_id: str, filename: str):
    job = _get_job(job_id)
    if not job or not job.worker or not job.worker.out_dir:
        return "", 404

    out_dir  = job.worker.out_dir.resolve()
    filepath = (out_dir / filename).resolve()

    # Protection path traversal
    if not str(filepath).startswith(str(out_dir)):
        return "", 403

    if not filepath.exists():
        return "", 404

    return send_file(filepath)


# ---------------------------------------------------------------------------
if __name__ == "__main__":
    print("Serveur démarré → http://localhost:5000")
    app.run(host="0.0.0.0", port=5000, debug=False, threaded=True)
