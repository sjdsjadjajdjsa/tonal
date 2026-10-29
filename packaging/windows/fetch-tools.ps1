# Downloads mpv.exe and yt-dlp.exe into the folder given as the first argument.
param([Parameter(Mandatory = $true)][string]$Dest)
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory $Dest -Force | Out-Null
$tmp = Join-Path $env:RUNNER_TEMP "tonal-tools"
New-Item -ItemType Directory $tmp -Force | Out-Null

Invoke-WebRequest -Uri "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" -OutFile (Join-Path $Dest "yt-dlp.exe")

$release = Invoke-RestMethod -Uri "https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/releases/latest" -Headers @{ "User-Agent" = "tonal-release" }
$asset = $release.assets | Where-Object { $_.name -match '^mpv-x86_64-\d{8}-git-[0-9a-f]+\.7z$' } | Select-Object -First 1
if (-not $asset) { throw "No mpv build found in $($release.tag_name)" }
$archive = Join-Path $tmp $asset.name
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $archive
7z x $archive "-o$tmp\mpv" -y | Out-Null
Copy-Item "$tmp\mpv\mpv.exe" $Dest
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/mpv-player/mpv/master/LICENSE.GPL" -OutFile (Join-Path $Dest "mpv-LICENSE.GPL.txt")
Set-Content -Path (Join-Path $Dest "MPV-VERSION.txt") -Value "$($asset.name) from $($release.html_url)"
Write-Host "Bundled $($asset.name) and yt-dlp"
