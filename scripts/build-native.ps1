$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
function Get-TranslitHash([string] $Path) {
    $stream = [IO.File]::OpenRead($Path)
    $hash = [Security.Cryptography.SHA256]::Create()
    try { [BitConverter]::ToString($hash.ComputeHash($stream)) } finally { $stream.Dispose(); $hash.Dispose() }
}
cmake -S "$projectRoot/native" -B "$projectRoot/native/build" -A x64
if ($LASTEXITCODE -ne 0) { throw 'CMake configure failed' }
cmake --build "$projectRoot/native/build" --config Release --parallel
if ($LASTEXITCODE -ne 0) { throw 'Native build failed' }
$destination = "$projectRoot/src-tauri/resources/bin"
New-Item -ItemType Directory -Force -Path $destination | Out-Null
foreach ($translitBinary in @('translit_hook_v2.dll','translit_native.exe','translit_test_host.exe')) {
    $translitBuilt = "$projectRoot/native/build/Release/$translitBinary"
    $translitResource = "$destination/$translitBinary"
    # An unchanged DLL can already be resident in the running game. Do not rewrite it.
    if ((Test-Path -LiteralPath $translitResource) -and (Get-TranslitHash $translitResource) -eq (Get-TranslitHash $translitBuilt)) { continue }
    Copy-Item -LiteralPath $translitBuilt -Destination $translitResource
}
