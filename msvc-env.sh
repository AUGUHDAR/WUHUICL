# MSVC + Windows SDK 环境（Git Bash source 用）
# 用法：source "D:/桌面/WUHUICL/wuhuicl-rt/msvc-env.sh" && cargo check
MSVC="C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207"
SDK="C:/Program Files (x86)/Windows Kits/10"
SDKVER="10.0.19041.0"
export PATH="/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64:/c/Program Files (x86)/Windows Kits/10/bin/10.0.19041.0/x64:$PATH"
export INCLUDE="$MSVC/include;$SDK/include/$SDKVER/ucrt;$SDK/include/$SDKVER/um;$SDK/include/$SDKVER/shared;$SDK/include/$SDKVER/winrt;$SDK/include/$SDKVER/cppwinrt"
export LIB="$MSVC/lib/x64;$SDK/lib/$SDKVER/ucrt/x64;$SDK/lib/$SDKVER/um/x64"
