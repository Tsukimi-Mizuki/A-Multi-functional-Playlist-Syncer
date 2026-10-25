using System.Text;
using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Core.Library;

/// <summary>
/// 歌单歌曲与本地曲库的一对一匹配器。
/// 先按 <see cref="LocalTrack.NeteaseId"/> 精确匹配，再在剩余歌曲上按
/// 标题（0.55）+ 歌手（0.30）+ 时长（0.15）的相似度做贪心一对一匹配。
/// 标题/歌手比较前会做 Unicode NFKC、全角转半角、小写化、去标点与括号内容等归一化。
/// </summary>
public sealed class SongMatcher : ISongMatcher
{
    /// <summary>大于等于该分值才认为模糊匹配成立。</summary>
    public const double DefaultThreshold = 0.72;

    /// <summary>时长容差（毫秒）；差值达到该值时时长得分归零。</summary>
    public const int DurationToleranceMs = 12_000;

    private const double TitleWeight = 0.55;
    private const double ArtistWeight = 0.30;
    private const double DurationWeight = 0.15;

    private static readonly char[] ArtistSeparators = { '/', '&', '、', ';', '；', ',', '，' };

    private static readonly char[] OpenBrackets = { '(', '[', '{', '【', '「', '『', '《', '〈', '〔', '（' };
    private static readonly char[] CloseBrackets = { ')', ']', '}', '】', '」', '』', '》', '〉', '〕', '）' };

    /// <inheritdoc />
    public IReadOnlyList<SongMatch> Match(
        IReadOnlyList<NeteaseSong> playlistSongs,
        IReadOnlyList<LocalTrack> localTracks)
    {
        ArgumentNullException.ThrowIfNull(playlistSongs);
        ArgumentNullException.ThrowIfNull(localTracks);

        var songCount = playlistSongs.Count;
        var localCount = localTracks.Count;
        var results = new SongMatch[songCount];
        if (songCount == 0)
        {
            return results;
        }

        var assigned = new bool[songCount];
        var usedLocal = new bool[localCount];

        // 第一阶段：网易云 ID 精确匹配。
        var localByNeteaseId = new Dictionary<long, int>();
        for (var i = 0; i < localCount; i++)
        {
            if (localTracks[i].NeteaseId is long id && id > 0 && !localByNeteaseId.ContainsKey(id))
            {
                localByNeteaseId[id] = i;
            }
        }

        if (localByNeteaseId.Count > 0)
        {
            for (var songIndex = 0; songIndex < songCount; songIndex++)
            {
                var song = playlistSongs[songIndex];
                if (!localByNeteaseId.TryGetValue(song.Id, out var localIndex) || usedLocal[localIndex])
                {
                    continue;
                }

                usedLocal[localIndex] = true;
                assigned[songIndex] = true;
                results[songIndex] = CreateMatch(song, localTracks[localIndex], MatchMethod.NeteaseId, 1.0);
            }
        }

        // 第二阶段：基于归一化元数据 + 时长的一对一贪心匹配。
        var profiles = new SongProfile[songCount];
        var localProfiles = new LocalProfile[localCount];
        for (var i = 0; i < localCount; i++)
        {
            localProfiles[i] = LocalProfile.Create(localTracks[i]);
        }

        var queue = new PriorityQueue<Candidate, Candidate>(CandidateComparer.Instance);
        for (var songIndex = 0; songIndex < songCount; songIndex++)
        {
            if (assigned[songIndex])
            {
                continue;
            }

            profiles[songIndex] = SongProfile.Create(playlistSongs[songIndex]);
            var best = FindBestCandidate(songIndex, profiles[songIndex], localProfiles, usedLocal);
            if (best is not null)
            {
                queue.Enqueue(best, best);
            }
        }

        while (queue.TryDequeue(out var candidate, out _))
        {
            if (assigned[candidate.SongIndex])
            {
                continue;
            }

            if (usedLocal[candidate.LocalIndex])
            {
                // 该候选的本地曲已被更高分的歌曲占用，重新找当前最优候选。
                var next = FindBestCandidate(
                    candidate.SongIndex,
                    profiles[candidate.SongIndex],
                    localProfiles,
                    usedLocal);
                if (next is not null)
                {
                    queue.Enqueue(next, next);
                }

                continue;
            }

            usedLocal[candidate.LocalIndex] = true;
            assigned[candidate.SongIndex] = true;
            results[candidate.SongIndex] = CreateMatch(
                playlistSongs[candidate.SongIndex],
                localTracks[candidate.LocalIndex],
                MatchMethod.Metadata,
                candidate.Score);
        }

        for (var songIndex = 0; songIndex < songCount; songIndex++)
        {
            results[songIndex] ??= new SongMatch
            {
                Song = playlistSongs[songIndex],
                LocalTrack = null,
                Method = MatchMethod.None,
                Confidence = 0,
                Availability = LocalAvailability.Missing,
            };
        }

        return results;
    }

