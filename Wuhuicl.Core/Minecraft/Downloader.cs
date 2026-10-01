using System.IO;
using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;

namespace Wuhuicl.Core.Minecraft;

public sealed record DownloadItem(
    string Url, string Dest, string? Sha1 = null, long Size = 0,
    string? Label = null, string[]? Mirrors = null);

public sealed record DownloadResult(DownloadItem Item, bool Skipped, bool Verified, string? Error)
{
    public bool Ok => Error is null;
}

/// <summary>
/// 并发下载器：断点续传、镜像回退、sha1 强校验、临时文件 + 原子改名。
/// 校验失败一定删掉半成品——绝不允许"文件已存在"当作"文件正确"（台账 PACK-1）。
/// </summary>
public sealed class Downloader
{
    static readonly HttpClient Http = CreateClient();
    public int Concurrency { get; set; } = 6;
    public int MaxAttempts { get; set; } = 4;
    /// <summary>主地址失败后是否优先试镜像（国内网络默认开）。</summary>
    public bool PreferMirrorFirst { get; set; }
    public long MinFreeBytes { get; set; } = 200L * 1024 * 1024;
    public string UserAgent { get; set; } = "WUHUICL/0.1 (launcher)";

    static HttpClient CreateClient()
    {
        var h = new SocketsHttpHandler
        {
            AutomaticDecompression = DecompressionMethods.All,
            PooledConnectionLifetime = TimeSpan.FromMinutes(5),
            MaxConnectionsPerServer = 16,
        };
        return new HttpClient(h) { Timeout = TimeSpan.FromSeconds(60) };
    }

    public async Task<IReadOnlyList<DownloadResult>> RunAsync(
        IEnumerable<DownloadItem> items, IProgress<string>? progress = null,
        CancellationToken ct = default)
    {
        var list = items.ToList();
        var results = new DownloadResult[list.Count];
        var pending = new System.Collections.Concurrent.ConcurrentQueue<int>();
        for (int i = 0; i < list.Count; i++) pending.Enqueue(i);
        int done = 0;

        async Task Worker()
        {
            while (pending.TryDequeue(out var idx) && !ct.IsCancellationRequested)
            {
                var item = list[idx];
                results[idx] = await OneAsync(item, ct);
                var n = Interlocked.Increment(ref done);
                progress?.Report($"{n}/{list.Count} {item.Label ?? Path.GetFileName(item.Dest)}" +
                                 (results[idx].Skipped ? "（已存在）" :
                                  results[idx].Ok ? "" : " 失败：" + results[idx].Error));
            }
        }

        var workers = Enumerable.Range(0, Math.Max(1, Math.Min(Concurrency, list.Count))).Select(_ => Worker());
        await Task.WhenAll(workers);
        return results;
    }

    public async Task<DownloadResult> OneAsync(DownloadItem item, CancellationToken ct)
    {
        try
        {
            var dir = Path.GetDirectoryName(item.Dest);
            if (!string.IsNullOrEmpty(dir)) Directory.CreateDirectory(dir);
            EnsureDiskSpace(dir ?? Path.GetPathRoot(item.Dest)!);

            // 已存在：有 hash 就校验，无 hash 且大小吻合才放过
            if (File.Exists(item.Dest))
            {
                var fi = new FileInfo(item.Dest);
                if (!string.IsNullOrEmpty(item.Sha1))
                {
                    if (await VerifyAsync(item.Dest, item.Sha1, ct))
                        return new DownloadResult(item, true, true, null);
                    TryDelete(item.Dest);                     // 坏文件必须清掉重下
                }
                else if (item.Size > 0 && fi.Length == item.Size)
                    return new DownloadResult(item, true, false, null);
                else TryDelete(item.Dest);
            }
            if (!string.IsNullOrEmpty(item.Sha1) && File.Exists(item.Dest))
                TryDelete(item.Dest);

            var urls = BuildUrlList(item);
            string? lastError = null;
            foreach (var url in urls)
            {
                for (int attempt = 1; attempt <= MaxAttempts; attempt++)
                {
                    ct.ThrowIfCancellationRequested();
                    var (ok, err) = await TryFetchAsync(url, item, ct);
                    if (ok) return new DownloadResult(item, false, !string.IsNullOrEmpty(item.Sha1), null);
                    lastError = err;
                    if (err?.Contains("NOT_FOUND_HTTP") == true) break;   // 404 不必重试
                    await Task.Delay(Math.Min(800 * attempt, 4000), ct);
                }
            }
            return new DownloadResult(item, false, false, lastError ?? "所有地址都失败");
        }
        catch (OperationCanceledException) { throw; }
        catch (Exception ex)
        {
            return new DownloadResult(item, false, false, ex.Message);
        }
    }

