using NeteaseMusicManager.Core.Utilities;
using Xunit;

namespace NeteaseMusicManager.Core.Tests.Utilities;

public sealed class FileNameSanitizerTests
{
    [Fact]
    public void Sanitize_ReplacesInvalidWindowsCharacters()
    {
        var value = FileNameSanitizer.Sanitize("a/b:c*?\"<>|");
        Assert.DoesNotContain('/', value);
        Assert.DoesNotContain(':', value);
        Assert.DoesNotContain('*', value);
        Assert.DoesNotContain('?', value);
        Assert.DoesNotContain('|', value);
    }

    [Fact]
    public void BuildFileName_AppliesTemplateAndExtension()
    {
        var name = FileNameSanitizer.BuildFileName(
            "{artist} - {title} ({year}) [{track}]", "晴天", "周杰伦", "叶惠美", 3, 2003, 186016, ".mp3");
        Assert.Equal("周杰伦 - 晴天 (2003) [03].mp3", name);
    }

    [Fact]
    public void BuildFileName_HandlesEmptyValuesAndMissingExtension()
    {
        var name = FileNameSanitizer.BuildFileName("{title}", null, null, null, 0, 0, 12, "flac");
        Assert.EndsWith(".flac", name);
        Assert.False(string.IsNullOrWhiteSpace(name[..^5]));
    }
}
