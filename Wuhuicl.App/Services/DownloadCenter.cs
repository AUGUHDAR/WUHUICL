using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Net;
using System.Net.Http.Headers;
using System.Security.Cryptography;
using System.ComponentModel;
using System.Net.Http;
using System.IO;

namespace Wuhuicl.App.Services;

public enum DlKind { Version, Modpack, Download }

public enum DlStatus { Running, Paused, Done, Error }

/// <summary>250ms 节流的进度回调帧（taskId/stage/progress/speedBps/eta）。</summary>
public sealed record DlProgress(string TaskId, string Stage, double Progress, long SpeedBps, long EtaSeconds);

/// <summary>
/// 单个下载任务（KAMUCL tasks.ts 的 TaskRecord）：id/title/kind/status/progress/speedBps/etaSeconds。
/// 即注册即运行；Cancel 等价 AbortController.abort；Pause/Resume 用恢复门实现（读缓冲间隙生效）。
/// 属性从后台线程更新：面板行靠 MainWindow 的 TaskChanged 订阅就地刷新（WPF 绑定不直接挂这里）。
/// </summary>
public sealed class DownloadTask : INotifyPropertyChanged
{
    public string Id { get; } = Guid.NewGuid().ToString("N");
    public string Title { get; }
    public DlKind Kind { get; }
    public string Url { get; }
    public string Dest { get; }
    /// <summary>有 SHA1 就在落盘前校验，失败按引擎的重试策略重来。</summary>
    public string? Sha1 { get; }

    DlStatus _status = DlStatus.Running;
    public DlStatus Status
    {
        get => _status;
        private set { if (_status == value) return; _status = value; StatusChanged?.Invoke(); Raise(null); }
    }
    /// <summary>0-100；总大小未知时保持最近一次估值。</summary>
    public double Progress { get; private set; }
    public long SpeedBps { get; private set; }
    public long EtaSeconds { get; private set; } = -1;
    /// <summary>connecting / downloading / verifying / retrying / done / error。</summary>
    public string Stage { get; private set; } = "connecting";
    public string? Error { get; private set; }
    public long ReceivedBytes => Interlocked.Read(ref _received);
    public long TotalBytes => Interlocked.Read(ref _total);

    internal event Action? StatusChanged;

    readonly CancellationTokenSource _cts = new();
    readonly object _pauseGate = new();
    volatile bool _paused;
    TaskCompletionSource? _resume;
    long _received;
    long _total = -1;

    public DownloadTask(string url, string dest, string title,
        DlKind kind = DlKind.Download, string? sha1 = null)
    { Url = url; Dest = dest; Title = title; Kind = kind; Sha1 = sha1; }

    public event PropertyChangedEventHandler? PropertyChanged;
    void Raise(string? name) => PropertyChanged?.Invoke(this, new(name));

    internal CancellationToken Token => _cts.Token;

    /// <summary>AbortController.abort：排队/下载/校验的任何等待点都会立刻抛出取消。</summary>
    public void Cancel() => _cts.Cancel();

    public void Pause()
    {
        lock (_pauseGate) { if (_status != DlStatus.Running) return; _paused = true; }
        Status = DlStatus.Paused;
    }

    public void Resume()
    {
        lock (_pauseGate)
        {
            if (_status != DlStatus.Paused) return;
            _paused = false;
            _resume?.TrySetResult();
            _resume = null;
        }
        Status = DlStatus.Running;
    }

    internal async Task WaitWhilePausedAsync(CancellationToken ct)
    {
        while (true)
        {
            Task wait;
            lock (_pauseGate)
            {
                if (!_paused) return;
                _resume ??= new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
                wait = _resume.Task;
            }
            await wait.WaitAsync(ct);
        }
    }

    internal void Account(long bytes) => Interlocked.Add(ref _received, bytes);
    internal void ResetAccount() => Interlocked.Exchange(ref _received, 0);
    internal void SetTotal(long total) => Interlocked.Exchange(ref _total, total);
    internal void SetStage(string stage) { Stage = stage; Raise(nameof(Stage)); }

    /// <summary>250ms 节流的进度帧：一次 PropertyChanged(null) 让全部展示字段一起刷。</summary>
    internal void SetTick(double progress, long speed, long eta)
    {
        Progress = progress; SpeedBps = speed; EtaSeconds = eta;
        Raise(null);
    }

    internal void Complete()
    {
        Progress = 100; SpeedBps = 0; EtaSeconds = 0; Stage = "done";
        Status = DlStatus.Done;
    }

