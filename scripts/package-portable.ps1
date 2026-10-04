param([switch]$SkipArchive)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$releaseDirectory = "$projectRoot/src-tauri/target/release"
if (!(Test-Path -LiteralPath "$releaseDirectory/translit.exe")) { throw 'Run npm run release first' }
$destination = "$projectRoot/artifacts/Translit-0.2.0-portable"
New-Item -ItemType Directory -Force -Path $destination, "$destination/bin", "$destination/data" | Out-Null
Copy-Item -LiteralPath "$releaseDirectory/translit.exe" -Destination "$destination/Translit.exe"
Copy-Item -LiteralPath "$projectRoot/src-tauri/resources/bin/translit_native.exe" -Destination "$destination/bin"
$hookSource = "$projectRoot/src-tauri/resources/bin/translit_hook_v2.dll"
$hookTarget = "$destination/bin/translit_hook_v2.dll"
if (!(Test-Path -LiteralPath $hookTarget) -or (Get-FileHash -LiteralPath $hookSource).Hash -ne (Get-FileHash -LiteralPath $hookTarget).Hash) {
    Copy-Item -LiteralPath $hookSource -Destination $hookTarget
}
Copy-Item -LiteralPath "$projectRoot/src-tauri/resources/bin/translit_test_host.exe" -Destination "$destination/bin"
Get-ChildItem -LiteralPath "$projectRoot/data" -File | Copy-Item -Destination "$destination/data"
Copy-Item -LiteralPath "$projectRoot/README.md" -Destination $destination
Copy-Item -LiteralPath "$projectRoot/docs" -Destination $destination -Recurse -Force
Copy-Item -LiteralPath "$projectRoot/native/build/_deps/minhook-src/LICENSE.txt" -Destination "$destination/MinHook-LICENSE.txt"
foreach ($resource in @('models','runtime','ocr')) {
    $target = Join-Path $destination $resource
    New-Item -ItemType Directory -Force -Path $target | Out-Null
    Get-ChildItem -LiteralPath "$projectRoot/src-tauri/resources/$resource" | Copy-Item -Destination $target -Recurse -Force
}
Get-ChildItem -LiteralPath "$destination/runtime/windows-x64" -Filter '*.dll' | Copy-Item -Destination $destination
if (!$SkipArchive) { Compress-Archive -Path "$destination/*" -DestinationPath "$projectRoot/artifacts/Translit-0.2.0-portable.zip" -Force }
Get-Item -LiteralPath "$destination/Translit.exe" | Select-Object FullName,Length
