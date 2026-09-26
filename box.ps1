<#
.SYNOPSIS
    Goraw Process Governor Box (16 GB Memory & Recursion Ceiling with Telemetry)
.DESCRIPTION
    Запускает любые команды сборки, тестов или компилятора внутри Job Object
    контейнера с жестким лимитом памяти 16 ГБ через procgov64.
    Собирает и отображает полную телеметрию выполнения: пиковое использование памяти,
    время ЦП (User/Kernel), общее время, число порожденных процессов и I/O.
.EXAMPLE
    .\box.ps1 test
    .\box.ps1 build
    .\box.ps1 run --bin gorawc -- --run examples/strings_v2.gw
    .\box.ps1 gorawc --run examples/strings_v2.gw
#>

# 1. Извлечение флага Silent / --silent / -s / /silent и переменных окружения
$isSilent = $false
if ($env:GORAW_BOX_SILENT -eq "1" -or $env:SILENT -eq "1" -or $env:CI -eq "1") {
    $isSilent = $true
}

$filteredArgs = [System.Collections.Generic.List[string]]::new()
foreach ($arg in $args) {
    if ($arg -match '^(?i)(--?silent|-s|/silent)$') {
        $isSilent = $true
    } else {
        $filteredArgs.Add($arg)
    }
}
$CommandAndArgs = $filteredArgs.ToArray()

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $scriptDir) { $scriptDir = (Get-Location).Path }

# Поддержка команды подписания песочницы: .\box.ps1 --sign
if ($CommandAndArgs.Count -eq 1 -and $CommandAndArgs[0] -match '^(?i)(--?sign|-sign|/sign)$') {
    $signScript = Join-Path $scriptDir "tools\sign_sandbox.ps1"
    if (Test-Path $signScript) {
        & $signScript
        exit $LASTEXITCODE
    } else {
        Write-Error "[BOX] Скрипт tools\sign_sandbox.ps1 не найден!"
        exit 1
    }
}

# 2. Поиск procgov64.exe
$procgov = Join-Path $scriptDir "tools\procgov\procgov64.exe"
if (-not (Test-Path $procgov)) {
    $procgov = (Get-Command procgov64, procgov -ErrorAction SilentlyContinue | Select-Object -First 1).Source
}

if (-not $procgov -or -not (Test-Path $procgov)) {
    Write-Error "[BOX] procgov64.exe не найден! Проверьте tools\procgov\procgov64.exe или системный PATH."
    exit 1
}

# 3. Инициализация C# хелпера телеметрии Job Object
$dllPath = Join-Path $scriptDir "tools\procgov\GorawJobHelper.dll"
$csPath = Join-Path $scriptDir "tools\procgov\GorawJobHelper.cs"

if (-not ([System.Management.Automation.PSTypeName]'Goraw.Box.JobHelper').Type) {
    if (Test-Path $dllPath) {
        Add-Type -Path $dllPath -ErrorAction SilentlyContinue
    }
    if (-not ([System.Management.Automation.PSTypeName]'Goraw.Box.JobHelper').Type -and (Test-Path $csPath)) {
        Add-Type -Path $csPath -OutputAssembly $dllPath -ErrorAction SilentlyContinue
    }
}

$hasTelemetry = ([System.Management.Automation.PSTypeName]'Goraw.Box.JobHelper').Type -ne $null

# 4. Вывод справки, если аргументы не переданы
if ($CommandAndArgs.Count -eq 0) {
    Write-Host "==========================================================" -ForegroundColor Cyan
    Write-Host "        Goraw Container Box Runner (16 GB Hard Limit)     " -ForegroundColor Cyan
    Write-Host "==========================================================" -ForegroundColor Cyan
    Write-Host "Использование:"
    Write-Host "  .\box.ps1 test                   - запуск cargo test под лимитом 16 ГБ"
    Write-Host "  .\box.ps1 build                  - запуск cargo build под лимитом 16 ГБ"
    Write-Host "  .\box.ps1 check                  - запуск cargo check под лимитом 16 ГБ"
    Write-Host "  .\box.ps1 run [args]             - запуск cargo run под лимитом 16 ГБ"
    Write-Host "  .\box.ps1 gorawc [args]          - запуск скомпилированного gorawc под лимитом 16 ГБ"
    Write-Host "  .\box.ps1 gorawas [args]         - запуск ассемблера gorawas под лимитом 16 ГБ"
    Write-Host "  .\box.ps1 gorawpb [args]         - запуск генератора gorawpb под лимитом 16 ГБ"
    Write-Host "  .\box.ps1 <любая команда>        - запуск произвольной команды в изолированном контейнере"
    Write-Host ""
    Write-Host "Флаги безопасности и режимы:"
    Write-Host "  --silent, -s                     - автопропуск предупреждений неподписанной песочницы"
    Write-Host "  --sign                           - создание сертификата и подписание компонентов песочницы"
    Write-Host ""
    Write-Host "Телеметрия: отслеживание Peak RAM, CPU Time, Wall Clock, I/O и кодов завершения."
    exit 0
}

