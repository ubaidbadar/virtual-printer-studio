# Windows PowerShell 5.1 can lose Process.ExitCode after Refresh/MainWindowHandle
# queries. Cache the native handle while alive, then query the actual kernel exit code.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class VpsQaProcess {
 [DllImport("kernel32.dll",SetLastError=true)]
 public static extern bool GetExitCodeProcess(IntPtr process,out uint exitCode);
}
'@
function Wait-VpsProcess([Diagnostics.Process]$Process,[IntPtr]$NativeHandle,[int]$TimeoutMilliseconds=120000) {
    if(!$Process.WaitForExit($TimeoutMilliseconds)){$Process.Kill();throw 'Application process timed out.'}
    [uint32]$code=259
    if(![VpsQaProcess]::GetExitCodeProcess($NativeHandle,[ref]$code)){throw 'Cannot query the native application exit code.'}
    return $code
}
