# Kryos Language Installer for Windows
#
# Public-repo usage:
#   irm https://raw.githubusercontent.com/NORTHTEKDevs/kryos-lang/master/install.ps1 | iex
#
# Private-repo usage (auth required):
#   $env:GITHUB_TOKEN = "<your-PAT>"; irm <url> | iex
#
# Environment variables:
#   KRYOS_INSTALL_DIR  -- override install prefix (default: %USERPROFILE%\.kryos\bin)
#   KRYOS_VERSION      -- pin a specific release tag (default: latest release)
#   GITHUB_TOKEN       -- PAT for downloading release assets from private repos
#   GH_TOKEN           -- alternative auth env var (gh CLI compatible)

$ErrorActionPreference = "Stop"

$REPO         = "NORTHTEKDevs/kryos-lang"
$INSTALL_BIN  = if ($env:KRYOS_INSTALL_DIR) { $env:KRYOS_INSTALL_DIR } else { "$env:USERPROFILE\.kryos\bin" }
$INSTALL_ROOT = Split-Path -Parent $INSTALL_BIN
$INSTALL_LIB  = Join-Path $INSTALL_ROOT "lib"
$INSTALL_STD  = Join-Path $INSTALL_ROOT "stdlib"
$BINARY       = "kryos.exe"

# Auth header for private-repo access.
$AUTH = $null
if ($env:GITHUB_TOKEN) {
    $AUTH = @{ Authorization = "Bearer $env:GITHUB_TOKEN" }
} elseif ($env:GH_TOKEN) {
    $AUTH = @{ Authorization = "Bearer $env:GH_TOKEN" }
}

# Detect architecture
$arch = if ([System.Environment]::Is64BitOperatingSystem) { "x86_64" } else {
    Write-Error "Kryos requires a 64-bit system."
    exit 1
}

# Resolve release tag. GitHub's releases/latest is NOT used directly: the
# repo carries legacy v2.x/v4.x tags that outrank the current version line
# semantically. Query the release list (newest first) and take the first
# release whose tag is NOT one of those legacy major lines, falling back to
# a pinned floor if the API is unreachable. Override with KRYOS_VERSION to
# install any specific tag.
#
# HISTORY (2026-08-20, launch-readiness audit): this used to hardcode a
# "v1.0.0*" allowlist, which silently kept resolving to the stale
# v1.0.0-rc.2 release (published 2026-07-10) even after the project
# recalibrated its current version to 0.9.0. That allowlist is gone; the
# exclude-legacy-majors filter below is what runs now.
#
# NOTE (2026-08-31, v1.0.0 release verification): v1.0.0 was published
# 2026-09-01T04:40:34Z from commit 17afa1a1 and is the repo's Latest
# release, superseding v0.9.0. The equivalent filter in install.sh was run
# against the live releases API this day and resolves to exactly "v1.0.0"
# (not v1.0.0-rc.2, not v0.9.0), so FALLBACK_VERSION is bumped to match the
# real latest published release. Bump it again the day a later release is
# actually cut AND published -- it is only used when the API is unreachable.
$FALLBACK_VERSION = "v1.0.0"
if ($env:KRYOS_VERSION) {
    $TAG = $env:KRYOS_VERSION
    Write-Host "Installing pinned version: $TAG"
} else {
    $TAG = $null
    try {
        $rels = Invoke-RestMethod -Uri "https://api.github.com/repos/$REPO/releases?per_page=30" -Headers $AUTH -UseBasicParsing
        $TAG = ($rels | Where-Object { $_.tag_name -notlike "v2.*" -and $_.tag_name -notlike "v4.*" } | Select-Object -First 1).tag_name
    } catch { }
    if (-not $TAG) {
        $TAG = $FALLBACK_VERSION
        Write-Host "Installing default version (API unavailable, pinned floor): $TAG"
    } else {
        Write-Host "Installing latest release: $TAG"
    }
}

