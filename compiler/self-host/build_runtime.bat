@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >NUL
cd /d "%~dp0"
REM /GS- disables stack-buffer security cookie inserts so we don't
REM need __security_check_cookie + __GSHandlerCheck from the CRT.
cl.exe /nologo /c /O1 /Zl /GS- kryos_runtime.c || exit /b 1
lib.exe /nologo /OUT:kryos_runtime.lib kryos_runtime.obj || exit /b 1
echo BUILT
