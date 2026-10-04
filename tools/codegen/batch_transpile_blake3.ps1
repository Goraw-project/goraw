# Batch Transpiler for BLAKE3 into Pure Goraw

$srcDir = "p:\Goraw\blake3_repo\c"
$outDir = "p:\Goraw\std\crypto\blake3"

if (-not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Path $outDir -Force | Out-Null
}

$files = Get-ChildItem -Path $srcDir -Filter "*.c" | Where-Object { $_.Name -ne "example.c" -and $_.Name -ne "main.c" -and $_.Name -ne "blake3_tbb.cpp" } | Sort-Object Length
Write-Host "=== Transpiling BLAKE3 ($($files.Count) files) into std/crypto/blake3 ===" -ForegroundColor Magenta

$files | ForEach-Object -Parallel {
    $outDir = $using:outDir
    $srcDir = $using:srcDir
    $f = $_
    $baseName = [System.IO.Path]::GetFileNameWithoutExtension($f.Name)
    $targetGw = Join-Path $outDir "$baseName.gw"
    
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & goraw from-cpp $f.FullName -o $targetGw -I $srcDir
    $sw.Stop()
    
    if ($LASTEXITCODE -eq 0 -and (Test-Path $targetGw)) {
        $lines = (Get-Content $targetGw).Count
        Write-Host "[OK] $($f.Name) -> $baseName.gw ($lines lines in $($sw.Elapsed.TotalSeconds.ToString('F1'))s)" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] $($f.Name)" -ForegroundColor Red
    }
} -ThrottleLimit 8

$modPath = Join-Path $outDir "mod.gw"
$sb = [System.Text.StringBuilder]::new()
[void]$sb.AppendLine("// BLAKE3 Cryptographic Hash Module Entry Point")
[void]$sb.AppendLine()
$gwFiles = Get-ChildItem -Path $outDir -Filter "*.gw" | Where-Object { $_.Name -ne "mod.gw" } | Sort-Object Name
foreach ($gw in $gwFiles) {
    [void]$sb.AppendLine("import `"$($gw.Name)`";")
}
[System.IO.File]::WriteAllText($modPath, $sb.ToString())
Write-Host "=== std/crypto/blake3/mod.gw Complete ($($gwFiles.Count) modules)! ===" -ForegroundColor Green