    private static Candidate? FindBestCandidate(
        int songIndex,
        SongProfile song,
        LocalProfile[] localTracks,
        bool[] usedLocal)
    {
        Candidate? best = null;
        for (var localIndex = 0; localIndex < localTracks.Length; localIndex++)
        {
            if (usedLocal[localIndex])
            {
                continue;
            }

            var score = Score(song, localTracks[localIndex]);
            if (score < DefaultThreshold)
            {
                continue;
            }

            if (best is null || score > best.Score)
            {
                best = new Candidate(songIndex, localIndex, score);
            }
        }

        return best;
    }

    private static SongMatch CreateMatch(NeteaseSong song, LocalTrack localTrack, MatchMethod method, double confidence) =>
        new()
        {
            Song = song,
            LocalTrack = localTrack,
            Method = method,
            Confidence = confidence,
            Availability = localTrack.IsNcm ? LocalAvailability.EncryptedNcm : LocalAvailability.Available,
        };

    /// <summary>综合分：标题 0.55 + 歌手 0.30 + 时长 0.15。</summary>
    private static double Score(SongProfile song, LocalProfile local)
    {
        var titleScore = TitleSimilarity(song, local);
        var artistScore = ArtistSimilarity(song.ArtistTokens, local.ArtistTokens);
        var durationScore = DurationSimilarity(song.DurationMs, local.DurationMs);
        return (TitleWeight * titleScore) + (ArtistWeight * artistScore) + (DurationWeight * durationScore);
    }

    private static double TitleSimilarity(SongProfile song, LocalProfile local)
    {
        if (local.Title.Length == 0)
        {
            return 0;
        }

        var best = 0.0;
        foreach (var title in song.Titles)
        {
            if (title.Length == 0)
            {
                continue;
            }

            best = Math.Max(best, Similarity(title, local.Title));
        }

        return best;
    }

    private static double ArtistSimilarity(IReadOnlySet<string> songArtists, IReadOnlySet<string> localArtists)
    {
        if (songArtists.Count == 0 || localArtists.Count == 0)
        {
            // 未知歌手信息时保持中性，避免因为缺标签否定整首歌曲。
            return 0.5;
        }

        var intersection = 0;
        foreach (var artist in localArtists)
        {
            if (songArtists.Contains(artist))
            {
                intersection++;
            }
        }

        if (intersection > 0)
        {
            if (intersection == songArtists.Count && intersection == localArtists.Count)
            {
                return 1.0;
            }

            var dice = 2.0 * intersection / (songArtists.Count + localArtists.Count);
            return 0.75 + (0.25 * dice);
        }

        // 没有完全相同项时给「近似歌手名」较弱的加分（例如 周杰伦 vs 周杰倫 归一化后仍不同）。
        var best = 0.0;
        foreach (var left in songArtists)
        {
            foreach (var right in localArtists)
            {
                best = Math.Max(best, Similarity(left, right));
            }
        }

        return best * 0.6;
    }

    /// <summary>时长相似度：完全一致为 1，差值达到容差为 0；任一方未知时返回中性 0.5。</summary>
    internal static double DurationSimilarity(long songDurationMs, long localDurationMs)
    {
        if (songDurationMs <= 0 || localDurationMs <= 0)
        {
            return 0.5;
        }

        var difference = Math.Abs(songDurationMs - localDurationMs);
        if (difference >= DurationToleranceMs)
        {
            return 0;
        }

        return 1.0 - ((double)difference / DurationToleranceMs);
    }

    /// <summary>字符串相似度：相等为 1；包含关系按长度比 0.85~1；其余用字符二元组 Dice 系数。</summary>
    internal static double Similarity(string left, string right)
    {
        if (left.Length == 0 || right.Length == 0)
        {
            return 0;
        }

        if (string.Equals(left, right, StringComparison.Ordinal))
        {
            return 1.0;
        }

        var shorter = left.Length <= right.Length ? left : right;
        var longer = left.Length <= right.Length ? right : left;
        if (longer.Contains(shorter, StringComparison.Ordinal))
        {
            var ratio = (double)shorter.Length / longer.Length;
            return 0.85 + (0.15 * ratio);
        }

        return DiceCoefficient(left, right);
    }

