"""Serve the current UI against live AMS reads and a local publishing test double.

Run: python scripts/verify_manual_publish_ams.py --output <evidence-directory>
Change response.json in the output directory to select success, business-error,
http-error or malformed. requests.jsonl records every publishing request.
No publishing request is forwarded to the real data center.
"""
import argparse
import json
import time
import urllib.error
import urllib.request
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=18222)
    parser.add_argument("--model-url", default="http://127.0.0.1:8022")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    mode_file = args.output / "response.json"
    if not mode_file.exists():
        mode_file.write_text(json.dumps({"mode": "success"}), encoding="utf-8")
    web = Path(__file__).resolve().parents[1] / "web" / "public"

    class Handler(SimpleHTTPRequestHandler):
        def __init__(self, *a, **kw):
            super().__init__(*a, directory=str(web), **kw)

        def respond(self, status, body, content_type="application/json; charset=utf-8"):
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def proxy_read(self):
            body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            req = urllib.request.Request(args.model_url + self.path,
                                         data=body if self.command == "POST" else None,
                                         headers={"Content-Type": "application/json"},
                                         method=self.command)
            try:
                response = urllib.request.urlopen(req, timeout=30)
            except urllib.error.HTTPError as error:
                response = error
            self.respond(response.status, response.read(),
                         response.headers.get("Content-Type", "application/json"))

        def do_GET(self):
            if self.path.startswith("/api/"):
                return self.proxy_read()
            if self.path == "/config.json":
                return self.respond(200, json.dumps({
                    "model_api_url": f"http://127.0.0.1:{args.port}",
                    "data_api_url": f"http://127.0.0.1:{args.port}",
                }).encode())
            return super().do_GET()

        def do_POST(self):
            if self.path != "/send_room_code_to_data_center":
                # Only read-only query POSTs may reach AMS through this test harness.
                if self.path == "/api/v1/query":
                    return self.proxy_read()
                return self.respond(403, b'{"error":"test harness blocks model writes"}')
            raw = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            settings = json.loads(mode_file.read_text(encoding="utf-8"))
            payload = json.loads(raw)
            with (args.output / "requests.jsonl").open("a", encoding="utf-8") as out:
                out.write(json.dumps({"path": self.path, "content_type": self.headers.get("Content-Type"),
                                      "mode": settings["mode"], "payload": payload}, ensure_ascii=False) + "\n")
            time.sleep(settings.get("delay", 0))
            mode = settings["mode"]
            if mode == "http-error":
                return self.respond(503, b'{"error":"AMS local test HTTP failure"}')
            if mode == "malformed":
                return self.respond(200, b'not-json')
            response = {"Success": mode == "success",
                        "Result": "AMS local test accepted" if mode == "success" else "AMS local test rejected",
                        "KeyValue": "", "LoginUrl": ""}
            return self.respond(200, json.dumps(response).encode())

    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"AMS UI validation: http://127.0.0.1:{args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
