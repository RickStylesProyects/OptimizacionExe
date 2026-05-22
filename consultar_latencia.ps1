$signature = @'
[DllImport("ntdll.dll")]
public static extern int NtQueryTimerResolution(out uint MinimumResolution, out uint MaximumResolution, out uint CurrentResolution);
'@

$type = Add-Type -MemberDefinition $signature -Name "NtTimer" -Namespace "Win32" -PassThru
$min = [uint32]0
$max = [uint32]0
$curr = [uint32]0

$result = $type::NtQueryTimerResolution([ref]$min, [ref]$max, [ref]$curr)
if ($result -eq 0) {
    Write-Output "Minimum Resolution: $($min / 10000) ms"
    Write-Output "Maximum Resolution: $($max / 10000) ms"
    Write-Output "Current Resolution: $($curr / 10000) ms"
} else {
    Write-Output "Failed to query system timer resolution. NTSTATUS: $result"
}
