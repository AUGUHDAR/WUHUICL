# 按“窗口标题含 WUHUICL 且可见”定位主窗口并最小化/还原。
# 不用 Get-Process MainWindowHandle：最小化后它可能指向进程里别的窗口（实测踩坑）。
# 用法：powershell -File scripts\winctl.ps1 -Action minimize|restore
param(
    [Parameter(Mandatory = $true)][ValidateSet('minimize', 'restore')][string]$Action
)

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WuhuWin {
    private delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] private static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] private static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);

    public static IntPtr Find(uint targetPid, bool wantIconic) {
        IntPtr found = IntPtr.Zero;
        EnumProc cb = delegate (IntPtr h, IntPtr l) {
            uint pid;
            GetWindowThreadProcessId(h, out pid);
            if (pid == targetPid && IsWindowVisible(h) && IsIconic(h) == wantIconic) {
                var sb = new StringBuilder(256);
                GetWindowText(h, sb, 256);
                if (sb.ToString().Contains("WUHUICL")) { found = h; return false; }
            }
            return true;
        };
        EnumWindows(cb, IntPtr.Zero);
        return found;
    }
}
'@

$p = Get-Process wuhuicl -ErrorAction Stop
if ($Action -eq 'minimize') {
    $h = [WuhuWin]::Find($p.Id, $false)
    if ($h -eq [IntPtr]::Zero) { Write-Output "NOTFOUND-visible"; exit 1 }
    [void][WuhuWin]::ShowWindow($h, 6)
    Write-Output "MINIMIZED $h"
} else {
    # 还原：优先找 iconic 的真主窗口；找不到再退回可见窗口
    $h = [WuhuWin]::Find($p.Id, $true)
    if ($h -eq [IntPtr]::Zero) { $h = [WuhuWin]::Find($p.Id, $false) }
    if ($h -eq [IntPtr]::Zero) { Write-Output "NOTFOUND-iconic"; exit 1 }
    [void][WuhuWin]::ShowWindow($h, 9)
    Start-Sleep -Milliseconds 500
    [void][WuhuWin]::SetForegroundWindow($h)
    Write-Output "RESTORED $h"
}
