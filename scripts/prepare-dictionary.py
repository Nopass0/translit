"""Convert the licensed FreeDict TEI archive to a compact local lookup table."""
import json
import pathlib
import tarfile
import xml.etree.ElementTree as ET

root = pathlib.Path(__file__).resolve().parents[1]
with tarfile.open(root / "artifacts/eng-rus-large.src.tar.xz") as archive:
    source = archive.extractfile("eng-rus/eng-rus.tei").read()
    for filename in ("COPYING", "README"):
        (root / "data" / f"FreeDict-{filename}.txt").write_bytes(archive.extractfile(f"eng-rus/{filename}").read())
    (root / "data/eng-rus.tei").write_bytes(source)
tree = ET.fromstring(source)
ns = {"t": "http://www.tei-c.org/ns/1.0"}
dictionary = {}
for entry in tree.findall(".//t:entry", ns):
    spellings = [" ".join("".join(x.itertext()).split()).lower() for x in entry.findall(".//t:orth", ns)]
    translations = [" ".join("".join(x.itertext()).split()) for x in entry.findall(".//t:cit[@type='trans']/t:quote", ns)]
    if not translations:
        translations = [" ".join("".join(x.itertext()).split()) for x in entry.findall(".//t:trans", ns)]
    for spelling in spellings:
        if translations:
            dictionary.setdefault(spelling, [])
            dictionary[spelling] = list(dict.fromkeys(dictionary[spelling] + translations))
(root / "data/eng-rus.json").write_text(json.dumps(dictionary, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
print(f"FreeDict: {len(dictionary)} headwords")
