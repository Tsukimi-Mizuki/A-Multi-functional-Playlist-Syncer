using System.Text;

namespace LunaticPrincess.Core.Lyrics;

/// <summary>
/// QQ 音乐 QRC 使用的 DES 变体（与 LDDC / QQMusicDecoder 的 tripledes 实现一致）。
/// </summary>
/// <remarks>
/// <para>该变体与教科书 DES 的差异只在字节与位的编号约定（32 位字按小端存放），因此
/// <see cref="System.Security.Cryptography.TripleDES"/> 的结果与之不一致；这里按位运算逐轮实现，
/// 以便与 QQ 音乐服务端产生的密文对齐。</para>
/// <para>仅用于 QRC 歌词解密：8 字节一块、无填充、按 ECB 语义逐块处理。</para>
/// </remarks>
internal static class QqTripleDes
{
    /// <summary>QRC 固定密钥（24 字节）。</summary>
    internal static readonly byte[] QrcKey = Encoding.ASCII.GetBytes("!@#)(*$%123ZXC!@!@#)(NHL");

    private static readonly int[][] Sbox =
    {
        // S1
        new[]
        {
            14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7,
            0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8,
            4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0,
            15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
        },
        // S2
        new[]
        {
            15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10,
            3, 13, 4, 7, 15, 2, 8, 15, 12, 0, 1, 10, 6, 9, 11, 5,
            0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15,
            13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
        },
        // S3
        new[]
        {
            10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8,
            13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1,
            13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7,
            1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
        },
        // S4
        new[]
        {
            7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15,
            13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9,
            10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4,
            3, 15, 0, 6, 10, 10, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
        },
        // S5
        new[]
        {
            2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9,
            14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6,
            4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14,
            11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
        },
        // S6
        new[]
        {
            12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11,
            10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8,
            9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6,
            4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
        },
        // S7
        new[]
        {
            4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1,
            13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6,
            1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2,
            6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
        },
        // S8
        new[]
        {
            13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7,
            1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2,
            7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8,
            2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
        },
    };

    private static readonly int[] KeyPermC =
    {
        56, 48, 40, 32, 24, 16, 8, 0, 57, 49, 41, 33, 25, 17, 9, 1,
        58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35,
    };

    private static readonly int[] KeyPermD =
    {
        62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29, 21, 13, 5,
        60, 52, 44, 36, 28, 20, 12, 4, 27, 19, 11, 3,
    };

    private static readonly int[] KeyCompression =
    {
        13, 16, 10, 23, 0, 4, 2, 27, 14, 5, 20, 9, 22, 18, 11, 3,
        25, 7, 15, 6, 26, 19, 12, 1, 40, 51, 30, 36, 46, 54, 29, 39,
        50, 44, 32, 47, 43, 48, 38, 55, 33, 52, 45, 41, 49, 35, 28, 31,
    };

    private static readonly int[] Permutation =
    {
        15, 6, 19, 20, 28, 11, 27, 16, 0, 14, 22, 25, 4, 17, 30, 9,
        1, 7, 23, 13, 31, 26, 2, 8, 18, 12, 29, 5, 21, 10, 3, 24,
    };

    /// <summary>按 QRC 语义解密：每 8 字节一块，共 <paramref name="length"/> 字节。</summary>
    internal static byte[] Decrypt(byte[] key, byte[] cipher, int length)
    {
        var schedule = TripleDesKeySetup(key, decrypt: true);
        var output = new byte[length];
        var block = new byte[8];
        for (var offset = 0; offset < length; offset += 8)
        {
            Buffer.BlockCopy(cipher, offset, block, 0, 8);
            for (var round = 0; round < 3; round++)
            {
                block = Crypt(block, schedule[round]);
            }

            Buffer.BlockCopy(block, 0, output, offset, 8);
        }

        return output;
    }

    /// <summary>按 QRC 语义加密（与 <see cref="Decrypt"/> 对称，用于自检与测试）。</summary>
    internal static byte[] Encrypt(byte[] key, byte[] plain, int length)
    {
        var schedule = TripleDesKeySetup(key, decrypt: false);
        var output = new byte[length];
        var block = new byte[8];
        for (var offset = 0; offset < length; offset += 8)
        {
            Buffer.BlockCopy(plain, offset, block, 0, 8);
            for (var round = 0; round < 3; round++)
            {
                block = Crypt(block, schedule[round]);
            }

            Buffer.BlockCopy(block, 0, output, offset, 8);
        }

        return output;
    }

    /// <summary>单 DES 加密一个 8 字节块（仅用于校验实现正确性）。</summary>
    internal static byte[] EncryptBlock(byte[] key, byte[] block)
    {
        var schedule = KeySchedule(key, decrypt: false);
        return Crypt(block, schedule);
    }

    private static uint[][][] TripleDesKeySetup(byte[] key, bool decrypt)
    {
        if (decrypt)
        {
            return new uint[][][]
            {
                KeySchedule(key[16..24], decrypt: true),
                KeySchedule(key[8..16], decrypt: false),
                KeySchedule(key[0..8], decrypt: true),
            };
        }

        return new uint[][][]
        {
            KeySchedule(key[0..8], decrypt: false),
            KeySchedule(key[8..16], decrypt: true),
            KeySchedule(key[16..24], decrypt: false),
        };
    }

