
::查找文件，文件为wechat.exe，找到一个就停止，如果要查找所有的a.exe，则去掉goto语句
@echo off
::A B C D E这些字母代表盘符
set "FileName=WeChat.exe"
for %%a. in (A B C D E F G H I J K L M N) do (
    if exist %%a:\ (
        cd /d %%a:\
        for /f "delims=" %%b in ('dir /s /a-d /b %FileName%') do (
            if exist "%%b" (
                echo %%b 
                goto deal
            )
         )
      )
)

:deal
pause

TASKKILL /F /IM wechat.exe
start "" %%b