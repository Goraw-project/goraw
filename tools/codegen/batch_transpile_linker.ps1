$outDir = "p:\Goraw\backend\linker"
$inc1 = "p:\Goraw\llvm-project\lld\include"
$inc2 = "p:\Goraw\llvm-project\lld\COFF"
$inc3 = "p:\Goraw\llvm-project\llvm\include"
$inc4 = "p:\Goraw\llvm-project\llvm\lib\Linker"

if (-not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Path $outDir -Force | Out-Null
}

$files = @(
    Get-ChildItem -Path "p:\Goraw\llvm-project\lld\COFF" -Filter "*.cpp"
    Get-ChildItem -Path "p:\Goraw\llvm-project\llvm\lib\Linker" -Filter "*.cpp"
    Get-ChildItem -Path "p:\Goraw\llvm-project\llvm\tools\llvm-link" -Filter "*.cpp"
) | Sort-Object Length

Write-Host "=== LLVM Linker (LLD-Link / COFF + llvm-link) Parallel Transpiler ===" -ForegroundColor Magenta
Write-Host "Total files to transpile: $($files.Count)"

$totalSw = [System.Diagnostics.Stopwatch]::StartNew()

$files | ForEach-Object -Parallel {
    $outDir = $using:outDir
    $inc1 = $using:inc1
    $inc2 = $using:inc2
    $inc3 = $using:inc3
    $inc4 = $using:inc4
    $f = $_
    
    $baseName = [System.IO.Path]::GetFileNameWithoutExtension($f.Name)
    if ($baseName -eq "llvm-link") {
        $baseName = "LLVMLink"
    }
    $targetGw = Join-Path $outDir "$baseName.gw"
    
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & goraw from-cpp $f.FullName -o $targetGw -I $inc1 -I $inc2 -I $inc3 -I $inc4
    $sw.Stop()
    
    if ($LASTEXITCODE -eq 0 -and (Test-Path $targetGw)) {
        $gwLen = (Get-Item $targetGw).Length
        $lines = (Get-Content $targetGw).Count
        Write-Host "[OK] $($f.Name) -> $baseName.gw ($lines lines, $([math]::round($gwLen/1KB, 1)) KB in $($sw.Elapsed.TotalSeconds.ToString('F1'))s)" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] $($f.Name) (ExitCode: $LASTEXITCODE)" -ForegroundColor Red
    }
} -ThrottleLimit 8

$totalSw.Stop()
Write-Host "`nParallel transpilation finished in $($totalSw.Elapsed.TotalSeconds.ToString('F1'))s" -ForegroundColor Cyan

Write-Host "`n=== Generating mod.gw ===" -ForegroundColor Magenta
$modPath = Join-Path $outDir "mod.gw"

$sb = [System.Text.StringBuilder]::new()
[void]$sb.AppendLine("// Linker Module Entry Point (LLD-Link / COFF + LLVM-Link)")
[void]$sb.AppendLine()
$gwFiles = Get-ChildItem -Path $outDir -Filter "*.gw" | Where-Object { $_.Name -ne "mod.gw" -and $_.Name -notlike "*monolith*" } | Sort-Object Name
foreach ($gw in $gwFiles) {
    [void]$sb.AppendLine("import `"$($gw.Name)`";")
}

[System.IO.File]::WriteAllText($modPath, $sb.ToString())
Write-Host "=== Linker mod.gw Complete ($($gwFiles.Count) modules)! ===" -ForegroundColor Green
Write-Host "File: $modPath"

