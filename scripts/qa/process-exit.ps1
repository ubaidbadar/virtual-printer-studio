# Windows PowerShell 5.1 can lose Process.ExitCode after Refresh/MainWindowHandle
# queries. Cache the native handle while alive, then query the actual kernel exit code.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class VpsQaProcess {
 private delegate bool EnumWindowProc(IntPtr window,IntPtr parameter);
 [StructLayout(LayoutKind.Sequential)] private struct Rect { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowProc callback,IntPtr parameter);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr window,System.Text.StringBuilder text,int capacity);
 [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")] private static extern bool GetClientRect(IntPtr window,out Rect rectangle);
 [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
 public static IntPtr FindAppWindow(int processId,string title) {
  IntPtr result=IntPtr.Zero;
  EnumWindows(delegate(IntPtr window,IntPtr parameter){
   uint owner; GetWindowThreadProcessId(window,out owner);
   if(owner!=(uint)processId || !IsWindowVisible(window))return true;
   var name=new System.Text.StringBuilder(512);GetWindowText(window,name,name.Capacity);
   Rect area;if(name.ToString()!=title || !GetClientRect(window,out area) || area.Right-area.Left<640 || area.Bottom-area.Top<480)return true;
   result=window;return false;
  },IntPtr.Zero);
  return result;
 }
 [DllImport("kernel32.dll",SetLastError=true)]
 public static extern bool GetExitCodeProcess(IntPtr process,out uint exitCode);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr window,int command);
}
'@
function Wait-VpsProcess([Diagnostics.Process]$Process,[IntPtr]$NativeHandle,[int]$TimeoutMilliseconds=120000) {
    if(!$Process.WaitForExit($TimeoutMilliseconds)){$Process.Kill();throw 'Application process timed out.'}
    [uint32]$code=259
    if(![VpsQaProcess]::GetExitCodeProcess($NativeHandle,[ref]$code)){throw 'Cannot query the native application exit code.'}
    return $code
}
