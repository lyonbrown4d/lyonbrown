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

function Test-ReparsePoint([string]$Path) {
  $item = Get-Item -LiteralPath $Path
  return [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint)
}

function Repair-LlamaCMakeProjects {
  $targetRoot = Join-Path $PSScriptRoot '..\target'
  if (-not (Test-Path -LiteralPath $targetRoot -PathType Container)) {
    return
  }
  if (Test-ReparsePoint $targetRoot) {
    throw "Refusing reparse-point target directory: $targetRoot"
  }

  $resolvedTarget = (Resolve-Path -LiteralPath $targetRoot).Path.TrimEnd('\')
  $targetPrefix = $resolvedTarget + [IO.Path]::DirectorySeparatorChar
  $trustedSource = $null

  $profiles = Get-ChildItem -LiteralPath $targetRoot -Directory
  foreach ($profile in $profiles) {
    if ($profile.Attributes -band [IO.FileAttributes]::ReparsePoint) {
      continue
    }
    $buildRoot = Join-Path $profile.FullName 'build'
    if (-not (Test-Path -LiteralPath $buildRoot -PathType Container)) {
      continue
    }
    if (Test-ReparsePoint $buildRoot) {
      continue
    }

    $packages = Get-ChildItem -LiteralPath $buildRoot -Directory `
      -Filter 'llama-cpp-sys-2-*'
    foreach ($package in $packages) {
      if ($package.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        continue
      }
      $cmakeBuild = Join-Path $package.FullName 'out\build'
      $packageOut = Join-Path $package.FullName 'out'
      $cache = Join-Path $cmakeBuild 'CMakeCache.txt'
      if (-not (Test-Path -LiteralPath $cache -PathType Leaf)) {
        continue
      }
      if ((Test-ReparsePoint $packageOut) -or
        (Test-ReparsePoint $cache)) {
        continue
      }

      if (Test-ReparsePoint $cmakeBuild) {
        continue
      }

      $resolvedBuild = (Resolve-Path -LiteralPath $cmakeBuild).Path
      if (-not $resolvedBuild.StartsWith(
          $targetPrefix,
          [StringComparison]::OrdinalIgnoreCase
        )) {
        throw "Refusing llama.cpp build directory outside target: $resolvedBuild"
      }

      $entries = Get-Content -LiteralPath $cache
      $sourceEntry = $entries | Where-Object {
        $_.StartsWith('CMAKE_HOME_DIRECTORY:INTERNAL=')
      } | Select-Object -First 1
      $generatorEntry = $entries | Where-Object {
        $_ -match '^CMAKE_GENERATOR:(INTERNAL|UNINITIALIZED)='
      } | Select-Object -First 1
      if (-not $sourceEntry -or -not $generatorEntry) {
        continue
      }

      $ninjaProject = Join-Path $cmakeBuild 'build.ninja'
      $visualStudioProject = Join-Path $cmakeBuild 'INSTALL.vcxproj'
      if ((Test-Path -LiteralPath $ninjaProject -PathType Leaf) -or
        (Test-Path -LiteralPath $visualStudioProject -PathType Leaf)) {
        continue
      }

      $generator = $generatorEntry.Split('=', 2)[1]
      if (-not ($generator.StartsWith('Ninja') -or
          $generator.StartsWith('Visual Studio'))) {
        Write-Warning "Skipping unsupported CMake generator: $generator"
        continue
      }

      $source = $sourceEntry.Split('=', 2)[1]
      if (-not (Test-Path -LiteralPath $source -PathType Container)) {
        Write-Warning "Skipping stale llama.cpp CMake source: $source"
        continue
      }
      $resolvedSource = (Resolve-Path -LiteralPath $source).Path
      if (-not $trustedSource) {
        $metadataText = & cargo metadata --format-version 1 --locked `
          --filter-platform x86_64-pc-windows-msvc
        if ($LASTEXITCODE -ne 0) {
          throw 'Failed to resolve the locked llama-cpp-sys-2 package.'
        }
        $metadata = ($metadataText -join [Environment]::NewLine) |
          ConvertFrom-Json
        $llamaPackage = $metadata.packages | Where-Object {
          $_.name -eq 'llama-cpp-sys-2'
        } | Select-Object -First 1
        if (-not $llamaPackage) {
          throw 'The locked llama-cpp-sys-2 package was not found.'
        }
        $manifestRoot = Split-Path -Parent $llamaPackage.manifest_path
        $trustedSource = (Resolve-Path -LiteralPath `
            (Join-Path $manifestRoot 'llama.cpp')).Path
      }
      if (-not $resolvedSource.Equals(
          $trustedSource,
          [StringComparison]::OrdinalIgnoreCase
        )) {
        Write-Warning "Skipping unrelated llama.cpp CMake source: $resolvedSource"
        continue
      }

      Write-Host "Repairing incomplete llama.cpp CMake project: $cmakeBuild"
      & cmake -S $trustedSource -B $resolvedBuild
      if ($LASTEXITCODE -ne 0) {
        Write-Warning "Failed to repair llama.cpp CMake project: $cmakeBuild"
      }
    }
  }
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
  $CargoArgs = @('build', '-p', 'agent-cli', '-p', 'agentd')
}

Write-Host "Visual Studio: $resolvedVsPath"
Write-Host "LIBCLANG_PATH: $env:LIBCLANG_PATH"
Write-Host "CMAKE_GENERATOR: $env:CMAKE_GENERATOR"

Repair-LlamaCMakeProjects

if ($CleanLlama) {
  & cargo clean -p llama-cpp-sys-2
  if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
  }
}

& cargo @CargoArgs
exit $LASTEXITCODE
