# Link a self-host-emitted .obj into a Windows .exe against the Rust runtime.
# Usage: link-win.ps1 <input.obj> <output.exe>
param([Parameter(Mandatory=$true)][string]$Obj, [Parameter(Mandatory=$true)][string]$Out)
$ErrorActionPreference = "Stop"
$root = "C:\Users\Krist\projects\active\kryos-lang\compiler"
Set-Location $root

$vcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
$envBlock = & cmd /c "`"$vcvars`" >NUL && set"
foreach ($line in $envBlock) {
    if ($line -match "^([^=]+)=(.*)$") { Set-Item -Path "env:$($matches[1])" -Value $matches[2] }
}

$rt     = "$root\target\release\kryos_rt.lib"
$stdlib = "$root\target\release\kryos_stdlib_native.lib"
$shim   = "$root\self-host\rt_shim_win.obj"

$linkArgs = @(
    "/OUT:$Out", "/NOLOGO", "/SUBSYSTEM:CONSOLE", "/ENTRY:mainCRTStartup",
    "/STACK:268435456,268435456", "/MAP", "/DYNAMICBASE:NO", "/FIXED",
    $Obj, $shim, $rt, $stdlib,
    "/NODEFAULTLIB:libcmt.lib", "msvcrt.lib", "vcruntime.lib", "ucrt.lib",
    "legacy_stdio_definitions.lib", "kernel32.lib", "user32.lib", "advapi32.lib",
    "userenv.lib", "ws2_32.lib", "ntdll.lib", "bcrypt.lib", "synchronization.lib"
)
Write-Host "Linking $Out ..."
& link.exe @linkArgs
if ($LASTEXITCODE -ne 0) { Write-Error "link.exe failed: $LASTEXITCODE"; exit $LASTEXITCODE }
Get-Item $Out | Select-Object Name,Length,LastWriteTime | Format-List
