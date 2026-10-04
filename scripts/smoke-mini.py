"""Verify bundled CPU translation, multiline context and grammar using an owned worker."""
import json,os,secrets,socket,subprocess,time,urllib.request,urllib.error
from pathlib import Path
import shutil
project=Path(__file__).resolve().parent.parent
root=Path(os.environ['APPDATA'])/'local.translit.game-dictionary/models-v2/opus'
for name in ('mini_server.py','grammar_worker.py'):
    shutil.copyfile(project/'src-tauri/src'/name,root/'engine'/name)
token=secrets.token_hex(32)
with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
env=dict(os.environ,TRANSLIT_WORKER_TOKEN=token)
log=(project/'artifacts/mini-smoke.log').open('w')
child=subprocess.Popen([str(root/'engine/python.exe'),'-I','-X','utf8',str(root/'engine/mini_server.py'),'--root',str(root/'model'),'--port',str(port),'--threads','2'],env=env,stdout=log,stderr=log,creationflags=0x08004000)
def request(path,body=None):
    data=None if body is None else json.dumps(body).encode()
    req=urllib.request.Request(f'http://127.0.0.1:{port}{path}',data=data,headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'})
    with urllib.request.urlopen(req,timeout=30) as r:return json.load(r)
try:
    started=time.perf_counter()
    for _ in range(300):
        if child.poll() is not None:raise RuntimeError('Owned worker exited')
        try:request('/health');break
        except (urllib.error.URLError,TimeoutError):time.sleep(.1)
    else:raise RuntimeError('Startup timed out')
    loaded=time.perf_counter()-started
    context='Estelle: We should investigate the mysterious ruins.\nJoshua: Remember to protect your companions.\nEvery journey begins with a single step.'
    started=time.perf_counter();translated=request('/translate',{'selection':'protect your companions','context':context});elapsed=time.perf_counter()-started
    assert len(translated['context_translation'].splitlines())==3,translated
    assert any(stem in translated['translation'].lower() for stem in ('защи','охраня','берег')),translated
    grammar=request('/analyze',{'context':'She has written a letter.'})
    assert any(t['text']=='written' and t['verb_form']=='V3' and t['irregular'] for t in grammar['tokens']),grammar
    report={'startup_seconds':loaded,'translation_seconds':elapsed,'multiline_context':'passed','selected_second_line':'passed','grammar_v3_irregular':'passed','translation':translated}
    (project/'artifacts/mini-smoke.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps(report,ensure_ascii=False))
finally:
    child.terminate();child.wait(timeout=10);log.close()
