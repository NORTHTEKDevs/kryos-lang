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
#   KRYOS_VERSION      -- pin a specific release tag (default: latest)
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

# Resolve release tag.
if ($env:KRYOS_VERSION) {
    $TAG = $env:KRYOS_VERSION
    Write-Host "Installing pinned version: $TAG"
} else {
    try {
        $rel = if ($AUTH) {
            Invoke-RestMethod -Uri "https://api.github.com/repos/$REPO/releases/latest" -Headers $AUTH -UseBasicParsing
        } else {
            Invoke-RestMethod -Uri "https://api.github.com/repos/$REPO/releases/latest" -UseBasicParsing
        }
        $TAG = $rel.tag_name
    } catch {
        Write-Error "Could not determine latest release. If the repo is private, set `$env:GITHUB_TOKEN to a PAT with 'repo' scope."
        exit 1
    }
    Write-Host "Latest release: $TAG"
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
