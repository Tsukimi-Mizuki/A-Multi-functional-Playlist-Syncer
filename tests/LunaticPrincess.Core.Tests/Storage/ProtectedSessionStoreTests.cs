using System.Text;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Storage;
using Xunit;

namespace LunaticPrincess.Core.Tests.Storage;

public sealed class ProtectedSessionStoreTests : IDisposable
{
    private readonly string _directory = Path.Combine(Path.GetTempPath(), "nmm-tests-" + Guid.NewGuid().ToString("N"));

    private string SessionPath => Path.Combine(_directory, "session.dat");

    public ProtectedSessionStoreTests() => Directory.CreateDirectory(_directory);

    [Fact]
    public async Task SaveLoadClear_RoundTripsSession()
    {
        var store = new ProtectedSessionStore(SessionPath);
        var session = new NeteaseSession
        {
            Cookie = "MUSIC_U=secret-value; __csrf=csrf-value; os=pc",
            UserId = 123456789,
            Nickname = "测试用户",
            AvatarUrl = "https://example.invalid/avatar.jpg",
            VipType = 11,
            LoggedInAt = DateTimeOffset.Now,
        };

        await store.SaveAsync(session);

        Assert.True(File.Exists(SessionPath));
        var raw = await File.ReadAllBytesAsync(SessionPath);
        Assert.True(raw.AsSpan(0, 8).SequenceEqual("NMMSES01"u8));
        Assert.DoesNotContain("MUSIC_U=secret-value", Encoding.UTF8.GetString(raw));

        var loaded = await store.LoadAsync();
        Assert.NotNull(loaded);
        Assert.Equal(session.Cookie, loaded!.Cookie);
        Assert.Equal(session.UserId, loaded.UserId);
        Assert.Equal(session.Nickname, loaded.Nickname);
        Assert.Equal("csrf-value", loaded.CsrfToken);
        Assert.True(loaded.IsLoggedIn);

        await store.ClearAsync();
        Assert.False(File.Exists(SessionPath));
        Assert.Null(await store.LoadAsync());
    }

    [Fact]
    public async Task LoadAsync_MissingOrCorruptFile_ReturnsNull()
    {
        var store = new ProtectedSessionStore(SessionPath);
        Assert.Null(await store.LoadAsync());

        await File.WriteAllTextAsync(SessionPath, "not a session");
        Assert.Null(await store.LoadAsync());
    }

    [Fact]
    public void SessionHelpers_ParseCookies()
    {
        var session = new NeteaseSession { Cookie = "MUSIC_U=abc; __csrf=tok ; os=pc" };
        Assert.True(session.IsLoggedIn);
        Assert.Equal("abc", session.GetCookieValue("MUSIC_U"));
        Assert.Equal("tok", session.GetCookieValue("__CSRF"));
        Assert.Null(session.GetCookieValue("missing"));
        Assert.Equal("tok", session.CsrfToken);
    }

    public void Dispose()
    {
        try { Directory.Delete(_directory, recursive: true); } catch { }
    }
}
