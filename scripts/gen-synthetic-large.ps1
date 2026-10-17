# Generate the two synthetic fixtures that need byte-exact control
# (partial-tail: no trailing newline; huge-line: ~2 MiB of generated content).
$ErrorActionPreference = 'Stop'
$dir = Join-Path (Split-Path -Parent $PSScriptRoot) 'fixtures\synthetic'
$enc = [System.Text.UTF8Encoding]::new($false)

# --- partial-tail.jsonl: last line truncated mid-token, NO trailing newline ---
$l1 = '{"kind":0,"v":{"version":3,"creationDate":1006,"sessionId":"syn-tail","requests":[{"message":{"text":"u1"},"response":[{"value":"a1"}]}]}}'
$l2 = '{"kind":1,"k":["customTit'
[System.IO.File]::WriteAllText((Join-Path $dir 'partial-tail.jsonl'), ($l1 + "`n" + $l2), $enc)

# --- huge-line.jsonl: one oversized set + one oversized push ---
$bigA = 'x' * 1048576   # 1 MiB string in the `result` object
$bigB = 'b' * 1048576   # 1 MiB string as a response-part value
$h1 = '{"kind":0,"v":{"version":3,"creationDate":1008,"sessionId":"syn-huge","requests":[{"requestId":"r1","message":{"text":"u1"},"response":[{"value":"a1"}]}]}}'
$h2 = '{"kind":1,"k":["requests",0,"result"],"v":{"huge":"' + $bigA + '"}}'
$h3 = '{"kind":2,"k":["requests"],"v":[{"requestId":"r2","message":{"text":"u2"},"response":[{"value":"' + $bigB + '"}]}]}'
[System.IO.File]::WriteAllText((Join-Path $dir 'huge-line.jsonl'), ($h1 + "`n" + $h2 + "`n" + $h3 + "`n"), $enc)

# --- verify ---
$pt = Join-Path $dir 'partial-tail.jsonl'
$fs = [System.IO.File]::Open($pt, 'Open', 'Read')
try {
    $fs.Seek(-1, [System.IO.SeekOrigin]::End) | Out-Null
    $lastByte = $fs.ReadByte()
} finally { $fs.Dispose() }
"partial-tail.jsonl bytes={0} lastByte={1} (expected != 10)" -f (Get-Item $pt).Length, $lastByte
"huge-line.jsonl bytes={0}" -f (Get-Item (Join-Path $dir 'huge-line.jsonl')).Length
