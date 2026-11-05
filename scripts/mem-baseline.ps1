# 进程树内存快照：主进程 + 全部 WebView2 子进程的 WorkingSet / PrivateBytes 汇总。
# 用法：
#   powershell -File scripts/mem-baseline.ps1                     # 自动找 wuhuicl 主进程
#   powershell -File scripts/mem-baseline.ps1 -Name wuhuicl       # 指定主进程名
#   powershell -File scripts/mem-baseline.ps1 -RootPid 12345      # 指定主进程 pid
# 输出末行 TOTAL: working=<MB> private=<MB> procs=<n>  便于脚本化对比。
param(
    [string]$Name = "wuhuicl",
    [int]$RootPid = 0
)

$ErrorActionPreference = "Stop"

$all = Get-CimInstance Win32_Process -Filter "Name='wuhuicl.exe' OR Name='msedgewebview2.exe' OR Name='msedge.exe'"
if ($RootPid -le 0) {
    $mains = @($all | Where-Object { $_.Name -ieq "wuhuicl.exe" })
    if ($mains.Count -eq 0) { Write-Output "NO-PROCESS"; exit 1 }
    # 多个实例时取启动时间最早的一个，避免把误开的多份都算进来
    $root = $mains | Sort-Object CreationDate | Select-Object -First 1
    $RootPid = $root.ProcessId
} else {
    $root = $all | Where-Object { $_.ProcessId -eq $RootPid }
    if (-not $root) { Write-Output "NO-PROCESS"; exit 1 }
}

# BFS 收集整棵子树（WebView2 的 gpu/renderer/utility 可能挂在 browser 子进程下）
$byParent = @{}
foreach ($p in $all) {
    if (-not $byParent.ContainsKey([int]$p.ParentProcessId)) { $byParent[[int]$p.ParentProcessId] = @() }
    $byParent[[int]$p.ParentProcessId] += $p
}
$ids = New-Object System.Collections.Generic.HashSet[int]
[void]$ids.Add([int]$RootPid)
$queue = [System.Collections.Queue]::new(); $queue.Enqueue([int]$RootPid)
while ($queue.Count -gt 0) {
    $cur = $queue.Dequeue()
    if ($byParent.ContainsKey($cur)) {
        foreach ($c in $byParent[$cur]) {
            if ($ids.Add([int]$c.ProcessId)) { $queue.Enqueue([int]$c.ProcessId) }
        }
    }
}

$rows = @()
foreach ($p in $all) {
    if (-not $ids.Contains([int]$p.ProcessId)) { continue }
    $gp = Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue
    if (-not $gp) { continue }
    $rows += [pscustomobject]@{
        Pid     = $p.ProcessId
        Name    = $p.Name
        Role    = if ($p.ProcessId -eq $RootPid) { "main" } else { "webview" }
        WS_MB   = [math]::Round($gp.WorkingSet64 / 1MB, 1)
        Priv_MB = [math]::Round($gp.PrivateMemorySize64 / 1MB, 1)
    }
}
$rows | Sort-Object Priv_MB -Descending | Format-Table -AutoSize
$ws = ($rows | Measure-Object WS_MB -Sum).Sum
$priv = ($rows | Measure-Object Priv_MB -Sum).Sum
Write-Output ("TOTAL: working={0} private={1} procs={2} root={3}" -f [math]::Round($ws, 1), [math]::Round($priv, 1), $rows.Count, $RootPid)
