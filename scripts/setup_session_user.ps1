$ErrorActionPreference = "Continue"
$username = "aerostream_remote"
$password = "AeroStream#123"

# Check if user already exists
$existing = Get-LocalUser -Name $username -ErrorAction SilentlyContinue
if (-not $existing) {
    Write-Host "Creating user $username..."
    $sec = ConvertTo-SecureString $password -AsPlainText -Force
    try {
        New-LocalUser -Name $username -Password $sec -Description "AeroStream Session Mode Secondary User" -PasswordNeverExpires -ErrorAction Stop
        Write-Host "User $username created successfully!"
    } catch {
        Write-Warning "Failed to create user directly (need Admin): $_"
        Write-Host "Trying via net user with echo Y..."
        cmd.exe /c "echo Y | net user $username $password /add /comment:`"AeroStream Session Mode Secondary User`""
    }
} else {
    Write-Host "User $username already exists."
}

# Add to Remote Desktop Users
try {
    Add-LocalGroupMember -Group "Remote Desktop Users" -Member $username -ErrorAction Stop
    Write-Host "Added $username to 'Remote Desktop Users'."
} catch {
    Write-Warning "Failed to add to group directly: $_"
    cmd.exe /c "net localgroup `"Remote Desktop Users`" $username /add"
}

# Verify
$check = Get-LocalUser -Name $username -ErrorAction SilentlyContinue
if ($check) {
    Write-Host "VERIFICATION: User exists! Enabled: $($check.Enabled)"
    $members = Get-LocalGroupMember -Group "Remote Desktop Users" -ErrorAction SilentlyContinue | Where-Object { $_.Name -like "*$username*" }
    if ($members) {
        Write-Host "VERIFICATION: User is in 'Remote Desktop Users'!"
    } else {
        Write-Warning "VERIFICATION: User is NOT yet in 'Remote Desktop Users'."
    }
} else {
    Write-Warning "VERIFICATION: User does not exist."
}
