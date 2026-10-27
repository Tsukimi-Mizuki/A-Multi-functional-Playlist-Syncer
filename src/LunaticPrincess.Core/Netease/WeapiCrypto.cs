using System.Globalization;
using System.Numerics;
using System.Security.Cryptography;
using System.Text;

namespace LunaticPrincess.Core.Netease;

/// <summary>
/// 网易云 weapi 请求体加密。算法与社区实现一致：
/// params = Base64(AES-128-CBC(Base64(AES-128-CBC(业务 JSON, 固定 key)), 随机 secret))，
/// encSecKey = 教材式 RSA（PKCS#1 v1.5 无填充，即 m^e mod n）加密反转后的 secret，输出 256 位小写 hex。
/// </summary>
internal static class WeapiCrypto
{
    /// <summary>weapi 内层固定 AES key。</summary>
    internal const string InnerKey = "0CoJUm6Qyw8W8jud";

    /// <summary>两层 AES-CBC 共用的初始向量。</summary>
    internal const string Iv = "0102030405060708";

    /// <summary>网易云固定 RSA 模数（2048 bit，带一个前导 00 保证 BigInteger 正数解析）。</summary>
    internal const string ModulusHex =
        "00e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b725152b3ab17a876aea8a5aa76d2e417629ec4ee341f56135fccf695280104e0312ecbda92557c93870114af6c9d05c4f7f0c3685b7a46bee255932575cce10b424d813cfe4875d3e82047b97ddef52741d546b8e289dc6935b3ece0462db0a22b8e7";

    /// <summary>网易云固定 RSA 公钥指数。</summary>
    internal const string ExponentHex = "010001";

    /// <summary>随机 secret 长度（16 个 [A-Za-z0-9] 字符）。</summary>
    internal const int SecretLength = 16;

    private const int EncSecKeyHexLength = 256;

    private const string SecretAlphabet = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

    private static readonly BigInteger Modulus = BigInteger.Parse(ModulusHex, NumberStyles.HexNumber, CultureInfo.InvariantCulture);
    private static readonly BigInteger Exponent = BigInteger.Parse(ExponentHex, NumberStyles.HexNumber, CultureInfo.InvariantCulture);

    /// <summary>生成一个符合网易云要求的 16 位随机 secret（[A-Za-z0-9]）。</summary>
    public static string CreateSecret()
    {
        var chars = new char[SecretLength];
        for (var i = 0; i < chars.Length; i++)
        {
            chars[i] = SecretAlphabet[RandomNumberGenerator.GetInt32(SecretAlphabet.Length)];
        }

        return new string(chars);
    }

    /// <summary>把业务 JSON 加密成 weapi 表单需要的 params/encSecKey。</summary>
    public static (string Params, string EncSecKey) Encrypt(string businessJson, string secret)
    {
        ArgumentNullException.ThrowIfNull(businessJson);
        if (string.IsNullOrEmpty(secret) || secret.Length != SecretLength)
        {
            throw new ArgumentException($"weapi secret 必须是 {SecretLength} 个字符。", nameof(secret));
        }

        var inner = AesEncryptToBase64(businessJson, InnerKey);
        var outer = AesEncryptToBase64(inner, secret);
        return (outer, RsaEncryptHex(secret));
    }

    /// <summary>两层 AES-128-CBC 解密，仅供单元测试与问题定位使用。</summary>
    internal static string DecryptPayload(string paramsBase64, string secret)
    {
        var inner = AesDecryptFromBase64(paramsBase64, secret);
        return AesDecryptFromBase64(inner, InnerKey);
    }

    /// <summary>AES-128-CBC（PKCS#7）加密并返回 Base64。</summary>
    internal static string AesEncryptToBase64(string plainText, string key)
    {
        using var aes = Aes.Create();
        aes.Mode = CipherMode.CBC;
        aes.Padding = PaddingMode.PKCS7;
        aes.Key = Encoding.UTF8.GetBytes(key);
        aes.IV = Encoding.UTF8.GetBytes(Iv);
        using var encryptor = aes.CreateEncryptor();
        var plainBytes = Encoding.UTF8.GetBytes(plainText);
        var cipherBytes = encryptor.TransformFinalBlock(plainBytes, 0, plainBytes.Length);
        return Convert.ToBase64String(cipherBytes);
    }

    /// <summary>AES-128-CBC（PKCS#7）解密 Base64，仅供单元测试与问题定位使用。</summary>
    internal static string AesDecryptFromBase64(string cipherBase64, string key)
    {
        using var aes = Aes.Create();
        aes.Mode = CipherMode.CBC;
        aes.Padding = PaddingMode.PKCS7;
        aes.Key = Encoding.UTF8.GetBytes(key);
        aes.IV = Encoding.UTF8.GetBytes(Iv);
        using var decryptor = aes.CreateDecryptor();
        var cipherBytes = Convert.FromBase64String(cipherBase64);
        var plainBytes = decryptor.TransformFinalBlock(cipherBytes, 0, cipherBytes.Length);
        return Encoding.UTF8.GetString(plainBytes);
    }

    /// <summary>教材式 RSA（无填充）加密 secret，输出 256 位小写 hex。</summary>
    public static string RsaEncryptHex(string secret)
    {
        ArgumentException.ThrowIfNullOrEmpty(secret);

        // 网易云要求把 secret 逐字符反转后再做 RSA（服务端会反转回来）。
        var reversed = secret.ToCharArray();
        Array.Reverse(reversed);
        var message = new BigInteger(Encoding.UTF8.GetBytes(new string(reversed)), isUnsigned: true, isBigEndian: true);
        var cipher = BigInteger.ModPow(message, Exponent, Modulus);
        // 注意：BigInteger 的 "x" 格式对最高位为 1 的正数会补一个前导 0（避免与负数混淆），
        // 而网易云要求与 JS BigInt.toString(16) 一致的纯无符号 hex，因此先去掉前导 0 再补齐 256 位。
        var hex = cipher.ToString("x", CultureInfo.InvariantCulture).TrimStart('0');
        return hex.PadLeft(EncSecKeyHexLength, '0');
    }
}