# 5. Экспорт переменных окружения песочницы для процессов
$env:GORAW_SANDBOX_ACTIVE = "1"
if ($isSilent) {
    $env:GORAW_SILENT = "1"
}

$procgovSig = Get-AuthenticodeSignature -FilePath $procgov -ErrorAction SilentlyContinue
if ($procgovSig -and ($procgovSig.Status -eq 'Valid' -or $procgovSig.SignerCertificate -ne $null)) {
    $env:GORAW_SANDBOX_SIGNED = "1"
} else {
    $env:GORAW_SANDBOX_SIGNED = "0"
}

# 6. Разбор команды
$first = $CommandAndArgs[0]
$rest = if ($CommandAndArgs.Count -gt 1) { @($CommandAndArgs[1..($CommandAndArgs.Count - 1)]) } else { @() }

$targetExe = ""
$targetArgs = @()

switch ($first) {
    "test" {
        $targetExe = "cargo"
        $targetArgs = @("test") + @($rest)
    }
    "build" {
        $targetExe = "cargo"
        $targetArgs = @("build") + @($rest)
    }
    "check" {
        $targetExe = "cargo"
        $targetArgs = @("check") + @($rest)
    }
    "run" {
        $targetExe = "cargo"
        $targetArgs = @("run") + @($rest)
    }
    { $_ -in "gorawc", "goraw" } {
        $exeName = "$first.exe"
        $exePath = Join-Path $scriptDir "target\debug\$exeName"
        if (-not (Test-Path $exePath)) {
            $altPath = Join-Path $scriptDir "target\debug\gorawc.exe"
            if (Test-Path $altPath) {
                $exePath = $altPath
            } else {
                Write-Host "[BOX] $exeName не найден, выполняем сборку..." -ForegroundColor Yellow
                cargo build --bin $first
            }
        }
        $targetExe = $exePath
        $targetArgs = @($rest)
    }
    "gorawas" {
        $exePath = Join-Path $scriptDir "target\debug\gorawas.exe"
        if (-not (Test-Path $exePath)) {
            Write-Host "[BOX] gorawas.exe не найден, выполняем сборку..." -ForegroundColor Yellow
            cargo build --bin gorawas
        }
        $targetExe = $exePath
        $targetArgs = @($rest)
    }
    "gorawpb" {
        $exePath = Join-Path $scriptDir "target\debug\gorawpb.exe"
        if (-not (Test-Path $exePath)) {
            Write-Host "[BOX] gorawpb.exe не найден, выполняем сборку..." -ForegroundColor Yellow
            cargo build --bin gorawpb
        }
        $targetExe = $exePath
        $targetArgs = @($rest)
    }
    default {
        $targetExe = $first
        $targetArgs = @($rest)
    }
}

$fullCmd = @($targetExe) + @($targetArgs)

# 7. Подготовка Job Object и запуск под Process Governor
$jobName = "goraw_box_" + [System.Guid]::NewGuid().ToString("N")
$hJob = [IntPtr]::Zero

if ($hasTelemetry) {
    $hJob = [Goraw.Box.JobHelper]::CreateBoxJob($jobName)
}

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$exitCode = 0

try {
    if ($hJob -ne [IntPtr]::Zero) {
        & $procgov --job-name=$jobName -m 16G -r -q -- @fullCmd
    } else {
        & $procgov -m 16G -r -q -- @fullCmd
    }
    $exitCode = $LASTEXITCODE
} finally {
    $sw.Stop()
}

# 8. Сбор метрик и форматирование телеметрии
$snap = $null
if ($hasTelemetry -and ($hJob -ne [IntPtr]::Zero)) {
    $snap = [Goraw.Box.JobHelper]::QueryJob($hJob)
    [Goraw.Box.JobHelper]::CloseBoxJob($hJob)
}

Write-Host ""
Write-Host "┌────────────────────────────────────────────────────────┐" -ForegroundColor DarkCyan
Write-Host "│                Goraw Container Telemetry               │" -ForegroundColor Cyan
Write-Host "├────────────────────────────────────────────────────────┤" -ForegroundColor DarkCyan

