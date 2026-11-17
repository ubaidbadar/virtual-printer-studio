param(
    [Parameter(Mandatory=$true)][string]$PrinterName,
    [Parameter(Mandatory=$true)][string]$FormName,
    [Parameter(Mandatory=$true)][string]$Report,
    [switch]$Remove
)
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class VpsCustomPaper {
 [StructLayout(LayoutKind.Sequential)] struct Defaults { public IntPtr DataType, DevMode; public uint Access; }
 [StructLayout(LayoutKind.Sequential)] struct Size { public int Width, Height; }
 [StructLayout(LayoutKind.Sequential)] struct Rect { public int Left, Top, Right, Bottom; }
 [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct Form { public uint Flags; [MarshalAs(UnmanagedType.LPWStr)] public string Name; public Size Size; public Rect Area; }
 [DllImport("winspool.drv",EntryPoint="OpenPrinterW",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool OpenPrinter(string name,out IntPtr handle,ref Defaults defaults);
 [DllImport("winspool.drv",EntryPoint="AddFormW",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool AddForm(IntPtr handle,uint level,ref Form form);
 [DllImport("winspool.drv",EntryPoint="DeleteFormW",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool DeleteForm(IntPtr handle,string form);
 [DllImport("winspool.drv",EntryPoint="DocumentPropertiesW",CharSet=CharSet.Unicode,SetLastError=true)] static extern int DocumentProperties(IntPtr window,IntPtr printer,string name,IntPtr output,IntPtr input,uint mode);
 [DllImport("winspool.drv",EntryPoint="SetPrinterW",SetLastError=true)] static extern bool SetPrinter(IntPtr printer,uint level,IntPtr info,uint command);
 [DllImport("winspool.drv")] static extern bool ClosePrinter(IntPtr handle);
 static IntPtr Open(string name,uint access) { IntPtr handle;Defaults defaults=new Defaults();defaults.Access=access;if(!OpenPrinter(name,out handle,ref defaults))throw new Win32Exception(Marshal.GetLastWin32Error());return handle; }
 public static void Remove(string formName) { IntPtr server=Open(null,1);try{if(!DeleteForm(server,formName)){int error=Marshal.GetLastWin32Error();if(error!=1902)throw new Win32Exception(error);}}finally{ClosePrinter(server);} }
 public static object Probe(string printerName,string formName) {
  IntPtr server=Open(null,1);
  try { Form form=new Form();form.Name=formName;form.Size.Width=80000;form.Size.Height=150000;form.Area.Right=80000;form.Area.Bottom=150000;if(!AddForm(server,1,ref form))throw new Win32Exception(Marshal.GetLastWin32Error()); }
  finally { ClosePrinter(server); }
  IntPtr printer=Open(printerName,12),buffer=IntPtr.Zero,info=IntPtr.Zero;
  try {
   int size=DocumentProperties(IntPtr.Zero,printer,printerName,IntPtr.Zero,IntPtr.Zero,0);
   if(size<220)throw new Exception("Driver returned an invalid DEVMODE size.");
   buffer=Marshal.AllocHGlobal(size);
   if(DocumentProperties(IntPtr.Zero,printer,printerName,buffer,IntPtr.Zero,2)!=1)throw new Exception("Could not read printer settings.");
   int fields=Marshal.ReadInt32(buffer,72);
   Marshal.WriteInt32(buffer,72,fields|2|4|8|0x10000);
   Marshal.WriteInt16(buffer,78,256);Marshal.WriteInt16(buffer,80,1500);Marshal.WriteInt16(buffer,82,800);
   byte[] name=new byte[64];byte[] encoded=System.Text.Encoding.Unicode.GetBytes(formName);Array.Copy(encoded,name,Math.Min(encoded.Length,62));Marshal.Copy(name,0,IntPtr.Add(buffer,102),64);
   if(DocumentProperties(IntPtr.Zero,printer,printerName,buffer,buffer,10)!=1)throw new Exception("Driver refused settings validation.");
   int paper=Marshal.ReadInt16(buffer,78),length=Marshal.ReadInt16(buffer,80),width=Marshal.ReadInt16(buffer,82);
   bool applied=paper!=1 && length==1500 && width==800;
   if(applied) {
    info=Marshal.AllocHGlobal(IntPtr.Size);Marshal.WriteIntPtr(info,buffer);
    if(!SetPrinter(printer,8,info,0)||!SetPrinter(printer,9,info,0))throw new Win32Exception(Marshal.GetLastWin32Error());
   }
   return new { form=formName,requested_width_mm=80,requested_height_mm=150,validated_paper_id=paper,validated_width_mm=width/10.0,validated_height_mm=length/10.0,applied=applied };
  } finally { if(info!=IntPtr.Zero)Marshal.FreeHGlobal(info);if(buffer!=IntPtr.Zero)Marshal.FreeHGlobal(buffer);ClosePrinter(printer); }
 }
}
'@
if ($Remove) { [VpsCustomPaper]::Remove($FormName) }
else { [VpsCustomPaper]::Probe($PrinterName,$FormName)|ConvertTo-Json|Set-Content -LiteralPath $Report -Encoding UTF8 }
