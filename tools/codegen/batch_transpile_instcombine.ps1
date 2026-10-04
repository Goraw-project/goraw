# Batch Transpiler for LLVM InstCombine Optimization Pass into Pure Goraw

$llvmInclude = "p:\Goraw\llvm-project\llvm\include"
$srcDir = "p:\Goraw\llvm-project\llvm\lib\Transforms\InstCombine"
$outDir = "p:\Goraw\backend\opt\instcombine"

if (-not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Path $outDir -Force | Out-Null
}

$files = Get-ChildItem -Path $srcDir -Filter "*.cpp" | Sort-Object Length
Write-Host "=== Transpiling LLVM InstCombine ($($files.Count) files) ===" -ForegroundColor Magenta

$files | ForEach-Object -Parallel {
    $outDir = $using:outDir
    $llvmInclude = $using:llvmInclude
    $f = $_
    $baseName = [System.IO.Path]::GetFileNameWithoutExtension($f.Name)
    $targetGw = Join-Path $outDir "$baseName.gw"
    
    if (Test-Path $targetGw) {
        if ((Get-Item $targetGw).Length -gt 50) {
            Write-Host "[SKIP] $baseName already exists" -ForegroundColor DarkGray
            return
        }
    }
    
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & goraw from-cpp $f.FullName -o $targetGw -I $llvmInclude
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
[void]$sb.AppendLine("// LLVM InstCombine Optimization Pass Module Entry Point")
[void]$sb.AppendLine()
$gwFiles = Get-ChildItem -Path $outDir -Filter "*.gw" | Where-Object { $_.Name -ne "mod.gw" } | Sort-Object Name
foreach ($gw in $gwFiles) {
    [void]$sb.AppendLine("import `"$($gw.Name)`";")
}
[System.IO.File]::WriteAllText($modPath, $sb.ToString())
Write-Host "=== instcombine/mod.gw Complete ($($gwFiles.Count) modules)! ===" -ForegroundColor Green
