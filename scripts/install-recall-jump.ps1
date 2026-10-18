# Sideload the local `recall-jump` extension into the user extensions dir.
# Manual install = copy the folder + reload window; uninstall = delete it.
# For a Marketplace install instead, just search "Recall Jump" in VS Code.
$ErrorActionPreference = 'Stop'
$src = Join-Path (Split-Path -Parent $PSScriptRoot) 'extensions\recall-jump'
$extRoot = Join-Path $env:USERPROFILE '.vscode\extensions'

$manifest = Get-Content -Raw (Join-Path $src 'package.json') | ConvertFrom-Json
$dst = Join-Path $extRoot "$($manifest.publisher).$($manifest.name)-$($manifest.version)"

# remove any older sideloaded version (including the pre-1.0 `local.` layout)
Get-ChildItem $extRoot -Directory -Filter '*recall-jump-*' -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -ne $dst } |
    ForEach-Object { Remove-Item $_.FullName -Recurse -Force }

New-Item -ItemType Directory -Force -Path $dst | Out-Null
Copy-Item -Path (Join-Path $src '*') -Destination $dst -Recurse -Force
"installed: $dst"
Get-ChildItem $dst | Select-Object Name, Length
"reload VS Code window (or restart) to activate the extension."
