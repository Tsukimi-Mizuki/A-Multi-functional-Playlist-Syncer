using System.Runtime.InteropServices;
using LunaticPrincess.Core.Utilities;
using Xunit;

namespace LunaticPrincess.Core.Tests.Utilities;

public sealed class ErrorTextTests
{
    private const string DeniedMessage = "Access to the path 'D:\\音乐\\中转\\.nmm-1a2b3c.part' is denied.";

    [Fact]
    public void DescribeWrite_PrefersTargetDirectoryAndExplainsWhatToDo()
    {
        var message = ErrorText.DescribeWrite(
            new UnauthorizedAccessException(DeniedMessage),
            "D:\\音乐\\中转");

        Assert.Contains("没有写入权限", message, StringComparison.Ordinal);
        Assert.Contains("D:\\音乐\\中转", message, StringComparison.Ordinal);
        Assert.Contains("管理员身份", message, StringComparison.Ordinal);
        Assert.DoesNotContain("is denied", message, StringComparison.Ordinal);
    }

    [Fact]
    public void Describe_ExtractsPathFromEnglishDeniedMessage()
    {
        var message = ErrorText.Describe(new UnauthorizedAccessException(DeniedMessage));

        Assert.Contains("权限不足", message, StringComparison.Ordinal);
        Assert.Contains(@"D:\音乐\中转\.nmm-1a2b3c.part", message, StringComparison.Ordinal);
    }

    [Fact]
    public void Describe_ExtractsPathFromLocalizedDeniedMessage()
    {
        var message = ErrorText.Describe(new IOException("对路径“D:\\音乐\\新歌.mp3”的访问被拒绝。"));

        Assert.Contains("权限不足", message, StringComparison.Ordinal);
        Assert.Contains(@"D:\音乐\新歌.mp3", message, StringComparison.Ordinal);
    }

    [Fact]
    public void Describe_HandlesDeniedIOExceptionWithoutPath()
    {
        var message = ErrorText.Describe(new IOException("Access is denied."));

        Assert.Contains("权限不足", message, StringComparison.Ordinal);
        Assert.Contains("管理员身份", message, StringComparison.Ordinal);
    }

    [Fact]
    public void Describe_ReportsDiskFull()
    {
        var exception = Marshal.GetExceptionForHR(unchecked((int)0x80070070))!;

        Assert.True(ErrorText.IsDiskFull(exception));
        Assert.Contains("磁盘空间不足", ErrorText.Describe(exception), StringComparison.Ordinal);
    }

    [Fact]
    public void Describe_ReportsFileInUse()
    {
        var exception = Marshal.GetExceptionForHR(unchecked((int)0x80070020))!;

        Assert.True(ErrorText.IsFileInUse(exception));
        Assert.Contains("正被其他程序占用", ErrorText.Describe(exception), StringComparison.Ordinal);
    }

    [Fact]
    public void Describe_ReportsPathTooLong()
    {
        var message = ErrorText.Describe(new PathTooLongException("The specified path is too long."));

        Assert.Contains("路径过长", message, StringComparison.Ordinal);
    }

    [Fact]
    public void DescribeWrite_FallsBackToGenericDescriptionForOtherErrors()
    {
        var message = ErrorText.DescribeWrite(new InvalidOperationException("接口返回 500"), "D:\\音乐");

        Assert.Equal("接口返回 500", message);
    }

    [Fact]
    public void Describe_KeepsUnknownMessage()
    {
        Assert.Equal("something odd", ErrorText.Describe(new InvalidOperationException("something odd")));
    }

    [Fact]
    public void IsKnownIoError_OnlyMatchesActionableIoErrors()
    {
        Assert.True(ErrorText.IsKnownIoError(new UnauthorizedAccessException(DeniedMessage)));
        Assert.True(ErrorText.IsKnownIoError(new PathTooLongException("too long")));
        Assert.False(ErrorText.IsKnownIoError(new InvalidOperationException("not io")));
    }
}
