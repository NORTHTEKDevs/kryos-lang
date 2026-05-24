@echo off
REM link_stage2.bat -- compile the Windows intrinsic shim, then link a prebuilt
REM stage-2 .obj + shim into kryos-stage2.exe.
REM Usage: link_stage2.bat <stage2-obj> <out-exe>
REM Run from compiler/ .
setlocal
set OBJ=%~1
set OUT=%~2
set HERE=%~dp0
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >NUL
cl /nologo /c /O2 /Zl "%HERE%rt_shim_win.c" /Fo"%HERE%rt_shim_win.obj"
if errorlevel 1 ( echo SHIM_COMPILE_FAILED & exit /b 1 )
link.exe /NOLOGO /SUBSYSTEM:CONSOLE /ENTRY:mainCRTStartup /STACK:33554432 /Brepro /OUT:"%OUT%" "%OBJ%" ^
    "%HERE%rt_shim_win.obj" ^
    "%HERE%..\target\release\kryos_rt.lib" ^
    "%HERE%..\target\release\kryos_stdlib_native.lib" ^
    /NODEFAULTLIB:libcmt.lib msvcrt.lib vcruntime.lib ucrt.lib ^
    legacy_stdio_definitions.lib kernel32.lib ws2_32.lib advapi32.lib ^
    userenv.lib bcrypt.lib ntdll.lib user32.lib
if errorlevel 1 ( echo LINK_FAILED errorlevel %errorlevel% & exit /b 1 )
echo LINK_OK %OUT%