# Статус завершения
if ($exitCode -eq 0) {
    Write-Host "│ Status:       " -NoNewline -ForegroundColor DarkCyan
    Write-Host "SUCCESS (Exit Code: 0)                   " -NoNewline -ForegroundColor Green
    Write-Host "│" -ForegroundColor DarkCyan
} else {
    $statusText = "FAILED (Exit Code: $exitCode)"
    $statusPad = $statusText.PadRight(41)
    Write-Host "│ Status:       " -NoNewline -ForegroundColor DarkCyan
    Write-Host $statusPad -NoNewline -ForegroundColor Red
    Write-Host "│" -ForegroundColor DarkCyan
}

# Время выполнения (Wall Clock)
$wallMs = $sw.ElapsedMilliseconds
$wallSec = $sw.Elapsed.TotalSeconds
$wallStr = if ($wallSec -ge 1.0) { "{0:N2} s ({1} ms)" -f $wallSec, $wallMs } else { "{0} ms" -f $wallMs }
$wallLine = ("Wall Clock:   " + $wallStr).PadRight(55)
Write-Host "│ $wallLine│" -ForegroundColor DarkCyan

# Время процессора (CPU Time)
if ($snap -and $snap.Success) {
    $cpuStr = "{0:N2} s (User: {1:N2}s | Kernel: {2:N2}s)" -f $snap.CpuTotalSeconds, $snap.CpuUserSeconds, $snap.CpuKernelSeconds
    $cpuLine = ("CPU Time:     " + $cpuStr).PadRight(55)
    Write-Host "│ $cpuLine│" -ForegroundColor DarkCyan
}

# Использование памяти (Peak RAM)
if ($snap -and $snap.Success -and ($snap.PeakJobMemoryBytes -gt 0)) {
    $peakMB = $snap.PeakJobMemoryMB
    $memStr = if ($peakMB -ge 1024.0) {
        "{0:N2} GB ({1:N0} MB) / 16,384 MB [{2:N2}%]" -f ($peakMB / 1024.0), $peakMB, $snap.MemoryUsagePercent
    } else {
        "{0:N2} MB / 16,384 MB [{1:N2}%]" -f $peakMB, $snap.MemoryUsagePercent
    }
    $memLine = ("Peak Memory:  " + $memStr).PadRight(55)
    Write-Host "│ $memLine│" -ForegroundColor DarkCyan
}

# Число процессов
if ($snap -and $snap.Success -and ($snap.TotalProcesses -gt 0)) {
    $procStr = "{0} spawned in container" -f $snap.TotalProcesses
    $procLine = ("Processes:    " + $procStr).PadRight(55)
    Write-Host "│ $procLine│" -ForegroundColor DarkCyan
}

# Дисковый I/O
if ($snap -and $snap.Success -and (($snap.ReadBytes -gt 0) -or ($snap.WriteBytes -gt 0))) {
    $ioStr = "Read: {0:N2} MB | Write: {1:N2} MB" -f $snap.ReadMB, $snap.WriteMB
    $ioLine = ("I/O Transfer: " + $ioStr).PadRight(55)
    Write-Host "│ $ioLine│" -ForegroundColor DarkCyan
}

# Лимит памяти
Write-Host "│ Memory Limit: 16.00 GB (Enforced via procgov64)        │" -ForegroundColor DarkCyan
Write-Host "└────────────────────────────────────────────────────────┘" -ForegroundColor DarkCyan

# Предупреждение при приближении к лимиту
if ($snap -and ($snap.PeakJobMemoryMB -ge 14000.0)) {
    Write-Host "⚠️ [WARNING] Использование памяти приблизилось к лимиту 16 ГБ ({0:N2} MB)!" -f $snap.PeakJobMemoryMB -ForegroundColor Red
}

exit $exitCode

