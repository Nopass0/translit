$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cache = Join-Path $projectRoot '.cache/ocr'
$destination = Join-Path $projectRoot 'src-tauri/resources/ocr'
$sourceUrl = 'https://github.com/tesseract-ocr/tesseract/releases/download/5.5.3/tesseract-ocr-w64-setup-5.5.3.20260724.exe'
$expectedHash = 'bee9e3434bd94fd65387d9be28cd467a41f61b1275383b55b0f59a1331270ae4'
New-Item -ItemType Directory -Force -Path $cache, $destination | Out-Null
$archive = Join-Path $cache 'tesseract-5.5.3.exe'
if (!(Test-Path -LiteralPath $archive)) { Invoke-WebRequest -Uri $sourceUrl -OutFile $archive }
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expectedHash) { throw 'Tesseract archive checksum mismatch' }
$unpacked = Join-Path $cache 'extracted'
& 7z x $archive "-o$unpacked" -y | Out-Null
if ($LASTEXITCODE -ne 0) { throw '7-Zip could not extract Tesseract' }
Get-ChildItem -LiteralPath $unpacked -Filter '*.dll' -File | Copy-Item -Destination $destination
Copy-Item -LiteralPath "$unpacked/tesseract.exe" -Destination $destination
Copy-Item -LiteralPath "$unpacked/doc" -Destination $destination -Recurse -Force
New-Item -ItemType Directory -Force -Path "$destination/tessdata" | Out-Null
# Model release 4.1.0 is pinned below; verify the downloaded bytes before bundling.
$modelUrl = 'https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/4.1.0/eng.traineddata'
$model = Join-Path $destination 'tessdata/eng.traineddata'
if (!(Test-Path -LiteralPath $model)) { Invoke-WebRequest -Uri $modelUrl -OutFile $model }
$modelHash = (Get-FileHash -LiteralPath $model -Algorithm SHA256).Hash.ToLowerInvariant()
if ($modelHash -ne '7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2') { throw "English OCR model checksum mismatch: $modelHash" }
"Tesseract 5.5.3 Windows build: $sourceUrl`nSHA256: $expectedHash`nEnglish tessdata_fast 4.1.0: $modelUrl`nSHA256: $modelHash`nTesseract / traineddata: Apache-2.0; component licenses in doc/ and upstream MSYS2 packages." | Set-Content -LiteralPath "$destination/SOURCES.txt" -Encoding utf8
& "$destination/tesseract.exe" --version