$platform = "windows-$arch"
$download_url = "https://github.com/$REPO/releases/download/$TAG/kryos-$platform.zip"
# Private repos reject the browser URL for API tokens; resolve the API asset
# endpoint (releases/assets/<id>) instead.
if ($AUTH) {
    try {
        $relInfo = Invoke-RestMethod -Uri "https://api.github.com/repos/$REPO/releases/tags/$TAG" -Headers $AUTH -UseBasicParsing
        $asset = $relInfo.assets | Where-Object { $_.name -eq "kryos-$platform.zip" } | Select-Object -First 1
        if ($asset) { $download_url = $asset.url }
    } catch {
        Write-Host "warn: could not resolve API asset URL; trying browser URL"
    }
}

Write-Host "Kryos Language Installer $TAG"
Write-Host "----------------------------------------"
Write-Host "Downloading kryos $TAG for $platform..."

foreach ($d in @($INSTALL_BIN, $INSTALL_LIB, $INSTALL_STD)) {
    if (-not (Test-Path $d)) { New-Item -ItemType Directory -Path $d -Force | Out-Null }
}

$zip_path     = "$env:TEMP\kryos-$TAG.zip"
$extract_path = "$env:TEMP\kryos-extract"

try {
    if ($AUTH) {
        # Asset URLs on private repos need the octet-stream Accept header
        # plus auth on the asset URL itself.
        $hdrs = $AUTH + @{ Accept = "application/octet-stream" }
        Invoke-WebRequest -Uri $download_url -Headers $hdrs -OutFile $zip_path -UseBasicParsing
    } else {
        Invoke-WebRequest -Uri $download_url -OutFile $zip_path -UseBasicParsing
    }
} catch {
    Write-Error "Failed to download Kryos. Check your network or visit https://github.com/$REPO/releases"
    exit 1
}

Write-Host "Extracting..."
if (Test-Path $extract_path) { Remove-Item $extract_path -Recurse -Force }
Expand-Archive -Path $zip_path -DestinationPath $extract_path -Force

# Copy binary
$src = Get-ChildItem -Path $extract_path -Filter $BINARY -Recurse | Select-Object -First 1
if (-not $src) {
    Write-Error "Binary not found in archive."
    exit 1
}
Copy-Item -Path $src.FullName -Destination "$INSTALL_BIN\$BINARY" -Force

# Copy runtime static libs if present in archive
foreach ($libname in @("kryos_rt.lib", "libkryos_rt.a", "kryos_stdlib_native.lib", "libkryos_stdlib_native.a")) {
    $libsrc = Get-ChildItem -Path $extract_path -Filter $libname -Recurse | Select-Object -First 1
    if ($libsrc) {
        Copy-Item -Path $libsrc.FullName -Destination "$INSTALL_LIB\$libname" -Force
    }
}

# Copy stdlib (.kry sources) if present
$stdsrc = Join-Path $extract_path "stdlib"
if (Test-Path $stdsrc) {
    Copy-Item -Path "$stdsrc\*" -Destination $INSTALL_STD -Recurse -Force
}

# Add to PATH for current user if not already present
$user_path = [System.Environment]::GetEnvironmentVariable("PATH", "User")
if ($user_path -notlike "*$INSTALL_BIN*") {
    [System.Environment]::SetEnvironmentVariable("PATH", "$user_path;$INSTALL_BIN", "User")
    Write-Host "Added $INSTALL_BIN to your PATH."
    Write-Host "Restart your terminal or run: `$env:PATH += `";$INSTALL_BIN`""
} else {
    Write-Host "$INSTALL_BIN is already in PATH."
}

# Cleanup
Remove-Item $zip_path -Force -ErrorAction SilentlyContinue
Remove-Item $extract_path -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "Kryos $TAG installed to $INSTALL_BIN\$BINARY"
Write-Host ""
Write-Host "Quick start:"
Write-Host "  kryos run hello.kry"
Write-Host "  kryos repl"
Write-Host ""
Write-Host "Docs: https://github.com/$REPO/tree/master/docs"