    private static double DiceCoefficient(string left, string right)
    {
        if (left.Length < 2 || right.Length < 2)
        {
            return 0;
        }

        var counts = new Dictionary<string, int>(left.Length, StringComparer.Ordinal);
        for (var i = 0; i < left.Length - 1; i++)
        {
            var gram = left.Substring(i, 2);
            counts[gram] = counts.TryGetValue(gram, out var count) ? count + 1 : 1;
        }

        var intersection = 0;
        for (var i = 0; i < right.Length - 1; i++)
        {
            var gram = right.Substring(i, 2);
            if (counts.TryGetValue(gram, out var count) && count > 0)
            {
                counts[gram] = count - 1;
                intersection++;
            }
        }

        return 2.0 * intersection / ((left.Length - 1) + (right.Length - 1));
    }

    /// <summary>标题/歌手比较用的归一化：NFKC（全角→半角）、去括号内容、去标点符号与空白、小写。</summary>
    internal static string NormalizeForMatch(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return string.Empty;
        }

        var normalized = value.Normalize(NormalizationForm.FormKC);
        var builder = new StringBuilder(normalized.Length);
        var bracketDepth = 0;
        foreach (var c in normalized)
        {
            if (Array.IndexOf(OpenBrackets, c) >= 0)
            {
                bracketDepth++;
                continue;
            }

            if (Array.IndexOf(CloseBrackets, c) >= 0)
            {
                if (bracketDepth > 0)
                {
                    bracketDepth--;
                }

                continue;
            }

            if (bracketDepth > 0)
            {
                continue;
            }

            if (char.IsWhiteSpace(c) || char.IsControl(c))
            {
                continue;
            }

            if (char.IsPunctuation(c) || char.IsSymbol(c))
            {
                continue;
            }

            builder.Append(char.ToLowerInvariant(c));
        }

        return builder.ToString();
    }

    /// <summary>按 / &amp; 、 ; 等分隔符拆分歌手串并归一化，返回去重后的集合。</summary>
    internal static IReadOnlySet<string> SplitArtistTokens(string? artists)
    {
        var tokens = new HashSet<string>(StringComparer.Ordinal);
        if (string.IsNullOrWhiteSpace(artists))
        {
            return tokens;
        }

        foreach (var token in artists.Split(ArtistSeparators, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            var normalized = NormalizeForMatch(token);
            if (normalized.Length > 0)
            {
                tokens.Add(normalized);
            }
        }

        return tokens;
    }

    private sealed class SongProfile
    {
        public required string[] Titles { get; init; }
        public required IReadOnlySet<string> ArtistTokens { get; init; }
        public long DurationMs { get; init; }

        public static SongProfile Create(NeteaseSong song)
        {
            var titles = new List<string>(2);
            var name = NormalizeForMatch(song.Name);
            if (name.Length > 0)
            {
                titles.Add(name);
            }

            if (!string.IsNullOrWhiteSpace(song.Alias))
            {
                var alias = NormalizeForMatch(song.Alias);
                if (alias.Length > 0 && !titles.Contains(alias, StringComparer.Ordinal))
                {
                    titles.Add(alias);
                }
            }

            var artists = new HashSet<string>(StringComparer.Ordinal);
            foreach (var artist in song.Artists)
            {
                foreach (var token in SplitArtistTokens(artist.Name))
                {
                    artists.Add(token);
                }
            }

            return new SongProfile
            {
                Titles = titles.ToArray(),
                ArtistTokens = artists,
                DurationMs = song.DurationMs,
            };
        }
    }

    private sealed class LocalProfile
    {
        public required string Title { get; init; }
        public required IReadOnlySet<string> ArtistTokens { get; init; }
        public long DurationMs { get; init; }

        public static LocalProfile Create(LocalTrack track) => new()
        {
            Title = NormalizeForMatch(track.Title),
            ArtistTokens = SplitArtistTokens(track.Artist),
            DurationMs = track.DurationMs,
        };
    }

    private sealed class Candidate
    {
        public Candidate(int songIndex, int localIndex, double score)
        {
            SongIndex = songIndex;
            LocalIndex = localIndex;
            Score = score;
        }

        public int SongIndex { get; }
        public int LocalIndex { get; }
        public double Score { get; }
    }

    private sealed class CandidateComparer : IComparer<Candidate>
    {
        public static readonly CandidateComparer Instance = new();

        public int Compare(Candidate? x, Candidate? y)
        {
            if (ReferenceEquals(x, y))
            {
                return 0;
            }

            if (x is null)
            {
                return 1;
            }

            if (y is null)
            {
                return -1;
            }

            var byScore = y.Score.CompareTo(x.Score);
            if (byScore != 0)
            {
                return byScore;
            }

            var bySong = x.SongIndex.CompareTo(y.SongIndex);
            return bySong != 0 ? bySong : x.LocalIndex.CompareTo(y.LocalIndex);
        }
    }
}
