# Generates a fresh untrusted certificate and ephemeral key for the local Python TLS fixture.
$ErrorActionPreference = 'Stop'

$rsa = [System.Security.Cryptography.RSACng]::new(2048)
if (-not $rsa.Key.IsEphemeral) { throw 'Synthetic key must be ephemeral.' }
$request = [System.Security.Cryptography.X509Certificates.CertificateRequest]::new(
    'CN=syrinx.invalid', $rsa, [System.Security.Cryptography.HashAlgorithmName]::SHA256,
    [System.Security.Cryptography.RSASignaturePadding]::Pkcs1)
$names = [System.Security.Cryptography.X509Certificates.SubjectAlternativeNameBuilder]::new()
$names.AddIpAddress([System.Net.IPAddress]::Loopback)
$request.CertificateExtensions.Add($names.Build())
$certificate = $request.CreateSelfSigned([DateTimeOffset]::UtcNow.AddMinutes(-5), [DateTimeOffset]::UtcNow.AddHours(1))
try {
    $parameters = $rsa.ExportParameters($true)
    @{
        certificate = [Convert]::ToBase64String($certificate.RawData)
        modulus = [Convert]::ToBase64String($parameters.Modulus)
        exponent = [Convert]::ToBase64String($parameters.Exponent)
        d = [Convert]::ToBase64String($parameters.D)
        p = [Convert]::ToBase64String($parameters.P)
        q = [Convert]::ToBase64String($parameters.Q)
        dp = [Convert]::ToBase64String($parameters.DP)
        dq = [Convert]::ToBase64String($parameters.DQ)
        iq = [Convert]::ToBase64String($parameters.InverseQ)
    } | ConvertTo-Json -Compress
} finally {
    $certificate.Dispose()
    $rsa.Dispose()
}
