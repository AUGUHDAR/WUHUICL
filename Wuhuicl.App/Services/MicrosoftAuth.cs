using System.IO;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using Wuhuicl.Core.Storage;
using Wuhuicl.Core.Stores;

namespace Wuhuicl.App.Services;

/// <summary>
/// 微软正版登录全链路，逐段对照 KAMUCL-1.1.10/src/main/core/accounts.ts 翻译：
/// 设备码（/devicecode）→ 轮询（/token，pending / slow_down +5s / 过期）→
/// XBL（user.auth.xboxlive.com）→ XSTS（xsts.auth.xboxlive.com，RETAIL / rp://api.minecraftservices.com/）→
/// login_with_xbox → entitlements 拥有权检查（必须有 product_minecraft）→ minecraft/profile 档案。
/// 步骤失败抛「步骤名：原因」（accounts.ts:18 的 step()）；XErr 有友好映射。
/// 令牌明文只在内存，落盘走 AccountStore 的 DPAPI（SecretProtect）；
/// 临期（&lt;5 分钟，accounts.ts:561）启动时自动用 refresh_token 重走全链。
/// msUseProxy=false 直连（安全优先，accounts.ts:321 语义），true 走系统代理。
/// </summary>
public static class MicrosoftAuth
{
    /// <summary>原版 DEFAULT_MS_CLIENT_ID（shared/types.ts:947，Prism 公开应用）。</summary>
    public const string DefaultClientId = "c36a9fb6-4f2a-41ff-90bd-ae7cc92031eb";

    /// <summary>consumers 租户（accounts.ts:32-34）：支持个人 MSA 的设备码流程。</summary>
    const string AuthBase = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
    const string Scope = "XboxLive.signin offline_access";   // accounts.ts:29

    /// <summary>devicecode 端点的返回（展示字段照弹窗需要裁剪）。</summary>
    public sealed record DeviceCodeInfo(string UserCode, string VerificationUri, string DeviceCode,
                                        int IntervalSec, int ExpiresInSec, string Message);

    static HttpClient MakeClient(bool useProxy) => new(new HttpClientHandler { UseProxy = useProxy })
    { Timeout = TimeSpan.FromSeconds(30) };   // accounts.ts 的 AbortSignal.timeout(30000)

    static async Task<JsonElement> PostFormAsync(HttpClient http, string url, Dictionary<string, string> body, CancellationToken ct)
    {
        using var content = new FormUrlEncodedContent(body);
        using var res = await http.PostAsync(url, content, ct);
        var text = await res.Content.ReadAsStringAsync(ct);
        JsonElement json;
        try { json = JsonDocument.Parse(textOrDefault(text)).RootElement; }
        catch { json = JsonDocument.Parse("{}").RootElement; }
        if (!res.IsSuccessStatusCode && json.ValueKind == JsonValueKind.Object && !json.TryGetProperty("error", out _))
            throw new Exception($"请求失败 HTTP {(int)res.StatusCode}: {url}");
        return json;
    }

    static string textOrDefault(string t) => string.IsNullOrWhiteSpace(t) ? "{}" : t;

    static async Task<JsonElement> PostJsonAsync(HttpClient http, string url, object body, CancellationToken ct)
    {
        using var content = new StringContent(JsonSerializer.Serialize(body), Encoding.UTF8, "application/json");
        using var res = await http.PostAsync(url, content, ct);
        var text = textOrDefault(await res.Content.ReadAsStringAsync(ct));
        JsonElement json;
        try { json = JsonDocument.Parse(text).RootElement; }
        catch { json = JsonDocument.Parse("{}").RootElement; }
        if (!res.IsSuccessStatusCode)
        {
            // XErr 提示（accounts.ts:351）+ 友好映射（2148916233 无 Xbox 档案 / 2148916238 儿童账号）
            string xerr = "";
            string friendly = "";
            if (json.ValueKind == JsonValueKind.Object && json.TryGetProperty("XErr", out var xe))
            {
                xerr = $" (XErr {xe.ToString()})";
                friendly = xe.GetInt64() switch
                {
                    2148916233 => "：该微软账号还没有 Xbox 档案，请先在 xbox.com 用此账号登录一次",
                    2148916238 => "：这是儿童账号，需要在家庭设置里允许访问",
                    _ => "",
                };
            }
            throw new Exception($"请求失败 HTTP {(int)res.StatusCode}{xerr}: {url}{friendly}");
        }
        return json;
    }

