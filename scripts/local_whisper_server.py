"""Minimal OpenAI-compatible transcription server on top of faster-whisper.

    pip install faster-whisper
    python scripts/local_whisper_server.py --model large-v3 --device cuda --port 8000

Then in OwnVoice Meet: Settings > 本機 Whisper, server http://localhost:8000/v1.
It implements only POST /v1/audio/transcriptions (multipart: file, model, language,
response_format=json), which is all OwnVoice Meet sends. For a fuller server, see docs/LOCAL.md.
Standard library only, apart from faster-whisper; audio never leaves this machine.
"""
import argparse
import io
import json
import threading
import wave
from email.parser import BytesParser
from email.policy import default as email_policy
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import numpy as np
from faster_whisper import WhisperModel

MODEL = None
LOCK = threading.Lock()  # one transcription at a time per GPU


def parse_multipart(content_type: str, body: bytes) -> dict:
    msg = BytesParser(policy=email_policy).parsebytes(
        b"Content-Type: " + content_type.encode() + b"\r\n\r\n" + body
    )
    fields = {}
    for part in msg.iter_parts():
        name = part.get_param("name", header="content-disposition")
        fields[name] = part.get_payload(decode=True)
    return fields


def read_wav(data: bytes) -> np.ndarray:
    with wave.open(io.BytesIO(data)) as w:
        if w.getsampwidth() != 2 or w.getnchannels() != 1:
            raise ValueError("expected 16-bit mono WAV")
        pcm = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16)
        rate = w.getframerate()
    audio = pcm.astype(np.float32) / 32768.0
    if rate != 16000:  # faster-whisper expects 16 kHz
        n = int(len(audio) * 16000 / rate)
        audio = np.interp(np.linspace(0, len(audio), n, endpoint=False), np.arange(len(audio)), audio).astype(np.float32)
    return audio


class Handler(BaseHTTPRequestHandler):
    def _send(self, code: int, obj: dict) -> None:
        body = json.dumps(obj, ensure_ascii=False).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):  # noqa: N802 (http.server naming)
        if self.path.rstrip("/") != "/v1/audio/transcriptions":
            return self._send(404, {"error": {"message": f"unknown path {self.path}"}})
        try:
            body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
            fields = parse_multipart(self.headers.get("Content-Type", ""), body)
            audio = read_wav(fields["file"])
            language = (fields.get("language") or b"").decode().strip() or None
            with LOCK:
                segments, _ = MODEL.transcribe(audio, language=language, beam_size=5, vad_filter=True,
                                               condition_on_previous_text=False)
                text = " ".join(s.text.strip() for s in segments if s.no_speech_prob < 0.6).strip()
            self._send(200, {"text": text})
        except Exception as e:  # noqa: BLE001 (report any failure to the client)
            self._send(400, {"error": {"message": str(e)}})

    def log_message(self, fmt, *args):
        print(f"[whisper] {self.address_string()} {fmt % args}", flush=True)


def main():
    global MODEL
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--model", default="large-v3")
    ap.add_argument("--device", default="cuda")
    ap.add_argument("--compute-type", default="float16")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=8000)
    a = ap.parse_args()
    print(f"[whisper] loading {a.model} on {a.device}…", flush=True)
    MODEL = WhisperModel(a.model, device=a.device, compute_type=a.compute_type)
    server = ThreadingHTTPServer((a.host, a.port), Handler)  # binds; fails here if the port is taken
    print(f"[whisper] ready on http://{a.host}:{a.port}/v1", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
