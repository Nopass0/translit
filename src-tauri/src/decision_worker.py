"""Private, bounded Laya ONNX inference; no Torch or user Python installation."""
import argparse
import hmac
import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path


def load(root, threads):
    """Load the pinned graph once with a bounded CPU thread pool."""
    sys.path.insert(0, str(root / "engine"))
    import numpy as np
    import onnxruntime as ort
    from tokenizers import Tokenizer
    dll_path = root / "engine/onnxruntime/capi"
    if dll_path.exists():
        dll_handle = os.add_dll_directory(str(dll_path))
    tok = Tokenizer.from_file(str(root / "model/tokenizer.json"))
    cfg = json.loads((root / "model/config.json").read_text(encoding="utf-8"))
    opts = ort.SessionOptions()
    opts.intra_op_num_threads = threads
    opts.inter_op_num_threads = 1
    opts.enable_mem_pattern = False
    session = ort.InferenceSession(str(root / "model/onnx/model_fp16.onnx"), opts,
                                  providers=["CPUExecutionProvider"])

    def choose(state, question, choices, limit):
        """Match upstream choice sequence construction, preserving neutral option labels."""
        _ = dll_handle if dll_path.exists() else None
        clean = lambda text: str(text).replace("[MASK]", " ").replace("<mask>", " ")
        encode = lambda text: tok.encode(clean(text), add_special_tokens=False).ids
        options = [[cfg["mask_token_id"]] + encode(" " + chr(65 + i) + ": " + text)[:48]
                   for i, text in enumerate(choices)]
        budget = 192 - sum(map(len, options))
        if budget < 16:
            per = max(4, (192 - 16) // len(options))
            options = [o[:per] for o in options]
            budget = 192 - sum(map(len, options))
        head = encode("choice question: " + question)[:max(8, budget)]
        ids = [cfg["cls_token_id"]] + head + [cfg["sep_token_id"]]
        markers = []
        for option in options:
            markers.append(len(ids))
            ids.extend(option)
        ids.append(cfg["sep_token_id"])
        state_ids = encode(state)
        room = max(0, limit - len(ids) - 1)
        ids += state_ids[:room] + [cfg["sep_token_id"]]
        feeds = {"input_ids": np.array([ids], dtype=np.int64),
                 "attention_mask": np.ones((1, len(ids)), dtype=np.int64),
                 "marker_pos": np.array([markers], dtype=np.int64),
                 "marker_mask": np.ones((1, len(markers)), dtype=np.bool_),
                 "qtype": np.array([0], dtype=np.int64)}
        logits = session.run(["logits"], feeds)[0][0][:len(choices)].astype(np.float64)
        probabilities = np.exp(logits - logits.max())
        probabilities /= probabilities.sum()
        index = int(probabilities.argmax())
        return {"choice": choices[index], "confidence": float(probabilities[index]),
                "probabilities": probabilities.tolist(), "truncated": len(state_ids) > room}

    return choose


def serve(root, port, threads):
    """Serve only authenticated loopback requests, serializing inference."""
    choose = load(root, threads)
    token = os.environ["TRANSLIT_WORKER_TOKEN"]

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            """Keep dialogue and authentication out of access logs."""

        def reply(self, code, value):
            """Return one UTF-8 JSON response."""
            data = json.dumps(value, ensure_ascii=False).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def authorized(self):
            """Authenticate every endpoint with the random process token."""
            return hmac.compare_digest(self.headers.get("Authorization", ""), "Bearer " + token)

        def do_GET(self):
            """Health becomes available only after weights and tokenizer have loaded."""
            self.reply(200 if self.authorized() and self.path == "/health" else 403,
                       {"ready": True})

        def do_POST(self):
            """Evaluate at most six short choice questions in one request."""
            if not self.authorized() or self.path != "/decide":
                return self.reply(403, {"error": "Unauthorized"})
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= 65536:
                    raise ValueError("Request too large")
                body = json.loads(self.rfile.read(length))
                state = body["state"]
                if not isinstance(state, str) or len(state) > 6000:
                    raise ValueError("Invalid state")
                questions = body["questions"]
                if not isinstance(questions, dict) or not 1 <= len(questions) <= 6:
                    raise ValueError("Invalid questions")
                limit = max(256, min(1024, int(body.get("tokens", 512))))
                start = time.perf_counter()
                answers = {}
                for key, question in questions.items():
                    choices = question["choices"]
                    if not isinstance(choices, list) or not 2 <= len(choices) <= 8 or any(
                            not isinstance(c, str) or not c.strip() or len(c) > 300 for c in choices):
                        raise ValueError("Invalid choices")
                    answers[key] = choose(state, str(question["question"])[:500], choices, limit)
                self.reply(200, {"answers": answers, "elapsed_ms": round((time.perf_counter() - start) * 1000)})
            except (ValueError, KeyError, TypeError):
                self.reply(400, {"error": "Invalid decision request"})
            except Exception:
                self.reply(500, {"error": "Decision inference failed"})

    HTTPServer(("127.0.0.1", port), Handler).serve_forever()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--threads", type=int, default=2)
    args = parser.parse_args()
    serve(args.root, args.port, max(1, min(8, args.threads)))