    static async Task<JsonElement> GetJsonAsync(HttpClient http, string url, string bearer, CancellationToken ct)
    {
        using var req = new HttpRequestMessage(HttpMethod.Get, url);
        req.Headers.Authorization = new System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", bearer);
        using var res = await http.SendAsync(req, ct);
        var text = textOrDefault(await res.Content.ReadAsStringAsync(ct));
        JsonElement json;
        try { json = JsonDocument.Parse(text).RootElement; }
        catch { json = JsonDocument.Parse("{}").RootElement; }
        if (!res.IsSuccessStatusCode) throw new Exception($"请求失败 HTTP {(int)res.StatusCode}: {url}");
        return json;
    }

    /// <summary>步骤失败抛「步骤名：原因」，日志同步进启动器日志窗（accounts.ts:18-26）。</summary>
    static async Task<T> Step<T>(string name, Action<string> log, Func<Task<T>> fn)
    {
        try { return await fn(); }
        catch (Exception e)
        {
            var msg = $"{name}：{e.Message}";
            log(msg);
            throw new Exception(msg);
        }
    }

    /// <summary>申请设备码。失败抛「获取设备码：原因」。</summary>
    public static async Task<DeviceCodeInfo> BeginDeviceCodeAsync(SettingsStore settings, CancellationToken ct)
    {
        using var http = MakeClient(settings.Value.MsUseProxy);
        var dc = await PostFormAsync(http, $"{AuthBase}/devicecode", new Dictionary<string, string>
        {
            ["client_id"] = DefaultClientId,
            ["scope"] = Scope,
        }, ct);
        var code = dc.TryGetProperty("device_code", out var c) ? c.GetString() : null;
        if (string.IsNullOrEmpty(code))
        {
            var msg = dc.TryGetProperty("error_description", out var ed) ? ed.GetString() : null;
            throw new Exception($"获取设备码：{msg ?? "响应缺少 device_code"}");
        }
        return new DeviceCodeInfo(
            dc.TryGetProperty("user_code", out var uc) ? uc.GetString() ?? "" : "",
            dc.TryGetProperty("verification_uri", out var vu) ? vu.GetString() ?? "" : "",
            code,
            dc.TryGetProperty("interval", out var iv) && iv.TryGetInt32(out var i) ? i : 5,
            dc.TryGetProperty("expires_in", out var ex) && ex.TryGetInt32(out var s) ? s : 900,
            dc.TryGetProperty("message", out var m) ? m.GetString() ?? "" : "");
    }

    /// <summary>
    /// 后台轮询 token 端点直到拿到令牌 / 过期 / 取消（accounts.ts:447-479）。
    /// 每步进度经 onStep 汇报（弹窗的等待文案）。返回已入 AccountsStore 的账号。
    /// </summary>
    public static async Task<Account> PollAndCompleteAsync(SettingsStore settings, GamePaths paths, AccountStore accounts,
        DeviceCodeInfo dc, Action<string> log, Action<string>? onStep, CancellationToken ct)
    {
        int interval = Math.Max(1, dc.IntervalSec) * 1000;
        var deadline = Environment.TickCount64 + dc.ExpiresInSec * 1000L;
        using var http = MakeClient(settings.Value.MsUseProxy);
        while (Environment.TickCount64 < deadline)
        {
            try { await Task.Delay(interval, ct); } catch (OperationCanceledException) { throw new OperationCanceledException("已取消"); }
            onStep?.Invoke($"每 {interval / 1000} 秒自动确认一次，等待授权中…");
            var t = await PostFormAsync(http, $"{AuthBase}/token", new Dictionary<string, string>
            {
                ["client_id"] = DefaultClientId,
                ["grant_type"] = "urn:ietf:params:oauth:grant-type:device_code",
                ["device_code"] = dc.DeviceCode,
            }, ct);
            if (t.ValueKind == JsonValueKind.Object && t.TryGetProperty("access_token", out var at) && at.ValueKind == JsonValueKind.String)
            {
                onStep?.Invoke("授权成功，正在完成登录…");
                var acc = await Step("完成登录", log, () => CompleteAsync(http, paths, accounts,
                    at.GetString()!,
                    t.TryGetProperty("refresh_token", out var rt) ? rt.GetString() ?? "" : "",
                    t.TryGetProperty("expires_in", out var ei) && ei.TryGetInt64(out var l) ? l : 86400,
                    log, ct));
                return acc;
            }
            var err = t.ValueKind == JsonValueKind.Object && t.TryGetProperty("error", out var e) ? e.GetString() : null;
            if (err is null or "authorization_pending") continue;
            if (err == "slow_down") { interval += 5000; continue; }
            var desc = t.ValueKind == JsonValueKind.Object && t.TryGetProperty("error_description", out var ed) ? ed.GetString() : null;
            throw new Exception(desc ?? err ?? "登录失败");
        }
        throw new Exception("登录超时，设备码已过期");
    }

