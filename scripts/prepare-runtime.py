"""Prepare an offline, hash-verified autonomous OPUS/spaCy runtime for the installer."""
from pathlib import Path
import os,json,hashlib,zipfile,shutil,urllib.request,argparse
root=Path(__file__).resolve().parent.parent
parser=argparse.ArgumentParser()
parser.add_argument('--cache',type=Path,default=root/'.cache/runtime-downloads')
parser.add_argument('--grammar-wheels',type=Path,default=root/'.cache/grammar-wheels')
args=parser.parse_args()
destination=root/'src-tauri/resources/models/opus'
destination.mkdir(parents=True,exist_ok=True)
def archive(url,digest,file,directory,strip=False):
    """Verify before extracting; refuse paths outside the intended build directory."""
    if not file.exists():
        file.parent.mkdir(parents=True,exist_ok=True)
        urllib.request.urlretrieve(url,file)
    with file.open('rb') as stream: actual=hashlib.file_digest(stream,'sha256').hexdigest()
    if actual!=digest:raise RuntimeError('Hash mismatch: '+file.name)
    with zipfile.ZipFile(file) as package:
        for member in package.infolist():
            if member.is_dir():continue
            parts=Path(member.filename).parts[1:] if strip else Path(member.filename).parts
            target=directory.joinpath(*parts)
            if not target.resolve().is_relative_to(directory.resolve()):raise RuntimeError('Invalid archive path')
            target.parent.mkdir(parents=True,exist_ok=True)
            with package.open(member) as source,target.open('wb') as output:shutil.copyfileobj(source,output)

archive('https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip','4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3',args.cache/'engine.zip',destination/'engine')
for dependency in json.loads((root/'src-tauri/src/mini-dependencies.json').read_text(encoding='utf-8-sig')):
    archive(dependency['url'],dependency['hash'],args.cache/dependency['file'],destination/dependency['destination'])
for dependency in json.loads((root/'src-tauri/src/grammar-dependencies.json').read_text()):
    archive(dependency['url'],dependency['hash'],args.grammar_wheels/dependency['file'],destination/dependency['destination'])
archive('https://huggingface.co/ordois/opus-mt-en-ru-ctranslate2-int8/resolve/9ae164a637a7e380d3e09768899c886af9925afe/opus-mt-en-ru-ctranslate2-int8.zip','f4c3ab9becb25549673ae407c5db73f86bcf57719426e497c592415a5b512bcd',args.cache/'model.zip',destination/'model',True)
(destination/'engine/python312._pth').write_text('python312.zip\n.\nLib/site-packages\nimport site\n')
for file in ('mini_server.py','grammar_worker.py'):shutil.copyfile(root/'src-tauri/src'/file,destination/'engine'/file)
crt=root/'src-tauri/resources/runtime/windows-x64'
crt.mkdir(parents=True,exist_ok=True)
for name in ('msvcp140.dll','msvcp140_1.dll','msvcp140_2.dll','vcruntime140.dll','vcruntime140_1.dll','concrt140.dll'):
    source=Path(os.environ['SystemRoot'])/'System32'/name
    if not source.exists():raise RuntimeError('Install the VC++ build runtime before packaging: '+name)
    shutil.copy2(source,crt/name);shutil.copy2(source,destination/'engine'/name)
(destination/'installed.json').write_text(json.dumps({'model_sha256':'f4c3ab9becb25549673ae407c5db73f86bcf57719426e497c592415a5b512bcd','runtime_sha256':'4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3'}))
(destination/'grammar-ready.json').write_text(json.dumps({'spacy':'3.8.11','model':'en_core_web_sm-3.8.0'}))
(destination/'SOURCES.txt').write_text('OPUS-MT EN-RU: Apache 2.0. https://huggingface.co/Helsinki-NLP/opus-mt-en-ru\nCTranslate2: MIT. Python: PSF. Pinned wheel URLs and hashes: src-tauri/src/mini-dependencies.json and grammar-dependencies.json. Licenses are preserved in dist-info directories.\n')
with (destination/'SOURCES.txt').open('a') as output:output.write('\nspaCy 3.8.11 and en_core_web_sm 3.8.0: MIT. https://spacy.io/models/en Licenses are preserved in package dist-info directories. Microsoft VC++ application-local runtime: https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files\n')
(crt/'SOURCES.txt').write_text('Microsoft Visual C++ application-local runtime. https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files\n')
print('Offline runtime:',destination)
