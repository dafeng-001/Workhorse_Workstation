@echo off
setlocal
set PID=47832
set NEW=C:\Users\10259\XiaomiMiMoProjects\牛马工作台\data\update\Workhorse_Workstation-win64.exe
set TARGET=C:\Users\10259\XiaomiMiMoProjects\牛马工作台\牛马工作台.exe
set SCRIPT=C:\Users\10259\XiaomiMiMoProjects\牛马工作台\apply_update.bat
:wait
timeout /t 1 /nobreak >nul
tasklist /fi "PID eq %PID%" 2>nul | find "%PID%" >nul
if not errorlevel 1 goto wait
if exist "%TARGET%.old" del /f /q "%TARGET%.old" >nul 2>&1
if exist "%TARGET%" move /y "%TARGET%" "%TARGET%.old" >nul
move /y "%NEW%" "%TARGET%" >nul
start "" "%TARGET%"
del /f /q "%SCRIPT%" >nul 2>&1
exit
