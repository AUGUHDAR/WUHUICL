# Screenshot harness: launch WUHUICL with a debug page arg, capture the window, kill it.
param(
    [Parameter(Mandatory=$true)][string]$Page,      # --page= value ("home" / "" for none)
    [string]$Open = "",                             # --open= value (optional)
    [Parameter(Mandatory=$true)][string]$OutFile
)
$exe = Join-Path $PSScriptRoot "publish\Wuhuicl.App.exe"
$args = @()
if ($Page -ne "") { $args += "--page=$Page" }
if ($Open -ne "") { $args += "--open=$Open" }

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32 {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[Win32]::SetProcessDPIAware() | Out-Null   # 不声明就拿到逻辑坐标，位图会把高 DPI 窗口裁掉右下

$proc = Start-Process -FilePath $exe -ArgumentList $args -PassThru
Start-Sleep -Seconds 14   # 单文件解压 + 首屏装载（含在线列表拉取的余量）

$proc.Refresh()
$hwnd = $proc.MainWindowHandle
if ($hwnd -eq [IntPtr]::Zero) { Start-Sleep -Seconds 6; $proc.Refresh(); $hwnd = $proc.MainWindowHandle }

if ($hwnd -ne [IntPtr]::Zero) {
    [Win32]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 600
    $rect = New-Object Win32+RECT
    [Win32]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
    $w = $rect.Right - $rect.Left; $h = $rect.Bottom - $rect.Top
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [Win32]::PrintWindow($hwnd, $hdc, 2) | Out-Null   # PW_RENDERFULLCONTENT
    $g.ReleaseHdc($hdc); $g.Dispose()
    $bmp.Save($OutFile, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "SHOT $OutFile ${w}x${h}"
} else {
    Write-Output "NO-WINDOW"
}

# 专用工作集采样（任务管理器口径）
Start-Sleep -Seconds 2
$proc.Refresh()
$private = $proc.PrivateMemorySize64
Write-Output ("MEM Private64={0:N0}" -f $private)

Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
