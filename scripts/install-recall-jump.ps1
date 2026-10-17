# Sideload the local `recall-jump` extension into the user extensions dir.
# Manual install = copy the folder + reload window; uninstall = delete it.
$ErrorActionPreference = 'Stop'
$src = Join-Path (Split-Path -Parent $PSScriptRoot) 'extensions\recall-jump'
$extRoot = Join-Path $env:USERPROFILE '.vscode\extensions'
$dst = Join-Path $extRoot 'local.recall-jump-0.0.2'

# remove any older sideloaded version
Get-ChildItem $extRoot -Directory -Filter 'local.recall-jump-*' -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -ne $dst } |
    ForEach-Object { Remove-Item $_.FullName -Recurse -Force }

New-Item -ItemType Directory -Force -Path $dst | Out-Null
Copy-Item -Path (Join-Path $src '*') -Destination $dst -Recurse -Force
"installed: $dst"
Get-ChildItem $dst | Select-Object Name, Length
"reload VS Code window (or restart) to activate the extension."
