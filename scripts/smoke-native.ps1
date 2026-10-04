$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$binary = Join-Path $projectRoot 'src-tauri/resources/bin/translit_native.exe'
$dll = Join-Path $projectRoot 'src-tauri/resources/bin/translit_hook_v2.dll'
$hostProcess = Start-Process -FilePath "$projectRoot/src-tauri/resources/bin/translit_test_host.exe" -WindowStyle Hidden -PassThru
$sessionDirectory = Join-Path ([IO.Path]::GetTempPath()) "translit-native-v2-$($hostProcess.Id)"
try {
    Start-Sleep -Milliseconds 1200
    & $binary inject $hostProcess.Id $dll
    if ($LASTEXITCODE -ne 0) { throw 'Injection failed' }
    [IO.File]::WriteAllText((Join-Path $sessionDirectory 'capture.request'), '1')
    $deadline = [DateTime]::UtcNow.AddSeconds(8)
    while (!(Test-Path -LiteralPath "$sessionDirectory/frame.ready")) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'No captured frame' }
        Start-Sleep -Milliseconds 50
    }
    if ((Get-Content -LiteralPath "$sessionDirectory/frame.ready" -Raw) -ne 'ok') { throw 'Capture failed' }
    $ocrJson = & $binary ocr "$sessionDirectory/frame.bmp"
    if ($LASTEXITCODE -ne 0) { throw 'OCR failed' }
    $ocr = $ocrJson | ConvertFrom-Json
    if ($ocr.text -notmatch 'investigate' -or $ocr.words.Count -lt 12) { throw "OCR missed test dialogue: $($ocr.text)" }
    New-Item -ItemType Directory -Force -Path "$projectRoot/artifacts" | Out-Null
    Copy-Item -LiteralPath "$sessionDirectory/frame.bmp" -Destination "$projectRoot/artifacts/native-test-frame.bmp"
    [IO.File]::WriteAllText("$projectRoot/artifacts/native-test-ocr.json", $ocrJson)
    $watchdog = Start-Process -FilePath $binary -ArgumentList @('pause', $hostProcess.Id, $PID) -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(4)
    while (!(Test-Path -LiteralPath "$sessionDirectory/paused.status")) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Pause watchdog was not ready' }
        Start-Sleep -Milliseconds 50
    }
    Remove-Item -LiteralPath "$sessionDirectory/frame.ready"
    [IO.File]::WriteAllText((Join-Path $sessionDirectory 'capture.request'), '1')
    Start-Sleep -Milliseconds 600
    $after = (Get-Process -Id $hostProcess.Id).TotalProcessorTime.TotalMilliseconds
    # The observable criterion is absence of Present; driver accounting may lag.
    if (Test-Path -LiteralPath "$sessionDirectory/frame.ready") { throw 'Render thread continued during pause' }
    & $binary resume $hostProcess.Id $PID
    if (!$watchdog.WaitForExit(4000)) { throw 'Resume watchdog did not exit' }
    $deadline = [DateTime]::UtcNow.AddSeconds(6)
    while (!(Test-Path -LiteralPath "$sessionDirectory/frame.ready")) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Game did not resume rendering' }
        Start-Sleep -Milliseconds 50
    }
    $parentSentinel = Start-Process powershell -ArgumentList @('-NoProfile', '-Command', 'Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru
    Remove-Item -LiteralPath "$sessionDirectory/paused.status" -ErrorAction SilentlyContinue
    $crashWatchdog = Start-Process -FilePath $binary -ArgumentList @('pause', $hostProcess.Id, $parentSentinel.Id) -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(4)
    while (!(Test-Path -LiteralPath "$sessionDirectory/paused.status")) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Crash watchdog was not ready' }
        Start-Sleep -Milliseconds 50
    }
    Stop-Process -Id $parentSentinel.Id
    if (!$crashWatchdog.WaitForExit(4000)) { throw 'Crash recovery did not resume game' }
    $testWindow=(& $binary list | ConvertFrom-Json) | Where-Object pid -eq $hostProcess.Id | Select-Object -First 1
    if (!$testWindow) { throw 'Test window not enumerated before pause' }
    $renderWatchdog=Start-Process -FilePath $binary -ArgumentList @('pause-render',$hostProcess.Id,$PID) -WindowStyle Hidden -PassThru
    $deadline=[DateTime]::UtcNow.AddSeconds(5)
    while(!(Test-Path -LiteralPath "$sessionDirectory/paused.status")) { if([DateTime]::UtcNow -gt $deadline){throw 'Render pause not ready'}; Start-Sleep -Milliseconds 30 }
    Start-Sleep -Seconds 6
    & $binary ping $testWindow.hwnd
    if($LASTEXITCODE -ne 0){throw 'Window hung during render pause'}
    Remove-Item -LiteralPath "$sessionDirectory/frame.ready" -ErrorAction SilentlyContinue
    [IO.File]::WriteAllText((Join-Path $sessionDirectory 'capture.request'),'1')
    Start-Sleep -Milliseconds 400
    if(Test-Path -LiteralPath "$sessionDirectory/frame.ready"){throw 'Present continued during render pause'}
    & $binary resume $hostProcess.Id $PID
    if(!$renderWatchdog.WaitForExit(4000)){throw 'Render watchdog did not exit'}
    $deadline=[DateTime]::UtcNow.AddSeconds(5)
    while(!(Test-Path -LiteralPath "$sessionDirectory/frame.ready")) { if([DateTime]::UtcNow -gt $deadline){throw 'Render pause did not release'}; Start-Sleep -Milliseconds 30 }
    $renderParent = Start-Process powershell -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru
    $renderCrash = Start-Process -FilePath $binary -ArgumentList @('pause-render',$hostProcess.Id,$renderParent.Id) -WindowStyle Hidden -PassThru
    $deadline=[DateTime]::UtcNow.AddSeconds(5)
    while(!(Test-Path -LiteralPath "$sessionDirectory/paused.status")) { if([DateTime]::UtcNow -gt $deadline){throw 'Render recovery pause not ready'}; Start-Sleep -Milliseconds 30 }
    Stop-Process -Id $renderParent.Id
    if(!$renderCrash.WaitForExit(4000)){throw 'Render crash recovery failed'}
    & $binary ping $testWindow.hwnd
    if($LASTEXITCODE -ne 0){throw 'Render crash recovery left a hung window'}
    & $binary detach $hostProcess.Id
    Start-Sleep -Milliseconds 100
    if ((Get-Content -LiteralPath "$sessionDirectory/hook.status" -Raw) -ne 'disabled') { throw 'Hook was not disabled' }
    & $binary inject $hostProcess.Id $dll
    if ($LASTEXITCODE -ne 0) { throw 'Reattachment failed' }
    Remove-Item -LiteralPath "$sessionDirectory/frame.ready"
    [IO.File]::WriteAllText((Join-Path $sessionDirectory 'capture.request'), '1')
    $deadline = [DateTime]::UtcNow.AddSeconds(6)
    while (!(Test-Path -LiteralPath "$sessionDirectory/frame.ready")) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'No frame after reattachment' }
        Start-Sleep -Milliseconds 50
    }
    & $binary detach $hostProcess.Id
    @{injection='passed';capture='passed';ocrWords=$ocr.words.Count;pause='passed';renderPauseResponsive='passed';resume='passed';crashRecovery='passed';detach='passed';reattachWithoutRestart='passed'} | ConvertTo-Json | Set-Content "$projectRoot/artifacts/native-smoke.json"
    Get-Content "$projectRoot/artifacts/native-smoke.json"
} finally {
    & $binary resume $hostProcess.Id $PID
    if (!$hostProcess.HasExited) { Stop-Process -Id $hostProcess.Id }
}
