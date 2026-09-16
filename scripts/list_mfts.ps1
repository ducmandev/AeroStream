$key = 'HKLM:\SOFTWARE\Classes\MediaFoundation\Transforms\Categories\f79eac7d-e545-4387-bdee-d647d7bde42a'
Get-ChildItem $key | ForEach-Object {
    $guid = $_.PSChildName
    $prop = Get-ItemProperty "HKLM:\SOFTWARE\Classes\MediaFoundation\Transforms\$guid" -ErrorAction SilentlyContinue
    [PSCustomObject]@{
        Guid = $guid
        Name = $prop.'(default)'
    }
}
