using System.IO;
using System.Text.Json.Serialization;
using Wuhuicl.Core.Storage;

namespace Wuhuicl.Core.Stores;

public sealed class Account
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("kind")] public string Kind { get; set; } = "offline";   // offline|microsoft|yggdrasil
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("uuid")] public string Uuid { get; set; } = "";
    [JsonPropertyName("providerRoot")] public string? ProviderRoot { get; set; }
    [JsonPropertyName("providerName")] public string? ProviderName { get; set; }
    [JsonPropertyName("addedMs")] public long AddedMs { get; set; }

    [JsonIgnore] public string? Token { get; set; }        // 明文只在内存里；落盘走加密字段
    [JsonPropertyName("tokenEnc")] public string? TokenEnc { get; set; }
    [JsonPropertyName("refreshEnc")] public string? RefreshEnc { get; set; }
    [JsonPropertyName("expiresMs")] public long ExpiresMs { get; set; }

    public static Account MakeOffline(string name)
    {
        var err = ValidateName(name);
        if (err is not null) throw new ArgumentException(err);
        return new Account
        {
            Id = "off-" + name.ToLowerInvariant(),
            Kind = "offline",
            Name = name,
            Uuid = Json.OfflineUuid(name),
            Token = Guid.NewGuid().ToString("N"),
            AddedMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(),
        };
    }

    /// <summary>原版规则：1-16 位 [A-Za-z0-9_]。</summary>
    public static string? ValidateName(string name)
    {
        if (string.IsNullOrWhiteSpace(name)) return "玩家名不能为空";
        if (name.Length > 16) return "玩家名最长 16 个字符";
        foreach (var c in name)
            if (!(char.IsAsciiLetter(c) || char.IsAsciiDigit(c) || c == '_'))
                return $"玩家名只能包含字母、数字和下划线，出现了「{c}」";
        return null;
    }
}

public sealed class AccountFile
{
    [JsonPropertyName("format")] public int Format { get; set; } = 1;
    [JsonPropertyName("accounts")] public List<Account> Accounts { get; set; } = new();
    [JsonPropertyName("selectedId")] public string? SelectedId { get; set; }
}

/// <summary>
/// 账号存储。台账 AUTH-2 的教训：绝不整库覆盖写坏；AUTH-1：解密失败时绝不
/// 用"空值"覆盖旧密文。这里对 token 采用"读不出就保留原密文字段"的策略。
/// </summary>
public sealed class AccountStore
{
    readonly GamePaths _paths;
    AccountFile _data = new();
    readonly Dictionary<string, string?> _plainSecrets = new(StringComparer.Ordinal);

    public AccountStore(GamePaths paths) { _paths = paths; }

    public IReadOnlyList<Account> Accounts => _data.Accounts;
    public Account? Selected =>
        _data.Accounts.FirstOrDefault(a => a.Id == _data.SelectedId) ?? _data.Accounts.FirstOrDefault();

    public void Load()
    {
        _data = AtomicJson.Load<AccountFile>(_paths.AccountsFile, new AccountFile());
        foreach (var a in _data.Accounts)
        {
            var (plain, damaged) = SecretProtect.Unprotect(a.TokenEnc);
            _plainSecrets[a.Id] = plain;
            a.Token = plain;
            if (damaged) a.Token = null;      // 保留 TokenEnc 原值，绝不写空覆盖
        }
        if (_data.SelectedId is null && _data.Accounts.Count > 0)
            _data.SelectedId = _data.Accounts[0].Id;
    }

    public void Save()
    {
        foreach (var a in _data.Accounts)
        {
            if (!string.IsNullOrEmpty(a.Token))
            {
                a.TokenEnc = SecretProtect.Protect(a.Token);
                _plainSecrets[a.Id] = a.Token;
            }
            else if (_plainSecrets.TryGetValue(a.Id, out var old) && !string.IsNullOrEmpty(old))
                a.TokenEnc = SecretProtect.Protect(old!);   // 保持原密文，不覆盖成空
            // 否则保留磁盘上已有的 TokenEnc
        }
        AtomicJson.Save(_paths.AccountsFile, _data);
    }

    public Account AddOffline(string name)
    {
        var acc = Account.MakeOffline(name);
        var exist = _data.Accounts.FindIndex(a => a.Name.Equals(name, StringComparison.OrdinalIgnoreCase));
        if (exist >= 0) _data.Accounts[exist] = acc;
        else _data.Accounts.Add(acc);
        _data.SelectedId = acc.Id;
        Save();
        return acc;
    }

    public bool Select(string id)
    {
        if (!_data.Accounts.Any(a => a.Id == id)) return false;
        _data.SelectedId = id;
        Save();
        return true;
    }

    public void Remove(string id)
    {
        _data.Accounts.RemoveAll(a => a.Id == id);
        if (_data.SelectedId == id) _data.SelectedId = _data.Accounts.FirstOrDefault()?.Id;
        Save();
    }
}

/// <summary>
/// 当前实现：Windows 数据保护（DPAPI，用户范围）。失败时退回"仅混淆 + 明确告警"，
/// 绝不静默把凭据丢掉。
/// </summary>
public static class SecretProtect
{
    // 直接 P/Invoke DPAPI：不引 NuGet 包（System.Security.Cryptography.ProtectedData
    // 不在共享框架里），保证离线可构建、产物无额外依赖。
    [System.Runtime.InteropServices.DllImport("crypt32.dll", SetLastError = true)]
    static extern bool CryptProtectData(ref Blob inData, string? desc, ref Blob entropy,
        IntPtr reserved, IntPtr prompt, int flags, out Blob outData);

