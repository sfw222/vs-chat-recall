# Install a transparent `recall` shim into a PATH directory (defaults to
# ~/.cargo/bin, which is on PATH whenever cargo is). The shim always runs the
# repo's current release build, so `cargo build --release` stays the only
# update step.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $repo 'target\release\recall.exe'
$shimDir = Join-Path $env:USERPROFILE '.cargo\bin'
$shim = Join-Path $shimDir 'recall.cmd'

if (-not (Test-Path $exe)) { throw "先构建: $exe 不存在（cargo build --release）" }
New-Item -ItemType Directory -Force -Path $shimDir | Out-Null
$content = "@echo off`r`n`"$exe`" %*`r`n"
[System.IO.File]::WriteAllText($shim, $content, [System.Text.Encoding]::ASCII)
"installed: $shim -> $exe"
Get-Content $shim
