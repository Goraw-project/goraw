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

$CommandAndArgs = $args

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $scriptDir) { $scriptDir = (Get-Location).Path }

# 1. Поиск procgov64.exe
$procgov = Join-Path $scriptDir "tools\procgov\procgov64.exe"
if (-not (Test-Path $procgov)) {
    $procgov = (Get-Command procgov64, procgov -ErrorAction SilentlyContinue | Select-Object -First 1).Source
}

if (-not $procgov -or -not (Test-Path $procgov)) {
    Write-Error "[BOX] procgov64.exe не найден! Проверьте tools\procgov\procgov64.exe или системный PATH."
    exit 1
}

# 2. Инициализация C# хелпера телеметрии Job Object
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

# 3. Вывод справки, если аргументы не переданы
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
    Write-Host "Телеметрия: отслеживание Peak RAM, CPU Time, Wall Clock, I/O и кодов завершения."
    exit 0
}

# 4. Разбор команды
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
    "gorawc" {
        $exePath = Join-Path $scriptDir "target\debug\gorawc.exe"
        if (-not (Test-Path $exePath)) {
            Write-Host "[BOX] gorawc.exe не найден, выполняем сборку..." -ForegroundColor Yellow
            cargo build --bin gorawc
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

# 5. Подготовка Job Object и запуск под Process Governor
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

# 6. Сбор метрик и форматирование телеметрии
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
