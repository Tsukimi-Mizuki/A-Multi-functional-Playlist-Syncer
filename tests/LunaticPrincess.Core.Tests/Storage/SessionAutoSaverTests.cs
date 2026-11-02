using System.Net;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Storage;
using LunaticPrincess.Core.Tests.Netease;
using Xunit;

namespace LunaticPrincess.Core.Tests.Storage;

public sealed class SessionAutoSaverTests
{
    private const string FixedSecret = "0123456789abcdef";
    private const string AccountJson = """{"code":200,"account":{"id":1},"profile":{"userId":1,"nickname":"n"}}""";

    private sealed class RecordingStore : ISessionStore
    {
        private readonly object _gate = new();

        public List<NeteaseSession> Saved { get; } = [];
        public bool ThrowOnSave { get; set; }

        public Task<NeteaseSession?> LoadAsync(CancellationToken cancellationToken = default) =>
            Task.FromResult<NeteaseSession?>(null);

        public Task SaveAsync(NeteaseSession session, CancellationToken cancellationToken = default)
        {
            if (ThrowOnSave)
            {
                throw new IOException("磁盘已满");
            }

            lock (_gate)
            {
                Saved.Add(session);
            }

            return Task.CompletedTask;
        }

        public Task ClearAsync(CancellationToken cancellationToken = default) => Task.CompletedTask;
    }

    [Fact]
    public async Task RotatedCookie_IsPersistedWithLatestSession()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(AccountJson, HttpStatusCode.OK, "MUSIC_U=rotated; Path=/");
        var client = TestHttp.CreateClient(handler, FixedSecret);
        client.SetSession(new NeteaseSession { Cookie = "MUSIC_U=old" });
        var store = new RecordingStore();
        using var saver = new SessionAutoSaver(client, store);

        await client.GetCurrentUserAsync();
        await saver.FlushAsync();

        var saved = Assert.Single(store.Saved);
        Assert.Equal("rotated", saved.GetCookieValue("MUSIC_U"));
    }

    [Fact]
    public async Task SaveFailure_IsReportedAndDoesNotThrow()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(AccountJson, HttpStatusCode.OK, "MUSIC_U=rotated; Path=/");
        var client = TestHttp.CreateClient(handler, FixedSecret);
        client.SetSession(new NeteaseSession { Cookie = "MUSIC_U=old" });
        var errors = new List<Exception>();
        using var saver = new SessionAutoSaver(client, new RecordingStore { ThrowOnSave = true }, errors.Add);

        await client.GetCurrentUserAsync();
        await saver.FlushAsync();

        Assert.IsType<IOException>(Assert.Single(errors));
    }

    [Fact]
    public async Task Dispose_StopsListening()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(AccountJson, HttpStatusCode.OK, "MUSIC_U=rotated; Path=/");
        var client = TestHttp.CreateClient(handler, FixedSecret);
        client.SetSession(new NeteaseSession { Cookie = "MUSIC_U=old" });
        var store = new RecordingStore();
        var saver = new SessionAutoSaver(client, store);
        saver.Dispose();

        await client.GetCurrentUserAsync();
        await saver.FlushAsync();

        Assert.Empty(store.Saved);
    }
}
