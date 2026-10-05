"""Private, bounded CPU translation worker shipped by Translit (no system Python)."""
import argparse
import json
import os
import re
from pathlib import Path
from http.server import BaseHTTPRequestHandler, HTTPServer

os.environ.setdefault("OMP_NUM_THREADS", "2")
os.environ.setdefault("OPENBLAS_NUM_THREADS", "1")
import ctranslate2
import sentencepiece
from grammar_worker import Grammar


class Translator:
    """Translate the sentence and align selected source tokens to its Russian words."""

    def __init__(self, directory, threads=2, nlp_only=False):
        self.grammar=Grammar()
        if nlp_only:
            self.engine=None
            return
        self.source = sentencepiece.SentencePieceProcessor(model_file=str(directory / "source.spm"))
        self.target = sentencepiece.SentencePieceProcessor(model_file=str(directory / "target.spm"))
        self.engine = ctranslate2.Translator(str(directory), device="cpu", compute_type="int8", inter_threads=1, intra_threads=threads, max_queued_batches=1)

    def translate(self, selection, context):
        """Return standalone fallback plus attention alignment from the current sentence."""
        if self.engine is None:raise ValueError('Translation model is unloaded')
        sentence = context.strip() or selection
        speaker=''
        if '\n' in sentence:
            first,rest=sentence.split('\n',1)
            if len(first)<80 and first.split() and len(first.split())<=3:
                labels=self.grammar.analyze(first)['tokens']
                if first.endswith(':') or (labels and all(t['pos']=='PROPN' for t in labels)):
                    speaker=first.rstrip(':')+': '
                    sentence=rest.strip()
        # Keep real dialogue boundaries while joining visual wraps within one sentence.
        lines=[]
        for raw in sentence.splitlines():
            line=' '.join(raw.split())
            if not line:continue
            speaker_label=bool(re.match(r"^[A-Za-z][A-Za-z .'-]{0,40}:\s",line))
            terminal=bool(lines and re.search(r'[.!?][\"\'”)]*$',lines[-1]))
            if lines and not speaker_label and not terminal:lines[-1]+=' '+line
            else:lines.append(line)
        selection=' '.join(selection.split())
        if len(lines)>1:
            translated=[self.translate(line,line) for line in lines]
            whole='\n'.join(item['context_translation'] for item in translated)
            index=next((i for i,line in enumerate(lines) if selection.lower() in line.lower()),None)
            selected=self.translate(selection,lines[index]) if index is not None else self.translate(selection,selection)
            if selection.lower()==' '.join(sentence.split()).lower():selected['translation']=whole
            return {**selected,'context_translation':speaker+whole}
        sentence=lines[0] if lines else selection
        # Normalize a few conversational affirmations which the tiny MT model transliterates.
        sentence=re.sub(r'^(Yup|Yep|Yeah)\b','Yes',sentence,flags=re.IGNORECASE)
        if selection.strip().rstrip('!.?').lower() in ('yup','yep','yeah'):
            selection='Yes'
        pieces = self.source.encode_as_immutable_proto(sentence).pieces
        result = self.engine.translate_batch([[p.piece for p in pieces] + ["</s>"]], beam_size=2, max_input_length=384, max_decoding_length=192, return_attention=True)[0]
        tokens = result.hypotheses[0]
        whole = self.target.decode(tokens)
        begin = sentence.lower().find(selection.strip().lower())
        end = begin + len(selection.strip())
        source_indices = {i for i, p in enumerate(pieces) if p.begin < end and p.end > begin} if begin >= 0 else set()
        selected = []
        for i, row in enumerate(result.attention[0] if result.attention else []):
            if i >= len(tokens) or not row:
                continue
            strongest = max(range(len(row)), key=row.__getitem__)
            if strongest in source_indices and sum(row[j] for j in source_indices if j < len(row)) >= .30:
                selected.append(i)
        aligned = ""
        if selected:
            first, last = min(selected), max(selected)
            while first > 0 and not tokens[first].startswith("▁"):
                first -= 1
            while last + 1 < len(tokens) and not tokens[last + 1].startswith("▁"):
                last += 1
            aligned = self.target.decode(tokens[first:last + 1]).strip()
        if selection.strip().lower().rstrip(".!?") == sentence.lower().rstrip(".!?"):
            aligned = whole
        if not aligned:
            fallback = self.engine.translate_batch([self.source.encode(selection, out_type=str) + ["</s>"]], beam_size=2, max_input_length=192, max_decoding_length=96)[0]
            aligned = self.target.decode(fallback.hypotheses[0])
        return {"translation": aligned, "context_translation": speaker+whole, "aligned": bool(selected)}


def serve(directory, port, token, threads, nlp_only=False):
    """Serve authenticated loopback requests serially, limiting body size and decode length."""
    translator = Translator(directory, threads, nlp_only)

    class Handler(BaseHTTPRequestHandler):
        """Reject unauthenticated requests; never log tokens or dialogue."""
        def log_message(self, *args):
            pass

        def answer(self, status, body):
            """Send one bounded UTF-8 JSON response without CORS headers."""
            data = json.dumps(body, ensure_ascii=False).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def authorized(self):
            """Check the per-process bearer token before health or inference."""
            return self.headers.get("Authorization", "") == "Bearer " + token

        def do_GET(self):
            """Report health only after the model has loaded."""
            self.answer(200 if self.authorized() and self.path == "/health" else 403, {"ready": True})

        def do_POST(self):
            """Run a single translation; reject excessive inputs before tokenizing."""
            if not self.authorized() or self.path not in ('/translate','/analyze'):
                return self.answer(403, {"error": "Unauthorized"})
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= 65536:
                    return self.answer(413, {"error": "Input too large"})
                body = json.loads(self.rfile.read(length))
                if self.path == '/analyze':
                    context=body.get('context','')
                    if not isinstance(context,str) or len(context)>12000:return self.answer(400,{'error':'Invalid text'})
                    return self.answer(200,translator.grammar.analyze(context))
                selection, context = body["selection"], body.get("context", "")
                if not isinstance(selection, str) or not isinstance(context, str) or not selection.strip() or len(selection) > 3000 or len(context) > 12000:
                    return self.answer(400, {"error": "Invalid text"})
                self.answer(200, translator.translate(selection, context))
            except (ValueError, KeyError, TypeError):
                self.answer(400, {"error": "Invalid request"})
            except Exception as error:
                print(type(error).__name__, flush=True)
                self.answer(500, {"error": "Inference failed"})

    print(f"OPUS-MT int8 ready; CPU threads={threads}", flush=True)
    HTTPServer(("127.0.0.1", port), Handler).serve_forever()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--threads", type=int, default=2)
    parser.add_argument('--nlp-only',action='store_true')
    args = parser.parse_args()
    serve(args.root, args.port, os.environ.pop("TRANSLIT_WORKER_TOKEN"), max(1,min(args.threads,64)),args.nlp_only)
