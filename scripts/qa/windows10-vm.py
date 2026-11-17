"""Create unattended evaluation VM input and collect evidence. Runs on disposable CI."""
import argparse
import http.server
import json
import secrets
import socket
from PIL import Image
import shutil
import subprocess
import threading
import time
from pathlib import Path
from xml.sax.saxutils import escape

p = argparse.ArgumentParser()
p.add_argument('--work', type=Path, required=True)
a = p.parse_args()
w = a.work.resolve()
w.mkdir(parents=True, exist_ok=True)
payload = w/'payload'
payload.mkdir(exist_ok=True)
shutil.copytree('scripts', payload/'scripts', dirs_exist_ok=True)
shutil.copytree('src/platform', payload/'src/platform', dirs_exist_ok=True)
shutil.copytree('target/qa-package', payload/'app', dirs_exist_ok=True)
shutil.copytree(w/'pwsh', payload/'pwsh', dirs_exist_ok=True)
(payload/'commit.txt').write_text(subprocess.check_output(['git','rev-parse','HEAD'], text=True).strip())
bootstrap = r'''
$ErrorActionPreference='Stop'
$source=$PSScriptRoot
Copy-Item -LiteralPath $source -Destination C:\vps -Recurse -Force
$env:GITHUB_SHA=Get-Content C:\vps\commit.txt
$evidence='C:\vps-evidence'
New-Item -ItemType Directory -Path $evidence -Force|Out-Null
$code=0
try {
    & C:\vps\pwsh\pwsh.exe -NoProfile -ExecutionPolicy Bypass -File C:\vps\scripts\qa\record-platform.ps1 -OutputDirectory $evidence -ExpectedMachine windows-10-x64-vm
    if($LASTEXITCODE -ne 0){throw 'Platform check failed'}
    & C:\vps\pwsh\pwsh.exe -NoProfile -ExecutionPolicy Bypass -File C:\vps\scripts\windows-smoke.ps1 -App C:\vps\app\virtual-printer-studio.exe -TestRoot "$evidence\printing" -Extended *> "$evidence\printing.log"
    if($LASTEXITCODE -ne 0){throw 'Real printer test failed'}
    . C:\vps\scripts\qa\process-exit.ps1
    $env:SLINT_BACKEND='winit-software'
    $env:SLINT_SCALE_FACTOR='1'
    $process=Start-Process -FilePath C:\vps\app\virtual-printer-studio.exe -ArgumentList @('--ui-snapshot',"$evidence\visuals") -PassThru -RedirectStandardError "$evidence\visuals.log"
    $nativeHandle=$process.Handle
    $snapshotExitCode=Wait-VpsProcess $process $nativeHandle 120000
    if($snapshotExitCode -ne 0){throw 'UI snapshot failed'}
    $probe=Start-Process powershell.exe -ArgumentList @('-MTA','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File','C:\vps\scripts\qa\ui-automation.ps1','-App','C:\vps\app\virtual-printer-studio.exe','-OutputDirectory',"$evidence\ui-automation") -PassThru -RedirectStandardOutput "$evidence\ui-automation.log" -RedirectStandardError "$evidence\ui-automation-error.log"
    $probeHandle=$probe.Handle
    if((Wait-VpsProcess $probe $probeHandle 180000) -ne 0){throw 'Native UI Automation failed'}
    $env:VPS_DISPOSABLE_TEST_VM='1'
    $live=Start-Process powershell.exe -ArgumentList @('-MTA','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File','C:\vps\scripts\qa\live-ui.ps1','-App','C:\vps\app\virtual-printer-studio.exe','-OutputDirectory',"$evidence\live-ui") -PassThru -RedirectStandardOutput "$evidence\live-ui.log" -RedirectStandardError "$evidence\live-ui-error.log"
    $liveHandle=$live.Handle
    if((Wait-VpsProcess $live $liveHandle 240000) -ne 0){throw 'Live GUI acceptance failed'}
} catch {
    $_|Out-String|Set-Content "$evidence\failure.txt"
    $code=1
}
@{exit_code=$code;environment='Windows 10 Enterprise LTSC evaluation, QEMU x64 VM';utc=[DateTime]::UtcNow.ToString('o')}|ConvertTo-Json|Set-Content "$evidence\result.json"
Get-Process -Name virtual-printer-studio -ErrorAction SilentlyContinue|Stop-Process -Force -ErrorAction SilentlyContinue
Add-Type -AssemblyName System.IO.Compression
$file=[IO.File]::Open('C:\evidence.zip',[IO.FileMode]::Create,[IO.FileAccess]::Write,[IO.FileShare]::Read)
$zip=New-Object IO.Compression.ZipArchive($file,[IO.Compression.ZipArchiveMode]::Create,$false)
try {
 foreach($item in Get-ChildItem -LiteralPath $evidence -Recurse -File){
  $inputFile=[IO.File]::Open($item.FullName,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
  $outputFile=$zip.CreateEntry($item.FullName.Substring($evidence.Length+1).Replace('\','/')).Open()
  try{$inputFile.CopyTo($outputFile)}finally{$outputFile.Dispose();$inputFile.Dispose()}
 }
}finally{$zip.Dispose();$file.Dispose()}
Invoke-WebRequest -Uri http://10.0.2.2:18080/evidence -Method Post -InFile C:\evidence.zip -UseBasicParsing -TimeoutSec 30|Out-Null
Stop-Computer -Force
'''
(payload/'bootstrap.ps1').write_text(bootstrap)
password=secrets.token_hex(16)+'Aa9!'
command="powershell.exe -NoProfile -ExecutionPolicy Bypass -Command \"Get-PSDrive -PSProvider FileSystem | ForEach-Object { $p=Join-Path $_.Root 'bootstrap.ps1'; if(Test-Path $p){ &amp; $p } }\""
answer=fr'''<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State">
<settings pass="windowsPE"><component name="Microsoft-Windows-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
<DiskConfiguration><Disk wcm:action="add"><DiskID>0</DiskID><WillWipeDisk>true</WillWipeDisk><CreatePartitions><CreatePartition wcm:action="add"><Order>1</Order><Type>Primary</Type><Size>100</Size></CreatePartition><CreatePartition wcm:action="add"><Order>2</Order><Type>Primary</Type><Extend>true</Extend></CreatePartition></CreatePartitions><ModifyPartitions><ModifyPartition wcm:action="add"><Order>1</Order><PartitionID>1</PartitionID><Active>true</Active><Format>NTFS</Format><Label>System</Label></ModifyPartition><ModifyPartition wcm:action="add"><Order>2</Order><PartitionID>2</PartitionID><Format>NTFS</Format><Letter>C</Letter></ModifyPartition></ModifyPartitions></Disk></DiskConfiguration>
<ImageInstall><OSImage><InstallFrom><MetaData wcm:action="add"><Key>/IMAGE/INDEX</Key><Value>1</Value></MetaData></InstallFrom><InstallTo><DiskID>0</DiskID><PartitionID>2</PartitionID></InstallTo></OSImage></ImageInstall><UserData><AcceptEula>true</AcceptEula><FullName>VPS QA</FullName><Organization>VPS test</Organization></UserData></component>
<component name="Microsoft-Windows-International-Core-WinPE" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS"><SetupUILanguage><UILanguage>en-US</UILanguage></SetupUILanguage><InputLocale>en-US</InputLocale><SystemLocale>en-US</SystemLocale><UILanguage>en-US</UILanguage><UserLocale>en-US</UserLocale></component></settings>
<settings pass="specialize"><component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS"><ComputerName>VPS-WIN10</ComputerName><TimeZone>UTC</TimeZone></component></settings>
<settings pass="oobeSystem"><component name="Microsoft-Windows-International-Core" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS"><InputLocale>0409:00000409</InputLocale><SystemLocale>en-US</SystemLocale><UILanguage>en-US</UILanguage><UserLocale>en-US</UserLocale></component><component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS"><OOBE><SkipMachineOOBE>true</SkipMachineOOBE><SkipUserOOBE>true</SkipUserOOBE><HideEULAPage>true</HideEULAPage><HideOnlineAccountScreens>true</HideOnlineAccountScreens><HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE><NetworkLocation>Work</NetworkLocation><ProtectYourPC>3</ProtectYourPC></OOBE><UserAccounts><LocalAccounts><LocalAccount wcm:action="add"><Name>VpsTest</Name><Group>Administrators</Group><Password><Value>{password}</Value><PlainText>true</PlainText></Password></LocalAccount></LocalAccounts></UserAccounts><AutoLogon><Username>VpsTest</Username><Enabled>true</Enabled><LogonCount>1</LogonCount><Password><Value>{password}</Value><PlainText>true</PlainText></Password></AutoLogon><FirstLogonCommands><SynchronousCommand wcm:action="add"><Order>1</Order><Description>Run acceptance</Description><CommandLine>{command.replace(chr(34),'&quot;')}</CommandLine></SynchronousCommand></FirstLogonCommands></component></settings></unattend>'''
(payload/'Autounattend.xml').write_text(answer)
subprocess.run(['genisoimage','-quiet','-J','-r','-V','VPSQA','-o',str(w/'payload.iso'),str(payload)], check=True)
subprocess.run(['qemu-img','create','-f','qcow2',str(w/'disk.qcow2'),'64G'],check=True)
class Receiver(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        size=int(self.headers.get('Content-Length','0'))
        if self.path != '/evidence' or not 0<size<200_000_000:
            self.send_error(400); return
        with (w/'evidence.zip.tmp').open('wb') as out:
            remaining=size
            while remaining:
                block=self.rfile.read(min(1024*1024,remaining))
                if not block: raise ConnectionError('Incomplete upload')
                out.write(block);remaining-=len(block)
        (w/'evidence.zip.tmp').rename(w/'evidence.zip')
        self.send_response(200);self.end_headers()
server=http.server.HTTPServer(('127.0.0.1',18080),Receiver)
threading.Thread(target=server.serve_forever,daemon=True).start()
kvm=Path('/dev/kvm').exists()
metadata={'hypervisor':'QEMU','acceleration':'KVM' if kvm else 'TCG','official_image':'Windows 10 Enterprise LTSC evaluation 19044.1288','iso_sha256':subprocess.check_output(['sha256sum',str(w/'windows.iso')],text=True).split()[0]}
(w/'vm-platform.json').write_text(json.dumps(metadata,indent=2))
print(json.dumps(metadata),flush=True)
args=['qemu-system-x86_64','-machine','q35,accel='+('kvm' if kvm else 'tcg'),'-cpu','host' if kvm else 'max','-m','6G','-smp','4','-drive',f'file={w}/disk.qcow2,format=qcow2,if=ide','-drive',f'file={w}/windows.iso,media=cdrom,readonly=on','-drive',f'file={w}/payload.iso,media=cdrom,readonly=on','-boot','order=c,once=d','-device','e1000,netdev=net0','-netdev','user,id=net0','-display','none','-vnc','127.0.0.1:1','-monitor',f'unix:{w}/monitor.sock,server,nowait','-no-reboot']
# Allow Windows setup restarts; disk becomes the default boot device after initial ISO boot.
args.remove('-no-reboot')
proc=subprocess.Popen(args,stdout=(w/'qemu.log').open('w'),stderr=subprocess.STDOUT)
try:
    for minute in range(25):
        if (w/'evidence.zip').exists():break
        if proc.poll() is not None:raise RuntimeError('VM exited before sending evidence')
        try:
            with socket.socket(socket.AF_UNIX) as monitor:
                monitor.connect(str(w/'monitor.sock'))
                monitor.recv(4096)
                monitor.sendall(f'screendump {w}/screen.ppm\n'.encode())
                time.sleep(1)
                Image.open(w/'screen.ppm').save(w/'screen.png')
        except (OSError, ValueError):
            pass
        print(f'VM acceptance minute {minute+1}; waiting for Windows guest evidence',flush=True)
        time.sleep(60)
    else:raise TimeoutError('Windows VM did not finish within 25 minutes')
finally:
    proc.terminate()
    proc.wait(timeout=20)
    server.shutdown()
