$srcDir = "p:\Goraw\llvm-project\llvm\lib\CodeGen\SelectionDAG"
$scratchDir = "p:\Goraw\scratch"
$includeDir = "p:\Goraw\llvm-project\llvm\include"

# Copy address analysis if needed
if ((Test-Path "$scratchDir\real_sd_address_ast.gw") -and -not (Test-Path "$scratchDir\real_SelectionDAGAddressAnalysis.gw")) {
    Copy-Item "$scratchDir\real_sd_address_ast.gw" "$scratchDir\real_SelectionDAGAddressAnalysis.gw"
}

$files = Get-ChildItem -Path $srcDir -Filter *.cpp | Sort-Object Length

Write-Host "=== SelectionDAG Transpiler Batch Runner ==="
Write-Host "Total files to process: $($files.Count)"

$successCount = 0
$failCount = 0

foreach ($f in $files) {
    $baseName = [System.IO.Path]::GetFileNameWithoutExtension($f.Name)
    $targetGw = Join-Path $scratchDir "real_$baseName.gw"
    
    if (Test-Path $targetGw) {
        $len = (Get-Item $targetGw).Length
        if ($len -gt 100) {
            Write-Host "[SKIP] $baseName already transpiled ($([math]::round($len/1KB, 1)) KB)" -ForegroundColor Green
            $successCount++
            continue
        }
    }
    
    Write-Host "[TRANSPILING] $($f.Name) ($([math]::round($f.Length/1KB, 1)) KB)..." -ForegroundColor Cyan
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    
    & goraw from-cpp $f.FullName -o $targetGw -I $includeDir
    $stopwatch.Stop()
    
    if ($LASTEXITCODE -eq 0 -and (Test-Path $targetGw)) {
        $gwLen = (Get-Item $targetGw).Length
        Write-Host "[DONE] $($f.Name) -> real_$baseName.gw ($([math]::round($gwLen/1KB, 1)) KB) in $($stopwatch.Elapsed.TotalSeconds.ToString('F1'))s" -ForegroundColor Green
        $successCount++
    } else {
        Write-Host "[WARN] Transpilation failed for $($f.Name) (ExitCode: $LASTEXITCODE)" -ForegroundColor Yellow
        $failCount++
    }
}

Write-Host "`n=== Generating mod.gw ===" -ForegroundColor Magenta
$modPath = Join-Path $scratchDir "mod.gw"

$sb = [System.Text.StringBuilder]::new()
[void]$sb.AppendLine("// LLVM SelectionDAG Module Entry Point - Pure Goraw Implementation")
[void]$sb.AppendLine()
$gwFiles = Get-ChildItem -Path $scratchDir -Filter "real_*.gw" | Where-Object { $_.Name -ne "mod.gw" -and $_.Name -ne "real_sd_address_ast.gw" } | Sort-Object Name
foreach ($gw in $gwFiles) {
    $cleanName = $gw.Name -replace "^real_", ""
    [void]$sb.AppendLine("import `"$cleanName`";")
}

[System.IO.File]::WriteAllText($modPath, $sb.ToString())
Write-Host "=== SelectionDAG mod.gw Complete ($($gwFiles.Count) modules)! ===" -ForegroundColor Green
Write-Host "File: $modPath"

Write-Host "Lines: $lineCount"
