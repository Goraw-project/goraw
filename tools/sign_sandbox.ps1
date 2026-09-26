<#
.SYNOPSIS
    Подписание компонентов песочницы Goraw цифровой подписью Authenticode
.DESCRIPTION
    Использует существующий или генерирует новый сертификат подписи кода
    и подписывает компоненты песочницы: procgov64.exe, GorawJobHelper.dll и box.ps1.
.EXAMPLE
    .\tools\sign_sandbox.ps1
    .\tools\sign_sandbox.ps1 -ForceNewCert
#>
param(
    [switch]$ForceNewCert
)

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $scriptDir) { $scriptDir = (Get-Location).Path }
$rootDir = Split-Path -Parent $scriptDir

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "         Goraw Sandbox Authenticode Signer                " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Поиск или создание Code Signing сертификата
$cert = $null
if (-not $ForceNewCert) {
    $cert = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert -ErrorAction SilentlyContinue | Where-Object {
        $_.Subject -match "Goraw" -or $_.Subject -match "luc_dev"
    } | Select-Object -First 1

    if (-not $cert) {
        $cert = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert -ErrorAction SilentlyContinue | Select-Object -First 1
    }
}

if (-not $cert) {
    Write-Host "[SIGN] Создание нового сертификата 'Goraw Sandbox Security'..." -ForegroundColor Cyan
    $cert = New-SelfSignedCertificate -Type CodeSigningCert `
        -Subject "CN=Goraw Sandbox Security, O=Goraw, OU=Process Governor" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -FriendlyName "Goraw Sandbox Code Signing" `
        -NotAfter (Get-Date).AddYears(10)
}

Write-Host "[SIGN] Сертификат: $($cert.Subject)" -ForegroundColor Green
Write-Host "       Отпечаток:  $($cert.Thumbprint)" -ForegroundColor Green

# 2. Подписание компонентов
$filesToSign = @(
    (Join-Path $rootDir "tools\procgov\procgov64.exe"),
    (Join-Path $rootDir "tools\procgov\procgov.exe"),
    (Join-Path $rootDir "tools\procgov\GorawJobHelper.dll"),
    (Join-Path $rootDir "box.ps1")
)

$allOk = $true
foreach ($f in $filesToSign) {
    if (Test-Path $f) {
        $leaf = Split-Path -Leaf $f
        Write-Host "[SIGN] Подписание $leaf..." -NoNewline
        try {
            $sig = Set-AuthenticodeSignature -FilePath $f -Certificate $cert -HashAlgorithm SHA256
            if ($sig.Status -eq 'Valid') {
                Write-Host " [OK] (Доверенная подпись)" -ForegroundColor Green
            } elseif ($sig.Status -eq 'UnknownError') {
                Write-Host " [OK] (Подписан: $($cert.Subject), локальный сертификат)" -ForegroundColor Green
            } else {
                Write-Host " [WARN] ($($sig.Status): $($sig.StatusMessage))" -ForegroundColor Yellow
                $allOk = $false
            }
        } catch {
            Write-Host " [FAIL] ($_)" -ForegroundColor Red
            $allOk = $false
        }
    }
}

Write-Host ""
if ($allOk) {
    Write-Host "✅ Все компоненты песочницы успешно подписаны!" -ForegroundColor Green
} else {
    Write-Host "⚠️ Некоторые компоненты требуют внимания." -ForegroundColor Yellow
}