    internal void Fail(string message)
    {
        Error = message; Stage = "error"; SpeedBps = 0; EtaSeconds = -1;
        Status = DlStatus.Error;
    }
}

/// <summary>
/// 下载中心（KAMUCL tasks.ts + downloadLimits.ts 的翻译）：任务队列 + 分片下载引擎 + 限速。
/// - ≥1MB 且服务器支持 Range → 分片（片大小 = min(4MB, max(1MB, size/16))），分片并发上限 8，
///   写 dest + ".part"，全部完成后 rename 成 dest；
/// - &lt;1MB 或不支持 Range → HttpClient 直接流式下载；
/// - SHA1 校验（如有 hash）；失败重试最多 4 次，指数退避 100ms×2^n；
/// - 全局并发 16；限速 KiB/s 走令牌桶（150ms 补偿窗口）；
/// - 进度回调 250ms 节流（taskId/stage/progress/speedBps/eta）。
/// </summary>
public sealed class DownloadCenter
{
    const long ChunkThreshold = 1L << 20;   // ≥1MB 才分片
    const int MaxRetries = 4;               // 失败重试最多 4 次
    const int ChunkConcurrency = 8;         // 分片并发上限
    const int GlobalConcurrency = 16;       // 全局并发

    static readonly HttpClient Http = CreateHttp();

    static HttpClient CreateHttp()
    {
        var h = new HttpClient(new SocketsHttpHandler
        {
            AllowAutoRedirect = true,
            ConnectTimeout = TimeSpan.FromSeconds(30),
            KeepAlivePingTimeout = TimeSpan.FromSeconds(30),
        })
        {
            // 流式下载不能挂整体超时（HttpClient.Timeout 会连内容流一起掐）；
            // 卡死/取消都走任务自己的 cts 与用户操作
            Timeout = Timeout.InfiniteTimeSpan,
        };
        h.DefaultRequestHeaders.UserAgent.ParseAdd("WUHUICL/1.0");
        return h;
    }

    /// <summary>任务队列（即注册即运行）。Enqueue/Remove 必须在 UI 线程调用。</summary>
    public ObservableCollection<DownloadTask> Tasks { get; } = new();

    readonly SemaphoreSlim _global = new(GlobalConcurrency);
    TokenBucket? _bucket;
    long? _limitKiBps;

    /// <summary>全局限速 KiB/s；null = 不限速（downloadLimits.ts 的开关）。</summary>
    public long? LimitKiBps
    {
        get => _limitKiBps;
        set { _limitKiBps = value; _bucket = value is > 0 ? new TokenBucket(value.Value * 1024) : null; }
    }

    /// <summary>250ms 节流的进度回调。</summary>
    public event Action<DlProgress>? Progress;
    /// <summary>任务属性变化（面板行就地刷新用，可能来自后台线程）。</summary>
    public event Action<DownloadTask>? TaskChanged;
    /// <summary>活动任务数（running+paused）变化 → DlBadge / 侧栏健康态。</summary>
    public event Action? ActiveCountChanged;

    public int ActiveCount => Tasks.Count(t => t.Status is DlStatus.Running or DlStatus.Paused);

    /// <summary>注册即运行。limitKiBps 给单任务限速（null 则用全局限速）。</summary>
    public DownloadTask Enqueue(string url, string dest, string title,
        DlKind kind = DlKind.Download, string? sha1 = null, long? limitKiBps = null)
    {
        var t = new DownloadTask(url, dest, title, kind, sha1);
        t.StatusChanged += () => ActiveCountChanged?.Invoke();
        Tasks.Add(t);
        ActiveCountChanged?.Invoke();
        _ = Task.Run(() => RunAsync(t, limitKiBps));
        return t;
    }

    /// <summary>移除记录：未完成的先取消再出列（done/error 行的 × 走这里）。</summary>
    public void Remove(string taskId)
    {
        var t = Tasks.FirstOrDefault(x => x.Id == taskId);
        if (t is null) return;
        if (t.Status is DlStatus.Running or DlStatus.Paused) t.Cancel();
        Tasks.Remove(t);
        ActiveCountChanged?.Invoke();
    }