    private static uint[][] KeySchedule(byte[] key, bool decrypt)
    {
        var schedule = new uint[16][];
        var shifts = new[] { 1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1 };
        var c = 0u;
        var d = 0u;
        for (var i = 0; i < 28; i++)
        {
            c |= BitNum(key, KeyPermC[i], 31 - i);
            d |= BitNum(key, KeyPermD[i], 31 - i);
        }

        for (var i = 0; i < 16; i++)
        {
            var shift = shifts[i];
            c = ((c << shift) | (c >> (28 - shift))) & 0xfffffff0u;
            d = ((d << shift) | (d >> (28 - shift))) & 0xfffffff0u;
            var target = decrypt ? 15 - i : i;
            var subKeys = new uint[6];
            for (var j = 0; j < 24; j++)
            {
                subKeys[j / 8] |= BitNumIntr(c, KeyCompression[j], 7 - (j % 8));
            }

            for (var j = 24; j < 48; j++)
            {
                subKeys[j / 8] |= BitNumIntr(d, KeyCompression[j] - 27, 7 - (j % 8));
            }

            schedule[target] = subKeys;
        }

        return schedule;
    }

    private static byte[] Crypt(byte[] block, uint[][] schedule)
    {
        var (left, right) = InitialPermutation(block);
        for (var i = 0; i < 15; i++)
        {
            var previous = right;
            right = F(right, schedule[i]) ^ left;
            left = previous;
        }

        left = F(right, schedule[15]) ^ left;
        return InversePermutation(left, right);
    }

    private static uint BitNum(byte[] data, int bit, int shift) =>
        (uint)(((data[(bit / 32) * 4 + 3 - (bit % 32) / 8] >> (7 - (bit % 8))) & 1) << shift);

    private static uint BitNumIntr(uint value, int bit, int shift) =>
        ((value >> (31 - bit)) & 1) << shift;

    private static uint BitNumIntl(uint value, int bit, int shift) =>
        ((value << bit) & 0x80000000u) >> shift;

    private static int SboxBit(int value) => (value & 32) | ((value & 31) >> 1) | ((value & 1) << 4);

    private static (uint Left, uint Right) InitialPermutation(byte[] data)
    {
        var left = 0u;
        var right = 0u;
        for (var i = 0; i < 32; i++)
        {
            var shift = 31 - i;
            var group = i / 8;
            var within = i % 8;
            left |= BitNum(data, 57 - (8 * within) + (2 * group), shift);
            right |= BitNum(data, 56 - (8 * within) + (2 * group), shift);
        }

        return (left, right);
    }

    private static byte[] InversePermutation(uint left, uint right)
    {
        var data = new byte[8];
        for (var index = 0; index < 8; index++)
        {
            var sourceBit = (index + 4) % 8;
            uint value = 0;
            for (var term = 0; term < 8; term++)
            {
                var source = term % 2 == 0 ? right : left;
                value |= BitNumIntr(source, sourceBit + (8 * (term / 2)), 7 - term);
            }

            data[index] = (byte)value;
        }

        return data;
    }

    private static uint F(uint state, uint[] key)
    {
        var t1 = BitNumIntl(state, 31, 0) | ((state & 0xf0000000u) >> 1) | BitNumIntl(state, 4, 5) |
                 BitNumIntl(state, 3, 6) | ((state & 0x0f000000u) >> 3) | BitNumIntl(state, 8, 11) |
                 BitNumIntl(state, 7, 12) | ((state & 0x00f00000u) >> 5) | BitNumIntl(state, 12, 17) |
                 BitNumIntl(state, 11, 18) | ((state & 0x000f0000u) >> 7) | BitNumIntl(state, 16, 23);

        var t2 = BitNumIntl(state, 15, 0) | ((state & 0x0000f000u) << 15) | BitNumIntl(state, 20, 5) |
                 BitNumIntl(state, 19, 6) | ((state & 0x00000f00u) << 13) | BitNumIntl(state, 24, 11) |
                 BitNumIntl(state, 23, 12) | ((state & 0x000000f0u) << 11) | BitNumIntl(state, 28, 17) |
                 BitNumIntl(state, 27, 18) | ((state & 0x0000000fu) << 9) | BitNumIntl(state, 0, 23);

        var l0 = (int)(((t1 >> 24) & 0xff) ^ key[0]);
        var l1 = (int)(((t1 >> 16) & 0xff) ^ key[1]);
        var l2 = (int)(((t1 >> 8) & 0xff) ^ key[2]);
        var l3 = (int)(((t2 >> 24) & 0xff) ^ key[3]);
        var l4 = (int)(((t2 >> 16) & 0xff) ^ key[4]);
        var l5 = (int)(((t2 >> 8) & 0xff) ^ key[5]);

        var s = ((uint)Sbox[0][SboxBit(l0 >> 2)] << 28) |
                ((uint)Sbox[1][SboxBit(((l0 & 3) << 4) | (l1 >> 4))] << 24) |
                ((uint)Sbox[2][SboxBit(((l1 & 0xf) << 2) | (l2 >> 6))] << 20) |
                ((uint)Sbox[3][SboxBit(l2 & 0x3f)] << 16) |
                ((uint)Sbox[4][SboxBit(l3 >> 2)] << 12) |
                ((uint)Sbox[5][SboxBit(((l3 & 3) << 4) | (l4 >> 4))] << 8) |
                ((uint)Sbox[6][SboxBit(((l4 & 0xf) << 2) | (l5 >> 6))] << 4) |
                (uint)Sbox[7][SboxBit(l5 & 0x3f)];

        var result = 0u;
        for (var i = 0; i < 32; i++)
        {
            result |= BitNumIntl(s, Permutation[i], i);
        }

        return result;
    }
}
