# Batch Transpiler for Clang Frontend (Lexer + Preprocessor + Parser) into Pure Goraw

$llvmInclude = "p:\Goraw\llvm-project\llvm\include"
$clangInclude = "p:\Goraw\llvm-project\clang\include"

# 1. Clang Lexer & Preprocessor
$lexSrc = "p:\Goraw\llvm-project\clang\lib\Lex"
$lexOut = "p:\Goraw\backend\clang_lex"
if (-not (Test-Path $lexOut)) {
    New-Item -ItemType Directory -Path $lexOut -Force | Out-Null
}

$lexFiles = Get-ChildItem -Path $lexSrc -Filter "*.cpp" | Sort-Object Length
Write-Host "=== Transpiling Clang Lexer & Preprocessor ($($lexFiles.Count) files) ===" -ForegroundColor Magenta

$lexFiles | ForEach-Object -Parallel {
    $lexOut = $using:lexOut
    $llvmInclude = $using:llvmInclude
    $clangInclude = $using:clangInclude
    $f = $_
    $baseName = [System.IO.Path]::GetFileNameWithoutExtension($f.Name)
    $targetGw = Join-Path $lexOut "$baseName.gw"
    
    if (Test-Path $targetGw) {
        if ((Get-Item $targetGw).Length -gt 50) {
            Write-Host "[SKIP] $baseName already exists" -ForegroundColor DarkGray
            return
        }
    }
    
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & goraw from-cpp $f.FullName -o $targetGw -I $llvmInclude -I $clangInclude
    $sw.Stop()
    
    if ($LASTEXITCODE -eq 0 -and (Test-Path $targetGw)) {
        $lines = (Get-Content $targetGw).Count
        Write-Host "[OK] $($f.Name) -> $baseName.gw ($lines lines in $($sw.Elapsed.TotalSeconds.ToString('F1'))s)" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] $($f.Name)" -ForegroundColor Red
    }
} -ThrottleLimit 8

# Generate clang_lex/mod.gw
$lexMod = Join-Path $lexOut "mod.gw"
$sbLex = [System.Text.StringBuilder]::new()
[void]$sbLex.AppendLine("// Clang Lexer & Preprocessor Module Entry Point")
[void]$sbLex.AppendLine()
$gwLex = Get-ChildItem -Path $lexOut -Filter "*.gw" | Where-Object { $_.Name -ne "mod.gw" } | Sort-Object Name
foreach ($gw in $gwLex) {
    [void]$sbLex.AppendLine("import `"$($gw.Name)`";")
}
[System.IO.File]::WriteAllText($lexMod, $sbLex.ToString())
Write-Host "=== clang_lex/mod.gw Complete ($($gwLex.Count) modules)! ===" -ForegroundColor Green

# 2. Clang Parser & AST Parsing
$parseSrc = "p:\Goraw\llvm-project\clang\lib\Parse"
$parseOut = "p:\Goraw\backend\clang_parse"
if (-not (Test-Path $parseOut)) {
    New-Item -ItemType Directory -Path $parseOut -Force | Out-Null
}

$parseFiles = Get-ChildItem -Path $parseSrc -Filter "*.cpp" | Sort-Object Length
Write-Host "`n=== Transpiling Clang Parser ($($parseFiles.Count) files) ===" -ForegroundColor Magenta

$parseFiles | ForEach-Object -Parallel {
    $parseOut = $using:parseOut
    $llvmInclude = $using:llvmInclude
    $clangInclude = $using:clangInclude
    $f = $_
    $baseName = [System.IO.Path]::GetFileNameWithoutExtension($f.Name)
    $targetGw = Join-Path $parseOut "$baseName.gw"
    
    if (Test-Path $targetGw) {
        if ((Get-Item $targetGw).Length -gt 50) {
            Write-Host "[SKIP] $baseName already exists" -ForegroundColor DarkGray
            return
        }
    }
    
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & goraw from-cpp $f.FullName -o $targetGw -I $llvmInclude -I $clangInclude
    $sw.Stop()
    
    if ($LASTEXITCODE -eq 0 -and (Test-Path $targetGw)) {
        $lines = (Get-Content $targetGw).Count
        Write-Host "[OK] $($f.Name) -> $baseName.gw ($lines lines in $($sw.Elapsed.TotalSeconds.ToString('F1'))s)" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] $($f.Name)" -ForegroundColor Red
    }
} -ThrottleLimit 8

# Generate clang_parse/mod.gw
$parseMod = Join-Path $parseOut "mod.gw"
$sbParse = [System.Text.StringBuilder]::new()
[void]$sbParse.AppendLine("// Clang Parser Module Entry Point")
[void]$sbParse.AppendLine()
$gwParse = Get-ChildItem -Path $parseOut -Filter "*.gw" | Where-Object { $_.Name -ne "mod.gw" } | Sort-Object Name
foreach ($gw in $gwParse) {
    [void]$sbParse.AppendLine("import `"$($gw.Name)`";")
}
[System.IO.File]::WriteAllText($parseMod, $sbParse.ToString())
Write-Host "=== clang_parse/mod.gw Complete ($($gwParse.Count) modules)! ===" -ForegroundColor Green
