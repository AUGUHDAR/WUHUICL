using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.RegularExpressions;

namespace Wuhuicl.Core.Minecraft;

/// <summary>
/// 游戏进程生命周期。刻意不把子进程塞进启动器的 Job Object：
/// 参考实现的事故是启动器自动退出时 Java 被连带杀掉（台账 RUN-1）。
/// </summary>
public sealed class GameLauncher : IDisposable
{
    Process? _proc;
    readonly object _gate = new();
    readonly Queue<string> _lines = new();
    const int MaxLines = 4000;
    public event Action<string, bool>? LineReceived;      // (文本, 是否 stderr)
    public event Action<int>? Exited;
    public string GameDir { get; private set; } = "";
    public int? ExitCode { get; private set; }
    public bool IsRunning => _proc is { HasExited: false };
    public int Pid => _proc is null ? 0 : SafePid(_proc);
    public int? LocalPort { get; private set; }

    static int SafePid(Process p) { try { return p.Id; } catch { return 0; } }

    public bool Start(LaunchPlan plan)
    {
        if (IsRunning) throw new InvalidOperationException("已有游戏在运行");
        if (string.IsNullOrEmpty(plan.Java) || !File.Exists(plan.Java))
            throw new FileNotFoundException("Java 可执行文件不存在：" + plan.Java);
        GameDir = plan.WorkingDirectory;

        var psi = new ProcessStartInfo(plan.Java)
        {
            WorkingDirectory = plan.WorkingDirectory,
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            RedirectStandardInput = true,
            CreateNoWindow = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8,
        };
        foreach (var a in plan.Args) psi.ArgumentList.Add(a);

        _proc = Process.Start(psi) ?? throw new InvalidOperationException("进程启动失败");
        _proc.EnableRaisingEvents = true;
        _proc.Exited += (_, _) =>
        {
            try { ExitCode = _proc.ExitCode; } catch { ExitCode = -1; }
            Exited?.Invoke(ExitCode ?? -1);
        };
        Pump(_proc.StandardOutput, false);
        Pump(_proc.StandardError, true);
        return true;
    }

    void Pump(StreamReader reader, bool isErr)
    {
        Task.Run(async () =>
        {
            try
            {
                string? line;
                while ((line = await reader.ReadLineAsync()) is not null)
                {
                    var text = Clean(line);
                    lock (_gate)
                    {
                        _lines.Enqueue(text);
                        while (_lines.Count > MaxLines) _lines.Dequeue();
                    }
                    DetectPort(text);
                    try { LineReceived?.Invoke(text, isErr); } catch { }
                }
            }
            catch { /* 进程结束时管道会抛，正常 */ }
        });
    }

    static readonly Regex Port1 = new(@"Starting local server on port (\d+)|Dedicated game port (\d+)", RegexOptions.IgnoreCase);
    static readonly Regex Port2 = new(@"开.*局域网游戏.*端口 (\d+)|本地游戏已在端口 (\d+)", RegexOptions.IgnoreCase);

    void DetectPort(string text)
    {
        var m = Port1.Match(text);
        if (m.Success && int.TryParse(m.Groups[1].Value, out var p1)) { LocalPort = p1; return; }
        m = Port2.Match(text);
        if (m.Success)
        {
            for (int g = 1; g <= 2; g++)
                if (int.TryParse(m.Groups[g].Value, out var p2)) { LocalPort = p2; return; }
        }
    }

    static string Clean(string line)
    {
        // 命令行里可能带 token，日志里一律打码（台账 AUTH 系列：日志泄露凭据）
        var s = line.TrimEnd('\r');
        s = Regex.Replace(s, @"(--(accessToken|session|uuid)\s+)\S+", "$1***");
        return s;
    }

    public string Tail(int lines = 400)
    {
        lock (_gate)
            return string.Join('\n', _lines.Skip(Math.Max(0, _lines.Count - lines)));
    }

    /// <summary>写入 /kill 之类命令；游戏未就绪时忽略。</summary>
    public void SendCommand(string cmd)
    {
        try { _proc?.StandardInput.WriteLine(cmd); _proc?.StandardInput.Flush(); } catch { }
    }

    public void Stop(int gracefulMs = 8000)
    {
        if (_proc is null || _proc.HasExited) return;
        try
        {
            if (IsRunning && gracefulMs > 0)
                _proc.WaitForExit(gracefulMs);
            if (_proc is { HasExited: false }) _proc.Kill(entireProcessTree: true);
        }
        catch { }
    }

    public void Dispose()
    {
        try { _proc?.Dispose(); } catch { }
        _proc = null;
    }
}
