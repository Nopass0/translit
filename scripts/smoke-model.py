"""Check the selected local GGUF preset with real authenticated inference and cleanup."""
import json
import argparse
import os
from pathlib import Path
import secrets
import socket
import subprocess
import time
import urllib.error
import urllib.request

root = Path(os.environ["APPDATA"]) / "local.translit.game-dictionary/models-v2"
parser=argparse.ArgumentParser()
parser.add_argument("--model",type=Path,default=root/"qwen35-2b/model.gguf")
parser.add_argument("--name",default="Qwen3.5 2B Q4_K_M")
args=parser.parse_args()
project = Path(__file__).resolve().parent.parent
key = root / "probe-key.txt"
token = secrets.token_hex(32)
key.write_text(token)
with socket.socket() as port_socket:
    port_socket.bind(("127.0.0.1", 0))
    port = port_socket.getsockname()[1]
log = (project / "artifacts/grammar-probe.log").open("w")
child = subprocess.Popen([str(root / "engine-vulkan/llama-server.exe"), "--model", str(args.model), "--host", "127.0.0.1", "--port", str(port), "--ctx-size", "4096", "--parallel", "1", "--threads", "8", "--threads-batch", "8", "--n-gpu-layers", "99", "--batch-size", "256", "--ubatch-size", "128", "--no-webui", "--reasoning", "off", "--api-key-file", str(key)], stdout=log, stderr=log, creationflags=0x08004000)


def request(path, body=None):
    """Call only this owned loopback worker, keeping its random token out of output."""
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=data, headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=80) as response:
        return json.load(response)


try:
    started = time.perf_counter()
    for _ in range(600):
        if child.poll() is not None:
            raise RuntimeError("Owned model process exited; inspect artifacts/grammar-probe.log")
        try:
            request("/health")
            break
        except (urllib.error.URLError, TimeoutError):
            time.sleep(.1)
    else:
        raise RuntimeError("Model startup timed out")
    load_seconds = time.perf_counter() - started
    started = time.perf_counter()
    output = request("/v1/chat/completions", {"model": "local", "temperature": .2, "max_tokens": 650, "chat_template_kwargs": {"enable_thinking": False}, "response_format": {"type": "json_object"}, "messages": [{"role": "system", "content": "You teach English through game dialogue. Translate into natural Russian. Use the verified dictionary_hint when present. butter me up means льстить or пытаться задобрить. Do not invent events or conditions. Analyze selected phrase only: me is an object, up is a particle in a separable phrasal verb. Give concise grammar in Russian. examples must contain at most two strings each with English and Russian, alternatives must be Russian synonyms. Reply ONLY JSON with fields translation, context_translation, phrase, explanation, construction, grammar, examples (array of English sentences with Russian translations), alternatives (array), idiom (boolean)."}, {"role": "user", "content": json.dumps({"selection": "butter me up", "current_sentence": "Stop trying to butter me up. I won't give you the key.", "previous_dialogue": [], "game": "owned translation probe", "dictionary_hint": {"phrase":"butter me up","meaning":"льстить; пытаться задобрить","grammar":"butter — глагол, me — местоимение-дополнение, up — частица. Разделяемый фразовый глагол: местоимение между глаголом и частицей."}})}]})
    result = {"load_seconds": load_seconds, "translation_seconds": time.perf_counter() - started, "model": args.name, "device": "Vulkan", "content": output["choices"][0]["message"]["content"], "usage": output.get("usage", {})}
    (project / "artifacts/model-smoke.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False))
finally:
    child.terminate()
    child.wait(timeout=10)
    log.close()
    key.unlink(missing_ok=True)