    async Task RunAsync(DownloadTask t, long? taskLimit)
    {
        var ct = t.Token;
        var part = t.Dest + ".part";
        var bucket = taskLimit is > 0 ? new TokenBucket(taskLimit.Value * 1024) : _bucket;
        // 250ms 进度循环的停机开关：任务自然完成后也要停（cts 只在取消时才触发）
        using var finished = CancellationTokenSource.CreateLinkedTokenSource(ct);
        var tracker = TrackAsync(t, finished.Token);
        try
        {
            for (int attempt = 0; ; attempt++)
            {
                try
                {
                    t.ResetAccount();
                    await DownloadAsync(t, part, bucket, ct);
                    if (!string.IsNullOrEmpty(t.Sha1))
                    {
                        t.SetStage("verifying");
                        var got = await Sha1Async(part, ct);
                        if (!string.Equals(got, t.Sha1, StringComparison.OrdinalIgnoreCase))
                            throw new InvalidDataException("SHA1 校验失败");
                    }
                    File.Move(part, t.Dest, overwrite: true);
                    break;
                }
                catch (OperationCanceledException) { throw; }
                catch (Exception) when (attempt < MaxRetries)
                {
                    t.SetStage("retrying");
                    await Task.Delay(100 * (1 << attempt), ct);   // 指数退避 100ms×2^n
                }
            }
            t.Complete();
        }
        catch (OperationCanceledException) { t.Fail("已取消"); TryDelete(part); }
        catch (Exception ex) { t.Fail(ex.Message); TryDelete(part); }
        finally
        {
            finished.Cancel();
            try { await tracker; } catch { /* tracker 吞过取消，兜住即可 */ }
        }
    }

    async Task DownloadAsync(DownloadTask t, string part, TokenBucket? bucket, CancellationToken ct)
    {
        var dir = Path.GetDirectoryName(Path.GetFullPath(t.Dest));
        if (!string.IsNullOrEmpty(dir)) Directory.CreateDirectory(dir);
        var (total, ranges) = await ProbeAsync(t.Url, ct);
        t.SetTotal(total);
        t.SetStage("downloading");
        if (total >= ChunkThreshold && ranges)
        {
            // 片大小 = min(4MB, max(1MB, size/16))
            long chunk = Math.Min(4L << 20, Math.Max(1L << 20, total / 16));
            int n = (int)((total + chunk - 1) / chunk);
            using var slots = new SemaphoreSlim(ChunkConcurrency);
            var all = new List<Task>(n);
            for (int i = 0; i < n; i++)
            {
                long off = i * chunk, end = Math.Min(off + chunk, total);
                await slots.WaitAsync(ct);
                all.Add(Task.Run(async () =>
                {
                    try
                    {
                        await _global.WaitAsync(ct);
                        try { await DownloadChunkAsync(t, off, end, part, bucket, ct); }
                        finally { _global.Release(); }
                    }
                    finally { slots.Release(); }
                }, ct));
            }
            await Task.WhenAll(all);
        }
        else
        {
            await _global.WaitAsync(ct);
            try { await DownloadWholeAsync(t, part, bucket, ct); }
            finally { _global.Release(); }
        }
    }

    async Task DownloadChunkAsync(DownloadTask t, long off, long end, string part,
        TokenBucket? bucket, CancellationToken ct)
    {
        using var req = new HttpRequestMessage(HttpMethod.Get, t.Url);
        req.Headers.Range = new RangeHeaderValue(off, end - 1);
        using var resp = await Http.SendAsync(req, HttpCompletionOption.ResponseHeadersRead, ct);
        resp.EnsureSuccessStatusCode();
        // 服务器嘴上说支持 Range、实际回 200 整包：写进偏移会静默腐坏，直接当失败走重试
        if (resp.StatusCode != HttpStatusCode.PartialContent)
            throw new HttpRequestException("服务器未返回 206，分片下载被降级");
        using var src = await resp.Content.ReadAsStreamAsync(ct);
        using var fs = new FileStream(part, FileMode.OpenOrCreate, FileAccess.Write, FileShare.Write);
        fs.Seek(off, SeekOrigin.Begin);
        await PumpAsync(t, src, fs, bucket, ct);
    }

    async Task DownloadWholeAsync(DownloadTask t, string part, TokenBucket? bucket, CancellationToken ct)
    {
        using var req = new HttpRequestMessage(HttpMethod.Get, t.Url);
        using var resp = await Http.SendAsync(req, HttpCompletionOption.ResponseHeadersRead, ct);
        resp.EnsureSuccessStatusCode();
        if (t.TotalBytes < 0 && resp.Content.Headers.ContentLength is long v) t.SetTotal(v);
        using var src = await resp.Content.ReadAsStreamAsync(ct);
        using var fs = new FileStream(part, FileMode.Create, FileAccess.Write, FileShare.Read);
        await PumpAsync(t, src, fs, bucket, ct);
    }

