using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Storage;

/// <summary>
/// 用 Windows DPAPI(当前用户) 加密登录 Cookie 后落盘。
/// 非 Windows 构建（未来 Android 复用 Core 时）会以带标记的 JSON 保存，届时应替换为平台安全存储。
/// </summary>
public sealed class ProtectedSessionStore : ISessionStore
{
    private static readonly byte[] Entropy = Encoding.UTF8.GetBytes("LunaticPrincess.Session.v1");
    private static readonly byte[] Magic = "NMMSES01"u8.ToArray();
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web)
    {
        WriteIndented = false,
    };

    private readonly string _filePath;
    private readonly SemaphoreSlim _gate = new(1, 1);

    public ProtectedSessionStore(string? filePath = null)
    {
        _filePath = filePath ?? AppPaths.SessionFilePath;
    }

    public string FilePath => _filePath;

    /// <inheritdoc />
    public string? LastLoadFailure { get; private set; }

    public async Task<NeteaseSession?> LoadAsync(CancellationToken cancellationToken = default)
    {
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            LastLoadFailure = null;
            if (!File.Exists(_filePath))
            {
                return null;
            }

            var raw = await File.ReadAllBytesAsync(_filePath, cancellationToken).ConfigureAwait(false);
            if (raw.Length <= Magic.Length + 1 || !raw.AsSpan(0, Magic.Length).SequenceEqual(Magic))
            {
                LastLoadFailure = "登录文件格式不正确或已损坏";
                return null;
            }

            var mode = raw[Magic.Length];
            var payload = raw[(Magic.Length + 1)..];
            byte[] jsonBytes;
            if (mode == 1)
            {
                if (!OperatingSystem.IsWindows())
                {
                    LastLoadFailure = "登录文件由 Windows 加密保存，当前系统无法解密";
                    return null;
                }

                jsonBytes = ProtectedData.Unprotect(payload, Entropy, DataProtectionScope.CurrentUser);
            }
            else
            {
                jsonBytes = payload;
            }

            var session = JsonSerializer.Deserialize<NeteaseSession>(jsonBytes, JsonOptions);
            return session is { IsLoggedIn: true } ? session : null;
        }
        catch (CryptographicException)
        {
            LastLoadFailure = "登录文件无法解密（可能来自其他 Windows 账号或其他电脑）";
            return null;
        }
        catch (JsonException)
        {
            LastLoadFailure = "登录文件内容无法解析";
            return null;
        }
        catch (IOException ex)
        {
            LastLoadFailure = "读取登录文件失败：" + ex.Message;
            return null;
        }
        catch (UnauthorizedAccessException ex)
        {
            LastLoadFailure = "没有权限读取登录文件：" + ex.Message;
            return null;
        }
        finally
        {
            _gate.Release();
        }
    }

    public async Task SaveAsync(NeteaseSession session, CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(session);

        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            var json = JsonSerializer.SerializeToUtf8Bytes(session, JsonOptions);
            byte mode;
            byte[] payload;
            if (OperatingSystem.IsWindows())
            {
                payload = ProtectedData.Protect(json, Entropy, DataProtectionScope.CurrentUser);
                mode = 1;
            }
            else
            {
                payload = json;
                mode = 2;
            }

            var directory = Path.GetDirectoryName(_filePath);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory);
            }

            var bytes = new byte[Magic.Length + 1 + payload.Length];
            Magic.CopyTo(bytes, 0);
            bytes[Magic.Length] = mode;
            payload.CopyTo(bytes, Magic.Length + 1);

            var tempPath = _filePath + ".tmp";
            await File.WriteAllBytesAsync(tempPath, bytes, cancellationToken).ConfigureAwait(false);
            File.Move(tempPath, _filePath, overwrite: true);
        }
        finally
        {
            _gate.Release();
        }
    }

    public async Task ClearAsync(CancellationToken cancellationToken = default)
    {
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            if (File.Exists(_filePath))
            {
                File.Delete(_filePath);
            }

            var tempPath = _filePath + ".tmp";
            if (File.Exists(tempPath))
            {
                File.Delete(tempPath);
            }
        }
        finally
        {
            _gate.Release();
        }
    }
}
