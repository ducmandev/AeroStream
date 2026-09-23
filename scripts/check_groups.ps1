Get-LocalGroup | ForEach-Object {
    $g = $_.Name
    Get-LocalGroupMember -Group $g -ErrorAction SilentlyContinue | ForEach-Object {
        [PSCustomObject]@{
            Group = $g
            Member = $_.Name
            ObjectClass = $_.ObjectClass
        }
    }
} | Format-Table -AutoSize