    /// <summary>读→暂停门→限速→写→记账，一条拷贝循环三处（分片/整包）共用。</summary>
    static async Task PumpAsync(DownloadTask t, Stream src, FileStream fs,
        TokenBucket? bucket, CancellationToken ct)
    {
        var buf = new byte[64 * 1024];
        int n;
        while ((n = await src.ReadAsync(buf, ct)) > 0)
        {
            await t.WaitWhilePausedAsync(ct);
            if (bucket is not null) await bucket.TakeAsync(n, ct);
            await fs.WriteAsync(buf.AsMemory(0, n), ct);
            t.Account(n);
        }
    }

    /// <summary>HEAD 探总大小与 Accept-Ranges；很多服务器不支持 HEAD，失败就按未知大小直接下载。</summary>
    static async Task<(long Total, bool Ranges)> ProbeAsync(string url, CancellationToken ct)
    {
        try
        {
            using var req = new HttpRequestMessage(HttpMethod.Head, url);
            using var resp = await Http.SendAsync(req, ct);
            resp.EnsureSuccessStatusCode();
            return (resp.Content.Headers.ContentLength ?? -1,
                    resp.Headers.AcceptRanges.Contains("bytes"));
        }
        catch (OperationCanceledException) { throw; }
        catch { return (-1, false); }
    }

    static async Task<string> Sha1Async(string path, CancellationToken ct)
    {
        await using var fs = File.OpenRead(path);
        using var sha = SHA1.Create();
        var hash = await sha.ComputeHashAsync(fs, ct);
        return Convert.ToHexString(hash);
    }

    static void TryDelete(string path)
    {
        try { if (File.Exists(path)) File.Delete(path); } catch { /* 残件不挡路 */ }
    }

    /// <summary>250ms 节流的进度循环：速度 = 250ms 增量 / dt，ETA = 剩余 / 速度。</summary>
    async Task TrackAsync(DownloadTask t, CancellationToken ct)
    {
        var sw = Stopwatch.StartNew();
        long lastBytes = 0, lastMs = 0;
        try
        {
            while (true)
            {
                await Task.Delay(250, ct);
                long ms = sw.ElapsedMilliseconds;
                double dt = (ms - lastMs) / 1000.0;
                if (dt <= 0) continue;
                long rec = t.ReceivedBytes;
                long speed = (long)(Math.Max(0, rec - lastBytes) / dt);
                lastBytes = rec; lastMs = ms;
                long total = t.TotalBytes;
                double prog = total > 0 ? Math.Min(100.0, rec * 100.0 / total) : t.Progress;
                long eta = speed > 0 && total > 0 ? Math.Max(0, (total - rec) / speed) : -1;
                t.SetTick(prog, speed, eta);
                Progress?.Invoke(new DlProgress(t.Id, t.Stage, prog, speed, eta));
                TaskChanged?.Invoke(t);
            }
        }
        catch (OperationCanceledException) { }
    }
}

/// <summary>
/// 令牌桶限速（downloadLimits.ts）：容量与初始量都取 150ms 的流量（补偿窗口），
/// 取不足按差额睡到攒够；单笔读取超过桶容量时记成欠债，靠后续时长补齐 —— 否则
/// 64KB 的读缓冲在低速率下永远凑不够 token，会原地打转。
/// 桶跨所有分片共享，全局限速才准。
/// </summary>
sealed class TokenBucket
{
    readonly long _rate;          // 字节/秒
    readonly double _capacity;    // 150ms 的流量
    readonly object _gate = new();
    double _tokens;
    long _last = Stopwatch.GetTimestamp();

    internal TokenBucket(long bytesPerSecond)
    {
        _rate = Math.Max(1024, bytesPerSecond);
        _capacity = _rate * 0.15;
        _tokens = _capacity;
    }

    internal async Task TakeAsync(int bytes, CancellationToken ct)
    {
        while (true)
        {
            double need;
            lock (_gate)
            {
                var now = Stopwatch.GetTimestamp();
                _tokens = Math.Min(_capacity,
                    _tokens + (now - _last) / (double)Stopwatch.Frequency * _rate);
                _last = now;
                if (_tokens >= bytes) { _tokens -= bytes; return; }
                if (bytes > _capacity) { _tokens -= bytes; return; }   // 单笔超大：记欠债放行
                need = (bytes - _tokens) / _rate;
            }
            await Task.Delay(Math.Max(1, (int)Math.Ceiling(need * 1000)), ct);
        }
    }
}