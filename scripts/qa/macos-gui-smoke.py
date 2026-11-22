#!/usr/bin/env python3
"""Launch the real desktop composition, including the native menu bar."""
import json,os,pathlib,subprocess,tempfile,time
root=pathlib.Path(__file__).resolve().parents[2]
app=root/'target/macos-package/Virtual Printer Studio.app/Contents/MacOS/virtual-printer-studio'
evidence=root/'target/macos-gui-evidence';evidence.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='vps-mac-gui-') as home:
    pathlib.Path(home,'Documents').mkdir()
    environment=os.environ.copy();environment['HOME']=home
    with (evidence/'desktop.log').open('w') as log:
        process=subprocess.Popen([str(app)],env=environment,stdout=log,stderr=log)
        try:
            time.sleep(5)
            assert process.poll() is None,'Desktop app exited during normal startup'
            probe=subprocess.run(['/usr/bin/swift',str(root/'scripts/qa/macos-gui-window.swift'),str(process.pid)],text=True,capture_output=True,timeout=45)
            assert probe.returncode==0,probe.stdout+probe.stderr
            result=json.loads(probe.stdout);result['architecture']=subprocess.check_output(['/usr/bin/uname','-m'],text=True).strip()
            result['os']=subprocess.check_output(['/usr/bin/sw_vers'],text=True).strip()
            result['launch']='normal app startup with isolated home; includes native menu bar and capture worker'
            (evidence/'results.json').write_text(json.dumps(result,indent=2))
            print('PASS: normal desktop app remains running with a visible main window')
        finally:
            if process.poll() is None:process.terminate();process.wait(timeout=15)
