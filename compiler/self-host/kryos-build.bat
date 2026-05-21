@echo off
REM kryos-build.bat -- one-shot Kryos source -> Windows .exe.
REM
REM Usage: kryos-build.bat input.kry [output.exe]
REM
REM Requires:
REM   - kryos-stage1.exe on PATH (or in ..\target\bootstrap\)
REM   - kryos_rt.lib + kryos_stdlib_native.lib in ..\target\release\
REM   - MSVC Build Tools (vcvars64.bat)
REM
REM Stage-1 now uses the same KryosString ABI as stage-0, so we link
REM against the full Rust runtime (kryos_rt + kryos_stdlib_native) rather
REM than the legacy minimal C runtime (kryos_runtime.lib). This unifies
REM stage-0 / stage-1 / stage-2 onto a single runtime.

setlocal enabledelayedexpansion

if "%~1"=="" (
    echo Usage: %~nx0 input.kry [output.exe]
    exit /b 1
)

set INPUT=%~1
if "%~2"=="" (
    set OUTPUT=%~dpn1.exe
) else (
    set OUTPUT=%~2
)

set OBJ=%~dpn1.obj
set HERE=%~dp0
set STAGE1=%HERE%..\target\bootstrap\kryos-stage1
if exist "%STAGE1%.exe" set STAGE1=%STAGE1%.exe
if not exist "%STAGE1%" (
    echo error: kryos-stage1 not found at %STAGE1%
    exit /b 1
)

REM kryos-stage1 was built on git-bash without an .exe suffix; rename
REM a copy to .exe so cmd.exe will execute it.
if not exist "%HERE%..\target\bootstrap\kryos-stage1.exe" (
    copy /b /y "%STAGE1%" "%HERE%..\target\bootstrap\kryos-stage1.exe" >NUL
    set STAGE1=%HERE%..\target\bootstrap\kryos-stage1.exe
)

echo [1/2] stage-1: emitting %OBJ%
"%STAGE1%" obj "%INPUT%" -o "%OBJ%"
if errorlevel 1 exit /b 1

echo [2/2] msvc-link: %OBJ% -^> %OUTPUT%
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >NUL
link.exe /NOLOGO /SUBSYSTEM:CONSOLE /ENTRY:mainCRTStartup /OUT:"%OUTPUT%" /Brepro "%OBJ%" ^
    "%HERE%..\target\release\kryos_rt.lib" ^
    "%HERE%..\target\release\kryos_stdlib_native.lib" ^
    /NODEFAULTLIB:libcmt.lib msvcrt.lib vcruntime.lib ucrt.lib ^
    legacy_stdio_definitions.lib kernel32.lib ws2_32.lib advapi32.lib ^
    userenv.lib bcrypt.lib ntdll.lib user32.lib
if errorlevel 1 exit /b 1

echo done: %OUTPUT%
