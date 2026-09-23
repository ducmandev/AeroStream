$cert = Get-ChildItem -Path Cert:\LocalMachine\My -CodeSigningCert -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $cert) {
    Write-Host 'Creating code signing cert...'
    $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=AeroStream UIPI' -CertStoreLocation Cert:\LocalMachine\My
    $rootStore = [System.Security.Cryptography.X509Certificates.X509Store]::new('Root', 'LocalMachine')
    $rootStore.Open('ReadWrite')
    $rootStore.Add($cert)
    $rootStore.Close()
    Write-Host 'Cert created and added to Trusted Root.'
} else {
    Write-Host 'Existing cert found.'
}
Write-Host "Thumbprint: $($cert.Thumbprint)"
