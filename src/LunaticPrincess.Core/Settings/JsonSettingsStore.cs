using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Settings;

public sealed class JsonSettingsStore : ISettingsStore
{
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web)
    {
        WriteIndented = true,
        Converters = { new JsonStringEnumConverter() },
    };

    private readonly string _filePath;
    private readonly SemaphoreSlim _gate = new(1, 1);

    public JsonSettingsStore(string? filePath = null)
    {
        _filePath = filePath ?? AppPaths.SettingsFilePath;
    }

    public string FilePath => _filePath;

    public async Task<AppSettings> LoadAsync(CancellationToken cancellationToken = default)
    {
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            if (!File.Exists(_filePath))
            {
                return new AppSettings().Normalize();
            }

            await using var stream = new FileStream(
                _filePath, FileMode.Open, FileAccess.Read, FileShare.ReadWrite, 4096, useAsync: true);
            var node = await JsonNode.ParseAsync(stream, cancellationToken: cancellationToken).ConfigureAwait(false);
            if (node is not JsonObject root)
            {
                return new AppSettings().Normalize();
            }

            MigrateLegacyValues(root);
            var settings = root.Deserialize<AppSettings>(JsonOptions);
            return (settings ?? new AppSettings()).Normalize();
        }
        catch (JsonException)
        {
            return new AppSettings().Normalize();
        }
        catch (IOException)
        {
            return new AppSettings().Normalize();
        }
        finally
        {
            _gate.Release();
        }
    }

    /// <summary>兼容旧版设置文件：音质枚举改名/删档后旧名称仍可读回，避免整份设置被判为损坏而重置。</summary>
    private static void MigrateLegacyValues(JsonObject root)
    {
        if (root["quality"] is not JsonValue qualityNode || !qualityNode.TryGetValue<string>(out var rawQuality))
        {
            return;
        }

        if (AudioQualityExtensions.TryParseSettingValue(rawQuality, out var quality))
        {
            root["quality"] = quality.ToString();
        }
        else
        {
            // 无法识别的值：丢弃该项让默认音质生效，其余设置保持原样。
            root.Remove("quality");
        }
    }

    public async Task SaveAsync(AppSettings settings, CancellationToken cancellationToken = default)
    {
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            var directory = Path.GetDirectoryName(_filePath);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory);
            }

            var tempPath = _filePath + ".tmp";
            await using (var stream = new FileStream(
                tempPath, FileMode.Create, FileAccess.Write, FileShare.None, 4096, useAsync: true))
            {
                await JsonSerializer.SerializeAsync(stream, settings.Normalize(), JsonOptions, cancellationToken)
                    .ConfigureAwait(false);
                await stream.FlushAsync(cancellationToken).ConfigureAwait(false);
            }

            File.Move(tempPath, _filePath, overwrite: true);
        }
        finally
        {
            _gate.Release();
        }
    }
}
