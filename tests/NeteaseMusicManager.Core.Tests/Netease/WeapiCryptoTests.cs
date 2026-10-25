using NeteaseMusicManager.Core.Netease;
using Xunit;

namespace NeteaseMusicManager.Core.Tests.Netease;

public sealed class WeapiCryptoTests
{
    // 参考向量由独立实现（Node.js crypto + BigInt）生成，锁定 AES/双次加密/RSA 全链路。
    private const string ReferenceJson = """{"csrf_token":"","uid":"1"}""";
    private const string ReferenceSecret = "0123456789abcdef";
    private const string ReferenceParams = "echuH06dDs7OxMA2egejtndmqu1aYuHshFuQMQCaqlxZeX1+XSQpJU0kXp3uWfRV";
    private const string ReferenceEncSecKey =
        "35701388baf89fed412e11269b9c76625d095ecaf17f03fa018abe19ea2d38b949debf242ee39a71ca1f6cda71b1b86a45aa909ee27f7e78e267d34e732f0de948206c3340a788d0003372183e2f753c1f78b66ac23d134ac1fc9b993156520ea826b8aa89a962d4491b4b8d7e08738e1da9b07aa39bf4a7ef0b1c210728cd52";

    [Fact]
    public void Encrypt_MatchesIndependentReferenceImplementation()
    {
        var (encryptedParams, encSecKey) = WeapiCrypto.Encrypt(ReferenceJson, ReferenceSecret);

        Assert.Equal(ReferenceParams, encryptedParams);
        Assert.Equal(ReferenceEncSecKey, encSecKey);
    }

    [Fact]
    public void Encrypt_MatchesSecondIndependentVector()
    {
        var (encryptedParams, encSecKey) = WeapiCrypto.Encrypt("""{"csrf_token":"csrf123","id":42}""", "Zz9AaBbCcDdEeFfG");

        Assert.Equal(
            "Q3pZQ8xAhNJnBJgOUdQz1jwOVFR8dSzgq58qXtA66cHu/2GBxCYvvBEY201jheFLoR4yV/QrTnhU0Ns0nObaNHyX8NVYjkRKsJf2dvLEIUc=",
            encryptedParams);
        Assert.Equal(
            "71c93ba0b9f0fc3dab23882d21a7ea8e199a54b2166e1e6d5412ddf04a007e9a0a22d498c725598892e3695d6d03c43b63971aec5bf84fe21c6492acfbc18586c929aa6b78f4d8b31c6061ee1a38c0f66423c0071426219a50a4b5a17088d09f255b581c6619023c8937d7869979c153b23029202071209a03a2d8bb0503197c",
            encSecKey);
    }

    [Fact]
    public void DecryptPayload_RoundTripsWeapiPlaintext()
    {
        var (encryptedParams, _) = WeapiCrypto.Encrypt(ReferenceJson, ReferenceSecret);

        var decrypted = WeapiCrypto.DecryptPayload(encryptedParams, ReferenceSecret);

        Assert.Equal(ReferenceJson, decrypted);
    }

    [Fact]
    public void RsaEncryptHex_Is256LowercaseHexCharacters()
    {
        foreach (var secret in new[] { ReferenceSecret, "abcdefghijklmnop", "0123456789ABCDEF", "Zz9AaBbCcDdEeFfG" })
        {
            var encrypted = WeapiCrypto.RsaEncryptHex(secret);

            Assert.Equal(256, encrypted.Length);
            Assert.Matches("^[0-9a-f]{256}$", encrypted);
        }
    }

    [Fact]
    public void RsaEncryptHex_IsDeterministicForSameSecret()
    {
        var first = WeapiCrypto.RsaEncryptHex(ReferenceSecret);
        var second = WeapiCrypto.RsaEncryptHex(ReferenceSecret);

        Assert.Equal(first, second);
    }

    [Fact]
    public void CreateSecret_Returns16AlphanumericCharacters()
    {
        var secrets = Enumerable.Range(0, 32).Select(_ => WeapiCrypto.CreateSecret()).ToArray();

        Assert.All(secrets, secret => Assert.Matches("^[A-Za-z0-9]{16}$", secret));
        Assert.True(secrets.Distinct().Count() > 1, "随机 secret 不应重复");
    }

    [Fact]
    public void Encrypt_RejectsNon16CharacterSecret()
    {
        Assert.Throws<ArgumentException>(() => WeapiCrypto.Encrypt(ReferenceJson, "too-short"));
        Assert.Throws<ArgumentException>(() => WeapiCrypto.Encrypt(ReferenceJson, "12345678901234567"));
    }

    [Fact]
    public void Modulus_Is2048BitNetEaseKey()
    {
        Assert.Equal(258, WeapiCrypto.ModulusHex.Length); // 2048 bit = 256 hex + 前导 00
        Assert.StartsWith("00e0b509f6", WeapiCrypto.ModulusHex);
        Assert.EndsWith("b0a22b8e7", WeapiCrypto.ModulusHex);
        Assert.Equal("010001", WeapiCrypto.ExponentHex);
        Assert.Equal(16, WeapiCrypto.SecretLength);
    }
}