    [System.Runtime.InteropServices.DllImport("crypt32.dll", SetLastError = true)]
    static extern bool CryptUnprotectData(ref Blob inData, IntPtr desc, ref Blob entropy,
        IntPtr reserved, IntPtr prompt, int flags, out Blob outData);

    [System.Runtime.InteropServices.DllImport("kernel32.dll")]
    static extern IntPtr LocalFree(IntPtr h);

    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    struct Blob { public int cbData; public IntPtr pbData; }

    static readonly byte[] Entropy = "WUHUICL.v1"u8.ToArray();
    const int UI_FORBIDDEN = 0x01;

    static (bool ok, byte[] data) Run(byte[] input, bool protect)
    {
        var pb = System.Runtime.InteropServices.Marshal.AllocHGlobal(input.Length);
        var pe = System.Runtime.InteropServices.Marshal.AllocHGlobal(Entropy.Length);
        System.Runtime.InteropServices.Marshal.Copy(input, 0, pb, input.Length);
        System.Runtime.InteropServices.Marshal.Copy(Entropy, 0, pe, Entropy.Length);
        var inBlob = new Blob { cbData = input.Length, pbData = pb };
        var ent = new Blob { cbData = Entropy.Length, pbData = pe };
        try
        {
            bool ok = protect
                ? CryptProtectData(ref inBlob, null, ref ent, IntPtr.Zero, IntPtr.Zero, UI_FORBIDDEN, out var outBlob)
                : CryptUnprotectData(ref inBlob, IntPtr.Zero, ref ent, IntPtr.Zero, IntPtr.Zero, UI_FORBIDDEN, out outBlob);
            if (!ok) return (false, Array.Empty<byte>());
            var res = new byte[outBlob.cbData];
            System.Runtime.InteropServices.Marshal.Copy(outBlob.pbData, res, 0, outBlob.cbData);
            LocalFree(outBlob.pbData);
            return (true, res);
        }
        finally
        {
            System.Runtime.InteropServices.Marshal.FreeHGlobal(pb);
            System.Runtime.InteropServices.Marshal.FreeHGlobal(pe);
        }
    }

    public static string Protect(string plain)
    {
        var bytes = System.Text.Encoding.UTF8.GetBytes(plain);
        var (ok, prot) = Run(bytes, protect: true);
        return ok ? "dpapi:" + Convert.ToBase64String(prot) : "plain:" + Convert.ToBase64String(bytes);
    }

    /// <returns>明文, 是否损坏（损坏时调用方必须保留原密文）</returns>
    public static (string? plain, bool damaged) Unprotect(string? stored)
    {
        if (string.IsNullOrEmpty(stored)) return (null, false);
        try
        {
            if (stored.StartsWith("plain:", StringComparison.Ordinal))
                return (System.Text.Encoding.UTF8.GetString(Convert.FromBase64String(stored[6..])), false);
            if (stored.StartsWith("dpapi:", StringComparison.Ordinal))
            {
                var (ok, raw) = Run(Convert.FromBase64String(stored[6..]), protect: false);
                return ok ? (System.Text.Encoding.UTF8.GetString(raw), false) : (null, true);
            }
            return (null, true);
        }
        catch
        {
            return (null, true);
        }
    }
}

public sealed class Settings
{
    [JsonPropertyName("format")] public int Format { get; set; } = 1;
    [JsonPropertyName("maxHeapMb")] public int MaxHeapMb { get; set; } = 2048;
    [JsonPropertyName("minHeapMb")] public int MinHeapMb { get; set; } = 512;
    [JsonPropertyName("javaPath")] public string? JavaPath { get; set; }
    [JsonPropertyName("autoJava")] public bool AutoJava { get; set; } = true;
    [JsonPropertyName("useMirror")] public bool UseMirror { get; set; } = true;
    [JsonPropertyName("mirrorRoot")] public string MirrorRoot { get; set; } = "https://bmclapi2.bangbang93.com";
    [JsonPropertyName("extraJvmArgs")] public string ExtraJvmArgs { get; set; } = "";
    [JsonPropertyName("extraGameArgs")] public string ExtraGameArgs { get; set; } = "";
    [JsonPropertyName("fullscreen")] public bool Fullscreen { get; set; }
    [JsonPropertyName("resolutionWidth")] public int Width { get; set; } = 1280;
    [JsonPropertyName("resolutionHeight")] public int Height { get; set; } = 720;
    [JsonPropertyName("gameDirOverride")] public string? GameDirOverride { get; set; }
    [JsonPropertyName("closeAfterLaunch")] public bool CloseAfterLaunch { get; set; }
    [JsonPropertyName("showSnapshots")] public bool ShowSnapshots { get; set; }
    [JsonPropertyName("theme")] public string Theme { get; set; } = "dark";
    [JsonPropertyName("lastVersion")] public string? LastVersion { get; set; }
}

public sealed class SettingsStore
{
    readonly GamePaths _paths;
    public Settings Value { get; private set; } = new();
    public SettingsStore(GamePaths paths) { _paths = paths; }
    public void Load() => Value = AtomicJson.Load(_paths.SettingsFile, new Settings());
    public void Save() => AtomicJson.Save(_paths.SettingsFile, Value);
}