    /// <summary>拿微软 access_token 后的完整链路（accounts.ts:377-444），成功即写入账号库。
    /// select=false 用于刷新路径——后台刷新不得改变用户当前选中的账号（accounts.ts:548）。</summary>
    static async Task<Account> CompleteAsync(HttpClient http, GamePaths paths, AccountStore accounts,
        string msAccessToken, string refreshToken, long expiresIn, Action<string> log, CancellationToken ct, bool select = true)
    {
        // 1. XBL 认证
        var xbl = await Step("Xbox Live 认证", log, async () => await PostJsonAsync(http,
            "https://user.auth.xboxlive.com/user/authenticate",
            new
            {
                Properties = new { AuthMethod = "RPS", SiteName = "user.auth.xboxlive.com", RpsTicket = $"d={msAccessToken}" },
                RelyingParty = "http://auth.xboxlive.com",
                TokenType = "JWT"
            }, ct));
        var xblToken = xbl.TryGetProperty("Token", out var xt) ? xt.GetString() : null;
        if (string.IsNullOrEmpty(xblToken)) throw new Exception("Xbox Live 认证失败：响应缺少 Token");

        // 2. XSTS 授权
        var xsts = await Step("XSTS 授权", log, async () => await PostJsonAsync(http,
            "https://xsts.auth.xboxlive.com/xsts/authorize",
            new
            {
                Properties = new { SandboxId = "RETAIL", UserTokens = new[] { xblToken } },
                RelyingParty = "rp://api.minecraftservices.com/",
                TokenType = "JWT"
            }, ct));
        var xstsToken = xsts.TryGetProperty("Token", out var st) ? st.GetString() : null;
        string? uhs = null;
        if (xsts.ValueKind == JsonValueKind.Object && xsts.TryGetProperty("DisplayClaims", out var dcl) &&
            dcl.TryGetProperty("xui", out var xui) && xui.ValueKind == JsonValueKind.Array && xui.GetArrayLength() > 0 &&
            xui[0].TryGetProperty("uhs", out var u))
            uhs = u.GetString();
        if (string.IsNullOrEmpty(xstsToken) || string.IsNullOrEmpty(uhs))
            throw new Exception("XSTS 授权失败（该账号可能尚未创建 Xbox 档案，请先在 xbox.com 登录一次）");

        // 3. MC 登录
        var mc = await Step("Minecraft 登录", log, async () => await PostJsonAsync(http,
            "https://api.minecraftservices.com/authentication/login_with_xbox",
            new { identityToken = $"XBL3.0 x={uhs};{xstsToken}" }, ct));
        var mcToken = mc.TryGetProperty("access_token", out var mt) ? mt.GetString() : null;
        if (string.IsNullOrEmpty(mcToken)) throw new Exception("Minecraft 登录失败：响应缺少 access_token");
        var mcExpires = mc.TryGetProperty("expires_in", out var mei) && mei.TryGetInt64(out var mel) ? mel : expiresIn;

        // 4. 拥有权检查
        var ent = await Step("Minecraft 拥有权检查", log, async () =>
            await GetJsonAsync(http, "https://api.minecraftservices.com/entitlements/mcstore", mcToken!, ct));
        bool owns = false;
        if (ent.ValueKind == JsonValueKind.Object && ent.TryGetProperty("items", out var items) && items.ValueKind == JsonValueKind.Array)
            foreach (var it in items.EnumerateArray())
                if (it.TryGetProperty("name", out var n) && n.GetString() == "product_minecraft") owns = true;
        if (!owns) throw new Exception("Minecraft 拥有权检查：该账号未拥有 Minecraft");

        // 5. 档案
        var profile = await Step("获取 Minecraft 档案", log, async () =>
            await GetJsonAsync(http, "https://api.minecraftservices.com/minecraft/profile", mcToken!, ct));
        var pid = profile.TryGetProperty("id", out var pi) ? pi.GetString() : null;
        var pname = profile.TryGetProperty("name", out var pn) ? pn.GetString() : null;
        if (string.IsNullOrEmpty(pid) || string.IsNullOrEmpty(pname)) throw new Exception("获取 Minecraft 档案失败");

        var uuid = UuidWithHyphens(pid!);
        var acc = new Account
        {
            Id = uuid,
            Kind = "microsoft",
            Name = pname!,
            Uuid = uuid,
            Token = mcToken,
            RefreshToken = string.IsNullOrEmpty(refreshToken) ? null : refreshToken,
            ExpiresMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() + mcExpires * 1000,
            AddedMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(),
        };

        // 档案里带皮肤：落一份脸缓存做账号头像（Avatar.vue 按账号取皮肤图的等价物）。
        _ = Task.Run(async () =>
        {
            try { await CacheProfileSkinAsync(http, paths, profile, uuid, ct); }
            catch { /* 头像失败不影响登录 */ }
        }, CancellationToken.None);

        return accounts.Upsert(acc, select);
    }

