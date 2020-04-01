#!/bin/bash
counts=1
regex="[0-9]"
chmod +x ./counts.sh
echo "你想打开多少个微信" 
read aNum

while [[ $aNum != $regex ]] #值做判断
do
    echo "输入不正确，重新输入数字吧"
    read aNum
done
    echo "棒棒，开始执行多开"
    while(( $aNum>=$counts ))
    do
        nohup /Applications/WeChat.app/Contents/MacOS/WeChat > /dev/null 2>&1 &
        echo "成功打开了$counts 个微信"
        let "counts++"
    done
        let "counts--"
        echo "执行完毕，一共打开了$counts 个微信，尽情享受吧！"