    List<string> BuildUrlList(DownloadItem item)
    {
        var urls = new List<string> { item.Url };
        if (item.Mirrors is not null)
            urls = PreferMirrorFirst ? item.Mirrors.Append(item.Url).ToList()
                                     : urls.Concat(item.Mirrors).ToList();
        return urls;
    }

    async Task<(bool ok, string? err)> TryFetchAsync(string url, DownloadItem item, CancellationToken ct)
    {
        var tmp = item.Dest + ".wu-" + Guid.NewGuid().ToString("N")[..8];
        long existing = 0;
        try
        {
            using var req = new HttpRequestMessage(HttpMethod.Get, url);
            req.Headers.TryAddWithoutValidation("User-Agent", UserAgent);
            if (File.Exists(tmp))
            {
                existing = new FileInfo(tmp).Length;
                if (existing > 0) req.Headers.Range = new RangeHeaderValue(existing, null);
            }
            using var resp = await Http.SendAsync(req, HttpCompletionOption.ResponseHeadersRead, ct);
            if (resp.StatusCode == HttpStatusCode.NotFound)
                return (false, "NOT_FOUND_HTTP " + url);
            if (resp.StatusCode == HttpStatusCode.RequestedRangeNotSatisfiable)
            {
                TryDelete(tmp);
                existing = 0;
                return await RetryFullAsync(url, item, tmp, ct);
            }
            if (!resp.IsSuccessStatusCode)
                return (false, $"HTTP {(int)resp.StatusCode} {url}");

            bool resume = resp.StatusCode == HttpStatusCode.PartialContent && existing > 0;
            var total = resp.Content.Headers.ContentLength ?? 0;
            await using (var src = await resp.Content.ReadAsStreamAsync(ct))
            await using (var dst = new FileStream(tmp, resume ? FileMode.Append : FileMode.Create,
                                                 FileAccess.Write, FileShare.None, 1 << 16))
            {
                var buf = new byte[1 << 16];
                int n;
                while ((n = await src.ReadAsync(buf, ct)) > 0) await dst.WriteAsync(buf.AsMemory(0, n), ct);
            }

            var fi = new FileInfo(tmp);
            if (item.Size > 0 && fi.Length != item.Size)
                return (false, $"大小不符：{fi.Length} != {item.Size}");
            if (!string.IsNullOrEmpty(item.Sha1) && !await VerifyAsync(tmp, item.Sha1, ct))
            {
                TryDelete(tmp);
                return (false, "SHA1 校验不通过");
            }
            Directory.CreateDirectory(Path.GetDirectoryName(item.Dest)!);
            if (File.Exists(item.Dest)) File.Delete(item.Dest);
            File.Move(tmp, item.Dest);
            return (true, null);
        }
        catch (OperationCanceledException) { throw; }
        catch (Exception ex)
        {
            return (false, ex.Message);
        }
        finally
        {
            TryDelete(tmp);
        }
    }

    async Task<(bool ok, string? err)> RetryFullAsync(string url, DownloadItem item, string tmp, CancellationToken ct)
    {
        // 服务器不支持 Range：删掉半截重来一次
        TryDelete(tmp);
        using var resp = await Http.GetAsync(url, HttpCompletionOption.ResponseHeadersRead, ct);
        if (!resp.IsSuccessStatusCode) return (false, $"HTTP {(int)resp.StatusCode}");
        await using var src = await resp.Content.ReadAsStreamAsync(ct);
        await using var dst = File.Create(tmp);
        await src.CopyToAsync(dst, ct);
        if (!string.IsNullOrEmpty(item.Sha1) && !await VerifyAsync(tmp, item.Sha1, ct))
        {
            TryDelete(tmp);
            return (false, "SHA1 校验不通过");
        }
        File.Move(tmp, item.Dest);
        return (true, null);
    }

    public static async Task<bool> VerifyAsync(string path, string expectSha1, CancellationToken ct)
    {
        if (string.IsNullOrWhiteSpace(expectSha1)) return true;
        try
        {
            await using var fs = File.OpenRead(path);
            var hash = await System.Security.Cryptography.SHA1.HashDataAsync(fs, ct);
            return string.Equals(Convert.ToHexString(hash).ToLowerInvariant(),
                                 expectSha1.Trim().ToLowerInvariant(), StringComparison.Ordinal);
        }
        catch { return false; }
    }

    void EnsureDiskSpace(string dir)
    {
        try
        {
            var root = Path.GetPathRoot(Path.GetFullPath(dir));
            if (string.IsNullOrEmpty(root)) return;
            var free = new DriveInfo(root).AvailableFreeSpace;
            if (free < MinFreeBytes)
                throw new IOException($"磁盘剩余不足：{free / 1024 / 1024} MB，至少需要 " +
                                      $"{MinFreeBytes / 1024 / 1024} MB");
        }
        catch (ArgumentException) { /* 取不到盘符就不拦 */ }
    }

    static void TryDelete(string path)
    {
        try { if (File.Exists(path)) File.Delete(path); } catch { }
    }
}
