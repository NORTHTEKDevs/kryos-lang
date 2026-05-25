@echo off
REM linkmap_stage2.bat -- same link as link_stage2.bat but emits a /MAP file
REM so we can resolve a faulting RVA to a function symbol.
REM Usage: linkmap_stage2.bat <stage2-obj> <out-exe> <map-out>
setlocal
set OBJ=%~1
set OUT=%~2
set MAP=%~3
set HERE=%~dp0
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >NUL
cl /nologo /c /O2 /Zl "%HERE%rt_shim_win.c" /Fo"%HERE%rt_shim_win.obj"
if errorlevel 1 ( echo SHIM_COMPILE_FAILED & exit /b 1 )
link.exe /NOLOGO /SUBSYSTEM:CONSOLE /ENTRY:mainCRTStartup /STACK:33554432 /Brepro /MAP:"%MAP%" /OUT:"%OUT%" "%OBJ%" ^
    "%HERE%rt_shim_win.obj" ^
    "%HERE%..\target\release\kryos_rt.lib" ^
    "%HERE%..\target\release\kryos_stdlib_native.lib" ^
    /NODEFAULTLIB:libcmt.lib msvcrt.lib vcruntime.lib ucrt.lib ^
    legacy_stdio_definitions.lib kernel32.lib ws2_32.lib advapi32.lib ^
    userenv.lib bcrypt.lib ntdll.lib user32.lib
if errorlevel 1 ( echo LINK_FAILED errorlevel %errorlevel% & exit /b 1 )
echo LINK_OK %OUT%
