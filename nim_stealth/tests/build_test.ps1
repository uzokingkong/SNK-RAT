## nim_stealth/tests/build_test.ps1
## 관리자 PowerShell에서 실행
## Usage: .\tests\build_test.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent

Write-Host "[*] nim_stealth 테스트 빌드 시작..." -ForegroundColor Cyan
Write-Host "[*] 루트: $root" -ForegroundColor Cyan

Push-Location $root

try {
    $args = @(
        "c",
        "-d:release",
        "-d:danger",
        "--opt:speed",
        "--threads:off",
        "--gc:orc",
        "--passL:-lpsapi",
        "--passL:-lntdll",
        "-o:tests\test_all.exe",
        "tests\test_all.nim"
    )

    Write-Host "[*] nim $($args -join ' ')" -ForegroundColor DarkGray
    & nim @args

    if ($LASTEXITCODE -eq 0) {
        Write-Host "`n[+] 빌드 성공! → tests\test_all.exe" -ForegroundColor Green
        Write-Host "[*] 실행 중 (관리자 권한 권장)..." -ForegroundColor Cyan
        & ".\tests\test_all.exe"
    } else {
        Write-Host "`n[!] 빌드 실패 (exit=$LASTEXITCODE)" -ForegroundColor Red
    }
} finally {
    Pop-Location
}
