"""Windows transport acceptance probe using only the inbox PDF/TCP-IP drivers."""
import socket, threading, subprocess, pathlib, uuid, time, json
root=pathlib.Path('target/lpr-probe').resolve();root.mkdir(parents=True,exist_ok=True)
name='VPS LPR '+str(uuid.uuid4())[:8]; queue='VPS_'+uuid.uuid4().hex[:24];jobs=[];errors=[]
listener=socket.socket();listener.bind(('127.0.0.1',15151));listener.listen(8);listener.settimeout(90)
def read_line(stream):
 line=stream.readline(4096)
 if not line.endswith(b'\n'): raise ValueError('missing command line')
 return line
def receive():
 try:
  while not jobs:
   client,_=listener.accept();client.settimeout(45)
   with client,client.makefile('rb') as stream:
    first=read_line(stream);print('LPR command',repr(first),flush=True)
    if first[:1]!=b'\x02':client.sendall(b'\x01');continue
    client.sendall(b'\0'); parts={}
    while len(parts)<2:
     line=read_line(stream);code=line[0];size=int(line[1:].split(b' ',1)[0]);print('LPR file',code,size,flush=True);client.sendall(b'\0')
     payload=stream.read(size)
     if len(payload)!=size or stream.read(1)!=b'\0':raise ValueError('incomplete data')
     parts[code]=payload;client.sendall(b'\0')
    jobs.append(parts)
 except Exception as e:errors.append(str(e));print('server error',repr(e),flush=True)
thread=threading.Thread(target=receive,daemon=True);thread.start()
script=r'''$ErrorActionPreference='Stop'
Start-Service Spooler
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public class ProbePort {
 [DllImport("winspool.drv",EntryPoint="DeletePortW",CharSet=CharSet.Unicode,SetLastError=true)]private static extern bool DeletePort(string server,IntPtr window,string name);
 public static bool Remove(string name){return DeletePort(null,IntPtr.Zero,name);}
}
'@
$port='__QUEUE__';$name='__NAME__'
try {
 New-CimInstance -ClassName Win32_TCPIPPrinterPort -Property @{Name=$port;Protocol=[uint32]2;HostAddress='127.0.0.1';PortNumber=[uint32]15151;Queue=$port;ByteCount=$true;SNMPEnabled=$false} | Out-Null
 Add-Printer -Name $name -DriverName 'Microsoft Print To PDF' -PortName $port
 Write-Output 'Installed PDF printer with built-in LPR port.'
 Add-Type -AssemblyName System.Drawing
 $doc=New-Object Drawing.Printing.PrintDocument
 $doc.PrinterSettings.PrinterName=$name
 $doc.PrintController=New-Object Drawing.Printing.StandardPrintController
 $doc.DocumentName='VPS LPR acceptance test'
 $doc.add_PrintPage({param($s,$e) $font=New-Object Drawing.Font('Arial',16);try{$e.Graphics.DrawString('Virtual Printer Studio LPR transport',$font,[Drawing.Brushes]::Black,72,72)}finally{$font.Dispose()};$e.HasMorePages=$false})
 try{$doc.Print()}finally{$doc.Dispose()}
 $deadline=(Get-Date).AddSeconds(60)
 while(@(Get-PrintJob -PrinterName $name).Count -gt 0 -and (Get-Date) -lt $deadline){Start-Sleep -Milliseconds 300}
}finally{
 Get-PrintJob -PrinterName $name -ErrorAction SilentlyContinue | Remove-PrintJob -ErrorAction SilentlyContinue
 if(Get-Printer -Name $name -ErrorAction SilentlyContinue){Remove-Printer -Name $name}
 for($i=0;$i -lt 20;$i++){if([ProbePort]::Remove($port)){Write-Output 'Removed LPR port.';break};Start-Sleep -Milliseconds 200}
 if($i -eq 20){throw 'Could not remove the LPR port.'}
}'''.replace('__QUEUE__',queue).replace('__NAME__',name)
file=root/'print.ps1';file.write_text(script,encoding='utf-8')
result=subprocess.run(['powershell.exe','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',str(file)],timeout=110)
thread.join(timeout=2);listener.close()
if not jobs:raise SystemExit(f'LPR probe failed: exit={result.returncode} errors={errors}')
control=jobs[0].get(2,b'');data=jobs[0].get(3,b'')
(root/'control.txt').write_bytes(control);(root/'document.pdf').write_bytes(data)
print('CONTROL',control.decode(errors='replace'),flush=True)
if not data.startswith(b'%PDF-'):raise SystemExit(f'Not PDF: {data[:100]!r}')
print('PASS: Microsoft Print to PDF captured over built-in LPR.',len(data),'bytes',flush=True)

if result.returncode:raise SystemExit(f'Cleanup failed: exit={result.returncode}')
