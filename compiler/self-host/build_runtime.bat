@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >NUL
cd /d "%~dp0"
cl.exe /nologo /c /O1 /Zl kryos_runtime.c || exit /b 1
lib.exe /nologo /OUT:kryos_runtime.lib kryos_runtime.obj || exit /b 1
echo BUILT
