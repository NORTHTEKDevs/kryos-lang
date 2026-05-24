@echo off
REM link_stage2.bat -- link a prebuilt stage-2 .obj into kryos-stage2.exe
REM Usage: link_stage2.bat <obj> <out-exe>
REM Run from compiler/ . Uses the same lib set as kryos-build.bat.
setlocal
set OBJ=%~1
set OUT=%~2
set HERE=%~dp0
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >NUL
link.exe /NOLOGO /SUBSYSTEM:CONSOLE /ENTRY:mainCRTStartup /STACK:33554432 /Brepro /OUT:"%OUT%" "%OBJ%" ^
    "%HERE%..\target\release\kryos_rt.lib" ^
    "%HERE%..\target\release\kryos_stdlib_native.lib" ^
    /NODEFAULTLIB:libcmt.lib msvcrt.lib vcruntime.lib ucrt.lib ^
    legacy_stdio_definitions.lib kernel32.lib ws2_32.lib advapi32.lib ^
    userenv.lib bcrypt.lib ntdll.lib user32.lib
if errorlevel 1 (
    echo LINK_FAILED errorlevel %errorlevel%
    exit /b 1
)
echo LINK_OK %OUT%
