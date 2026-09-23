<#
.SYNOPSIS
Builds Lyonbrown with a process-local Windows C/C++ toolchain environment.

.EXAMPLE
./scripts/build-windows.ps1 -CleanLlama

.EXAMPLE
./scripts/build-windows.ps1 -CargoArgs @('test', '--workspace')
#>
[CmdletBinding()]
param(
  [string]$VsInstallPath,
  [string]$LlvmBinPath,
  [switch]$CleanLlama,
  [string[]]$CargoArgs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Find-VsInstallPath {
  if ($VsInstallPath) {
    return $VsInstallPath
  }

  $vswhere = Join-Path ${env:ProgramFiles(x86)} `
    'Microsoft Visual Studio\Installer\vswhere.exe'
  if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
    throw 'Visual Studio locator (vswhere.exe) was not found.'
  }

  $installPath = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath
  if ($LASTEXITCODE -ne 0 -or -not $installPath) {
    throw 'Visual Studio with the MSVC x64 tools was not found.'
  }

  return $installPath.Trim()
}

function Find-LlvmBinPath([string]$VisualStudioPath) {
  if ($LlvmBinPath) {
    $candidates = @($LlvmBinPath)
  } else {
    $candidates = @()
    if ($env:LIBCLANG_PATH) {
      $candidates += $env:LIBCLANG_PATH
    }

    $clang = Get-Command clang.exe -ErrorAction SilentlyContinue
    if ($clang) {
      $candidates += Split-Path -Parent $clang.Source
    }

    $candidates += @(
      (Join-Path $env:LOCALAPPDATA 'LLVM\bin'),
      (Join-Path $env:ProgramFiles 'LLVM\bin'),
      (Join-Path ${env:ProgramFiles(x86)} 'LLVM\bin'),
      (Join-Path $VisualStudioPath 'VC\Tools\Llvm\x64\bin')
    )
  }

  foreach ($candidate in $candidates) {
    if ($candidate -and
      (Test-Path -LiteralPath (Join-Path $candidate 'libclang.dll') -PathType Leaf)) {
      return $candidate
    }
  }

  throw 'libclang.dll was not found. Pass -LlvmBinPath or install LLVM.'
}

$resolvedVsPath = Find-VsInstallPath
$devShell = Join-Path $resolvedVsPath 'Common7\Tools\Launch-VsDevShell.ps1'
if (-not (Test-Path -LiteralPath $devShell -PathType Leaf)) {
  throw "Visual Studio developer shell was not found at: $devShell"
}

& $devShell -VsInstallationPath $resolvedVsPath -Arch amd64 `
  -HostArch amd64 -SkipAutomaticLocation -NoLogo

$env:LIBCLANG_PATH = Find-LlvmBinPath $resolvedVsPath
$env:CMAKE_GENERATOR = 'Ninja'

if (-not (Get-Command ninja.exe -ErrorAction SilentlyContinue)) {
  throw 'Ninja was not found after entering the Visual Studio developer shell.'
}

if (-not $CargoArgs) {
  $CargoArgs = @('build', '-p', 'agent-cli')
}

Write-Host "Visual Studio: $resolvedVsPath"
Write-Host "LIBCLANG_PATH: $env:LIBCLANG_PATH"
Write-Host "CMAKE_GENERATOR: $env:CMAKE_GENERATOR"

if ($CleanLlama) {
  & cargo clean -p llama-cpp-sys-2
  if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
  }
}

& cargo @CargoArgs
exit $LASTEXITCODE