# SIG # Begin signature block
# MIIF+AYJKoZIhvcNAQcCoIIF6TCCBeUCAQExDzANBglghkgBZQMEAgEFADB5Bgor
# BgEEAYI3AgEEoGswaTA0BgorBgEEAYI3AgEeMCYCAwEAAAQQH8w7YFlLCE63JNLG
# KX7zUQIBAAIBAAIBAAIBAAIBADAxMA0GCWCGSAFlAwQCAQUABCAs1BJh/7BLV6WQ
# j5kOuwPXQCQnpCTqqasB8d2p2RuDBKCCA0wwggNIMIICMKADAgECAhAw3/ZKuit3
# nUacfZqWIRLKMA0GCSqGSIb3DQEBCwUAMDwxDDAKBgNVBAsMA05QUzEaMBgGA1UE
# CgwRZGV2LmRvdWJsZWx1Yy5pY3UxEDAOBgNVBAMMB2x1Y19kZXYwHhcNMjYwODEz
# MTUyMDM3WhcNMzEwODEzMTUzMDM4WjA8MQwwCgYDVQQLDANOUFMxGjAYBgNVBAoM
# EWRldi5kb3VibGVsdWMuaWN1MRAwDgYDVQQDDAdsdWNfZGV2MIIBIjANBgkqhkiG
# 9w0BAQEFAAOCAQ8AMIIBCgKCAQEAt+QxIUKtn3YIz4iH/Sqg1+IieXzqkGB++SbV
# avo8hSDmYweout4oSaWPm4rImBMz8rpat8wupTO1eClAzasM8x1UHhob1au9mizT
# L3IiU4R09Oa2Yh0h7hFU2CFvDKGUvzrWOeiLNc5KxpVEe1gxNigWnpEhsUGfwxkK
# 9zUsj1fPuL7Q/smd8FlRF5sj4rijExgbC9Kz1/VHu0prTfJ7Lzuczb44Sci75swf
# LBwEN8rrtnmBx2qSWllRi0TdxmBy08jOgQnYQxCZIQzCjj0dLsY87GK2yYiHriRT
# DZ8ojO+s4DDwvnrZTyWn2tR3uc7+ICFQHMyGMWlEABzVNBSb8QIDAQABo0YwRDAO
# BgNVHQ8BAf8EBAMCB4AwEwYDVR0lBAwwCgYIKwYBBQUHAwMwHQYDVR0OBBYEFIGx
# C7Hok8SOl4/HM+/OwA6WxsCLMA0GCSqGSIb3DQEBCwUAA4IBAQBHiiuq6UaGSELQ
# qERrG8fphE0wogQlx1h63pwtj10xO04tRZHTJuw27gkL1SKJ9N57ZRZKKgBtFMTD
# a86a/ChJcXEWtuOT8f9f5r+XtKEXCaVMXmDiChpHBewXSjZBH85BAQYuYvAb1pJW
# YKxh9ErDtfdaDBlb1U5LAHVEshbXU5b3MB6MeY5LRcEWkHjY0K/L1cplMjUdPQ3e
# QbAn5SlIkea/jljGrBo6duu3+oejp03DdtMCoC3sHkoC6QKdbswVT6x0sVvYOOea
# wPQ/6RcnRn4t7BSlqhFPl//K0KxBanUMOZJtwWaY3mwpWPjBQsKD5Ao2FJ6HG/B1
# /HuHK2fxMYICAjCCAf4CAQEwUDA8MQwwCgYDVQQLDANOUFMxGjAYBgNVBAoMEWRl
# di5kb3VibGVsdWMuaWN1MRAwDgYDVQQDDAdsdWNfZGV2AhAw3/ZKuit3nUacfZqW
# IRLKMA0GCWCGSAFlAwQCAQUAoIGEMBgGCisGAQQBgjcCAQwxCjAIoAKAAKECgAAw
# GQYJKoZIhvcNAQkDMQwGCisGAQQBgjcCAQQwHAYKKwYBBAGCNwIBCzEOMAwGCisG
# AQQBgjcCARUwLwYJKoZIhvcNAQkEMSIEIFEtXMPC0OFwzBlG8dbOhDrugRxhSJ7C
# 1mLwqE4v8CPKMA0GCSqGSIb3DQEBAQUABIIBAHrox+HoAF8eD51RNHAT6L+X0glZ
# 7T+yMp//CQixIzZmiF7WwhPBC63wOwi/zmBOZup5T/qPesdW4NWZ8KMf4NPq8isj
# L9q9Yj+iV9A3lR5A52KdQB9yUYoftyh5Hwiv+BmJCfC4veg0+uiE9M+c62sHnEXv
# IuOx/SQaArgW+69WiZWdHYdpdgi95txSnAPcZfvPtT2JGVRdnyrvWQWM1qi7wNhN
# 349A7bdoN0+7skgQew9vf4FO1qiVxdIt5lU4qcZf8IZIvoLvO/AXjuN0UIPgzQq5
# FyqU/kVVnFi2ufqG8HOynReeMi/syitSdHPrwC+O94UHlGbAhswx1zJ9zI8=
# SIG # End signature block