    /// <summary> skins[0]（state=ACTIVE）的 url 下载到 &lt;DataDir&gt;\facecache\&lt;uuid&gt;.png。</summary>
    static async Task CacheProfileSkinAsync(HttpClient http, GamePaths paths, JsonElement profile, string uuid, CancellationToken ct)
    {
        if (profile.ValueKind != JsonValueKind.Object || !profile.TryGetProperty("skins", out var skins) ||
            skins.ValueKind != JsonValueKind.Array) return;
        foreach (var s in skins.EnumerateArray())
        {
            var state = s.TryGetProperty("state", out var st) ? st.GetString() : null;
            if (state != "ACTIVE") continue;
            var url = s.TryGetProperty("url", out var ul) ? ul.GetString() : null;
            if (string.IsNullOrEmpty(url)) return;
            var dir = Path.Combine(paths.DataDir, "facecache");
            Directory.CreateDirectory(dir);
            var png = await http.GetByteArrayAsync(url, ct);
            File.WriteAllBytes(Path.Combine(dir, uuid + ".png"), png);
            SkinRenderer.InvalidateFace(uuid);
            return;
        }
    }

    static string UuidWithHyphens(string id)
    {
        var hex = id.Replace("-", "").ToLowerInvariant();
        return $"{hex[..8]}-{hex[8..12]}-{hex[12..16]}-{hex[16..20]}-{hex[20..]}";
    }

    /// <summary>用 refresh_token 重走整条链（accounts.ts:531-550）。更新库并保持该账号选中状态不变语义由 Upsert(select:false) 保证。</summary>
    public static async Task<Account> RefreshAsync(SettingsStore settings, GamePaths paths, AccountStore accounts, Account acc, Action<string> log, CancellationToken ct = default)
    {
        if (string.IsNullOrEmpty(acc.RefreshToken))
            throw new Exception("缺少 refresh_token，请重新登录");
        using var http = MakeClient(settings.Value.MsUseProxy);
        var t = await PostFormAsync(http, $"{AuthBase}/token", new Dictionary<string, string>
        {
            ["client_id"] = DefaultClientId,
            ["grant_type"] = "refresh_token",
            ["refresh_token"] = acc.RefreshToken!,
            ["scope"] = Scope,
        }, ct);
        if (t.ValueKind != JsonValueKind.Object || !t.TryGetProperty("access_token", out var at) || at.ValueKind != JsonValueKind.String)
        {
            var desc = t.ValueKind == JsonValueKind.Object && t.TryGetProperty("error_description", out var ed) ? ed.GetString() : null;
            throw new Exception(desc ?? "微软令牌刷新失败，请重新登录");
        }
        return await CompleteAsync(http, paths, accounts, at.GetString()!,
            t.TryGetProperty("refresh_token", out var rt) && rt.ValueKind == JsonValueKind.String ? rt.GetString()! : acc.RefreshToken!,
            t.TryGetProperty("expires_in", out var ei) && ei.TryGetInt64(out var l) ? l : 86400,
            log, ct, select: false);
    }

