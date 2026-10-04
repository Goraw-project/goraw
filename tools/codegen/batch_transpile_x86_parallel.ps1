$srcDir = "p:\Goraw\llvm-project\llvm\lib\Target\X86"
$outDir = "p:\Goraw\x86"
$includeDir1 = "p:\Goraw\llvm-project\llvm\include"
$includeDir2 = "p:\Goraw\llvm-project\llvm\lib\Target\X86"

if (-not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Path $outDir -Force | Out-Null
}

# Copy already tested modules
if (Test-Path "p:\Goraw\scratch\test_x86_mfi.gw") {
    Copy-Item "p:\Goraw\scratch\test_x86_mfi.gw" "$outDir\X86MachineFunctionInfo.gw" -Force
}
if (Test-Path "p:\Goraw\scratch\test_x86_subtarget.gw") {
    Copy-Item "p:\Goraw\scratch\test_x86_subtarget.gw" "$outDir\X86Subtarget.gw" -Force
}
if (Test-Path "p:\Goraw\scratch\test_x86_isellower.gw") {
    Copy-Item "p:\Goraw\scratch\test_x86_isellower.gw" "$outDir\X86ISelLowering.gw" -Force
}
if (Test-Path "p:\Goraw\scratch\test_x86_reginfo.gw") {
    Copy-Item "p:\Goraw\scratch\test_x86_reginfo.gw" "$outDir\X86RegisterInfo.gw" -Force
}

$files = Get-ChildItem -Path $srcDir -Filter *.cpp | Sort-Object Length

Write-Host "=== X86 Target Parallel Transpiler (8 Threads on 32 Cores) ===" -ForegroundColor Magenta
Write-Host "Total files to process: $($files.Count)"

$totalStopwatch = [System.Diagnostics.Stopwatch]::StartNew()

$files | ForEach-Object -Parallel {
    $outDir = $using:outDir
    $includeDir1 = $using:includeDir1
    $includeDir2 = $using:includeDir2
    $f = $_
    
    $baseName = [System.IO.Path]::GetFileNameWithoutExtension($f.Name)
    $targetGw = Join-Path $outDir "$baseName.gw"
    
    if (Test-Path $targetGw) {
        $len = (Get-Item $targetGw).Length
        if ($len -gt 50) {
            Write-Host "[SKIP] $baseName already exists ($([math]::round($len/1KB, 1)) KB)" -ForegroundColor DarkGray
            return
        }
    }
    
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & goraw from-cpp $f.FullName -o $targetGw -I $includeDir1 -I $includeDir2
    $sw.Stop()
    
    if ($LASTEXITCODE -eq 0 -and (Test-Path $targetGw)) {
        $gwLen = (Get-Item $targetGw).Length
        $lines = (Get-Content $targetGw).Count
        Write-Host "[OK] $($f.Name) -> $baseName.gw ($lines lines, $([math]::round($gwLen/1KB, 1)) KB in $($sw.Elapsed.TotalSeconds.ToString('F1'))s)" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] $($f.Name) (ExitCode: $LASTEXITCODE)" -ForegroundColor Red
    }
} -ThrottleLimit 8

$totalStopwatch.Stop()
Write-Host "`nParallel transpilation finished in $($totalStopwatch.Elapsed.TotalSeconds.ToString('F1'))s" -ForegroundColor Cyan

Write-Host "`n=== Generating mod.gw ===" -ForegroundColor Magenta
$modPath = Join-Path $outDir "mod.gw"

$sb = [System.Text.StringBuilder]::new()
[void]$sb.AppendLine("// X86-64 Target Architecture Module Entry Point")
[void]$sb.AppendLine()
$gwFiles = Get-ChildItem -Path $outDir -Filter "*.gw" | Where-Object { $_.Name -ne "mod.gw" -and $_.Name -notlike "*monolith*" } | Sort-Object Name
foreach ($gw in $gwFiles) {
    [void]$sb.AppendLine("import `"$($gw.Name)`";")
}

[System.IO.File]::WriteAllText($modPath, $sb.ToString())
Write-Host "=== X86 mod.gw Complete ($($gwFiles.Count) modules)! ===" -ForegroundColor Green
Write-Host "File: $modPath"

