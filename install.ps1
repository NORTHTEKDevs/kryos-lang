# Kryos Language Installer for Windows
# Usage: irm https://raw.githubusercontent.com/FrostbyteDevTeam/kryos-lang/master/install.ps1 | iex

$ErrorActionPreference = "Stop"

$KRYOS_VERSION = "v0.3.3"
$INSTALL_DIR = "$env:USERPROFILE\.kryos\bin"
$REPO = "FrostbyteDevTeam/kryos-lang"
$BINARY = "kryos.exe"

Write-Host "Kryos Language Installer $KRYOS_VERSION"
Write-Host "----------------------------------------"

# Detect architecture
$arch = if ([System.Environment]::Is64BitOperatingSystem) { "x86_64" } else {
    Write-Error "Kryos requires a 64-bit system."
    exit 1
}

# Download URL from GitHub releases
$platform = "windows-$arch"
$download_url = "https://github.com/$REPO/releases/download/$KRYOS_VERSION/kryos-$platform.zip"

Write-Host "Downloading kryos $KRYOS_VERSION for $platform..."

# Create install directory
if (-not (Test-Path $INSTALL_DIR)) {
    New-Item -ItemType Directory -Path $INSTALL_DIR -Force | Out-Null
}

$zip_path = "$env:TEMP\kryos-$KRYOS_VERSION.zip"
$extract_path = "$env:TEMP\kryos-extract"

try {
    Invoke-WebRequest -Uri $download_url -OutFile $zip_path -UseBasicParsing
} catch {
    Write-Error "Failed to download Kryos. Check your internet connection or visit https://github.com/$REPO/releases"
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
Copy-Item -Path $src.FullName -Destination "$INSTALL_DIR\$BINARY" -Force

# Add to PATH for current user if not already present
$user_path = [System.Environment]::GetEnvironmentVariable("PATH", "User")
if ($user_path -notlike "*$INSTALL_DIR*") {
    [System.Environment]::SetEnvironmentVariable("PATH", "$user_path;$INSTALL_DIR", "User")
    Write-Host "Added $INSTALL_DIR to your PATH."
    Write-Host "Restart your terminal or run: `$env:PATH += `";$INSTALL_DIR`""
} else {
    Write-Host "$INSTALL_DIR is already in PATH."
}

# Cleanup
Remove-Item $zip_path -Force -ErrorAction SilentlyContinue
Remove-Item $extract_path -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "Kryos $KRYOS_VERSION installed to $INSTALL_DIR\$BINARY"
Write-Host ""
Write-Host "Quick start:"
Write-Host "  kryos run hello.kry"
Write-Host "  kryos repl"
Write-Host ""
Write-Host "Docs: https://github.com/$REPO/tree/master/docs"