    /// <summary>启动前的账号保障（accounts.ts:553-565 getValidAccount）：离线直接过；微软临期自动刷新。</summary>
    public static async Task<Account> EnsureValidAsync(SettingsStore settings, GamePaths paths, AccountStore accounts, Account acc, Action<string> log)
    {
        if (acc.Kind != "microsoft") return acc;
        var now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        if (!string.IsNullOrEmpty(acc.Token) && acc.ExpiresMs - now >= 300_000) return acc;
        log("微软令牌已临期，正在自动刷新…");
        var refreshed = await RefreshAsync(settings, paths, accounts, acc, log);
        log($"已刷新 {refreshed.Name} 的微软令牌");
        return refreshed;
    }

    /// <summary>
    /// 上传皮肤到微软档案（skins.ts uploadSkin — :221-248）：multipart/form-data
    /// （variant=classic|slim + file），Bearer = MC 令牌（临期自动刷新）。
    /// 成功后刷新该账号的脸缓存并返回档案皮肤 URL（调用方负责刷新预览与本地历史）。
    /// pngBytes 必须已过 64×64 校验（SkinService.ImportSkin 同一套）。
    /// </summary>
    public static async Task UploadSkinAsync(SettingsStore settings, GamePaths paths, AccountStore accounts,
        Account acc, byte[] pngBytes, string variant, string fileName, Action<string> log)
    {
        if (variant is not ("classic" or "slim")) throw new Exception("无效的皮肤模型");
        var valid = await EnsureValidAsync(settings, paths, accounts, acc, log);
        var token = valid.Token ?? throw new Exception("需要微软正版登录后才能上传皮肤");

        using var http = MakeClient(settings.Value.MsUseProxy);
        http.Timeout = TimeSpan.FromSeconds(60);   // skins.ts:239 的 60s
        using var form = new MultipartFormDataContent();
        form.Add(new StringContent(variant), "variant");
        var file = new ByteArrayContent(pngBytes);
        file.Headers.ContentType = new System.Net.Http.Headers.MediaTypeHeaderValue("image/png");
        form.Add(file, "file", string.IsNullOrEmpty(fileName) ? "skin.png" : fileName);
        using var req = new HttpRequestMessage(HttpMethod.Post, "https://api.minecraftservices.com/minecraft/profile/skins")
        {
            Headers = { Authorization = new System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", token) },
            Content = form,
        };
        using var res = await http.SendAsync(req);
        if (!res.IsSuccessStatusCode)
        {
            var body = await res.Content.ReadAsStringAsync();
            string detail = body;
            try
            {
                using var doc = JsonDocument.Parse(body);
                if (doc.RootElement.TryGetProperty("errorMessage", out var em)) detail = em.GetString() ?? body;
                else if (doc.RootElement.TryGetProperty("error", out var er)) detail = er.GetString() ?? body;
            }
            catch { }
            throw new Exception($"皮肤上传失败 HTTP {(int)res.StatusCode}：{detail}");
        }

        // 上传成功 → 拉最新档案刷新脸缓存（ skins.ts:247 getProfile(true) 的等价物）
        var profile = await GetJsonAsync(http, "https://api.minecraftservices.com/minecraft/profile", token, CancellationToken.None);
        await CacheProfileSkinAsync(http, paths, profile, acc.Uuid, CancellationToken.None);
    }

    /// <summary>披风激活/卸下（skins.ts changeCape — :251-273）：PUT {capeId} / DELETE。</summary>
    public static async Task ChangeCapeAsync(SettingsStore settings, GamePaths paths, AccountStore accounts,
        Account acc, string? capeId, Action<string> log)
    {
        var valid = await EnsureValidAsync(settings, paths, accounts, acc, log);
        var token = valid.Token ?? throw new Exception("需要微软正版登录后才能更换披风");
        using var http = MakeClient(settings.Value.MsUseProxy);
        http.Timeout = TimeSpan.FromSeconds(30);
        using var req = new HttpRequestMessage(
            capeId is null ? HttpMethod.Delete : HttpMethod.Put,
            "https://api.minecraftservices.com/minecraft/profile/capes/active");
        req.Headers.Authorization = new System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", token);
        if (capeId is not null)
            req.Content = new StringContent(JsonSerializer.Serialize(new { capeId }), Encoding.UTF8, "application/json");
        using var res = await http.SendAsync(req);
        if (!res.IsSuccessStatusCode)
            throw new Exception($"披风更换失败 HTTP {(int)res.StatusCode}");
        var profile = await GetJsonAsync(http, "https://api.minecraftservices.com/minecraft/profile", token, CancellationToken.None);
        await CacheProfileSkinAsync(http, paths, profile, acc.Uuid, CancellationToken.None);
    }
}
