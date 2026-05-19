@echo off
REM Args: %1 = input .obj, %2 = output .exe
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >NUL
link.exe /NOLOGO /SUBSYSTEM:CONSOLE /ENTRY:mainCRTStartup /OUT:%2 %1 "%~dp0kryos_runtime.lib" kernel32.lib
