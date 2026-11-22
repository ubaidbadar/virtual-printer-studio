#!/usr/bin/env python3
"""Real Apple Silicon CUPS acceptance, with synthetic documents only."""
import hashlib,json,os,pathlib,shutil,socket,subprocess,sys,tempfile,time,uuid
root=pathlib.Path(__file__).resolve().parents[2]
evidence=root/'target/macos-evidence'
evidence.mkdir(parents=True,exist_ok=True)
app=root/'target/macos-package/Virtual Printer Studio.app/Contents/MacOS/virtual-printer-studio'
checks=[]
def run(*args,**kwargs):
    result=subprocess.run([str(a) for a in args],text=True,capture_output=True,**kwargs)
    if result.returncode:
        raise RuntimeError(f'{args[0]} failed: {result.stdout}\n{result.stderr}')
    return result.stdout.strip()
def passed(name):
    checks.append(name);print('PASS:',name,flush=True)
def wait_until(predicate,description,timeout=100):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        if predicate():return
        time.sleep(0.25)
    raise RuntimeError('Timed out: '+description)
run('/usr/bin/true')
assert run('/usr/bin/uname','-m')=='arm64'
work=pathlib.Path(tempfile.mkdtemp(prefix='vps-macos-'))
state=work/'state.json';output=work/'saved';output.mkdir()
id=str(uuid.uuid4());queue='VPS_'+id.replace('-','')
# lpr_queue is a stable SHA-256 prefix, not the CUPS queue name.
state.write_text(json.dumps({'welcomed':True,'notifications':False,'printers':[{'id':id,'name':'VPS Apple Silicon QA','folder':str(output),'pattern':'{title}_{counter}','enabled':True}]}))
service=None
installed=False
try:
    run(app,'--macos-printer-action','ensure','--state',state,'--printer',id)
    installed=True
    # The CUPS queue name is the profile identity, with its hyphens preserved.
    queue='VPS_'+id
    passed('native CUPS printer installation without Terminal')
    log=(evidence/'receiver.log').open('w')
    service=subprocess.Popen([str(app),'--serve','--state',str(state)],stdout=log,stderr=log)
    tag=hashlib.sha256(str(work.resolve()).encode()).hexdigest()[:24]
    endpoint=f'/private/tmp/vps-{os.getuid()}-{tag}/capture.sock'
    wait_until(lambda:pathlib.Path(endpoint).exists(),'local receiver')
    # The current user must not be accepted as a printer service.
    s=socket.socket(socket.AF_UNIX);s.settimeout(5);s.connect(endpoint)
    try:
        s.sendall(b'\x02unauthorized\n')
        assert s.recv(1)!=b'\x00'
    except (ConnectionResetError,BrokenPipeError):pass
    finally:s.close()
    passed('kernel peer credentials reject an ordinary user as print service')
    run(app,'--macos-printer-action','test','--state',state,'--printer',id)
    wait_until(lambda:len(list(output.glob('*.pdf')))>=1,'native queue test page')
    pdf=next(output.glob('*.pdf'))
    run('/usr/bin/swift',root/'scripts/qa/macos-native-print.swift','verify',pdf,'Virtual Printer Studio')
    passed('CUPS PDF filter to authenticated capture to valid saved PDF')
    token='AppleSiliconNativeReceipt'+uuid.uuid4().hex[:8]
    run('/usr/bin/swift',root/'scripts/qa/macos-native-print.swift','print',queue,token)
    wait_until(lambda:len(list(output.glob('*.pdf')))>=2,'native AppKit document')
    native=[p for p in output.glob('*.pdf') if p!=pdf][0]
    geometry=run('/usr/bin/swift',root/'scripts/qa/macos-native-print.swift','verify',native,token)
    (evidence/'native-receipt.txt').write_text(geometry+'\n')
    passed('actual AppKit print operation preserves printed content')
    run(app,'--macos-printer-action','pause','--state',state,'--printer',id)
    fixture=work/'fixture.pdf'
    run('/usr/bin/swift',root/'scripts/qa/macos-native-print.swift','fixture',fixture,'QueuedAppleSiliconReceipt')
    run('/usr/bin/lp','-d',queue,'-t','Paused Apple Silicon receipt',fixture)
    time.sleep(2)
    assert len(list(output.glob('*.pdf')))==2
    assert run('/usr/bin/lpstat','-W','not-completed','-o',queue)
    run(app,'--macos-printer-action','resume','--state',state,'--printer',id)
    wait_until(lambda:len(list(output.glob('*.pdf')))>=3,'resumed queued document')
    passed('paused printer retains queued documents and resumes delivery')
    wait_until(lambda:len(json.loads(state.read_text())['activity'])>=3,'durable activity update')
    service.terminate();service.wait(timeout=15);service=None
    # A stopped receiver must not turn an undelivered print into a successful job.
    run('/usr/bin/lp','-d',queue,'-t','Offline Apple Silicon receipt',fixture)
    time.sleep(3)
    assert len(list(output.glob('*.pdf')))==3
    assert run('/usr/bin/lpstat','-W','not-completed','-o',queue)
    passed('stopped receiver retains the unacknowledged CUPS job')
    service=subprocess.Popen([str(app),'--serve','--state',str(state)],stdout=log,stderr=log)
    wait_until(lambda:pathlib.Path(endpoint).exists(),'restarted receiver')
    # Expedite CUPS's retry delay without altering the document.
    run('/usr/bin/lp','-i',run('/usr/bin/lpstat','-W','not-completed','-o',queue).split()[0],'-H','resume')
    run(app,'--macos-printer-action','resume','--state',state,'--printer',id)
    wait_until(lambda:len(list(output.glob('*.pdf')))>=4,'offline queued job recovery',timeout=180)
    wait_until(lambda:len(json.loads(state.read_text())['activity'])>=4,'recovered activity update')
    passed('receiver restart recovers the queued document')
    time.sleep(2)
    assert len(list(output.glob('*.pdf')))==4
    assert all(p.read_bytes().startswith(b'%PDF-') for p in output.glob('*.pdf'))
    passed('no duplicate output after restart and all outputs are PDFs')
    report={'architecture':'arm64','os':run('/usr/bin/sw_vers'),'checks':checks,'pdf_count':4,'appkit_geometry':geometry}
    (evidence/'results.json').write_text(json.dumps(report,indent=2))
finally:
    if service is not None:service.terminate();service.wait(timeout=15)
    if installed:
        subprocess.run(['/usr/bin/cancel','-a',queue],capture_output=True)
        result=subprocess.run([str(app),'--macos-printer-action','remove','--state',str(state),'--printer',id],capture_output=True,text=True)
        (evidence/'cleanup.txt').write_text(result.stdout+result.stderr)
    (evidence/'completed-checks.json').write_text(json.dumps(checks,indent=2))
    subprocess.run(['/usr/bin/lpstat','-t'],stdout=(evidence/'cups-state.txt').open('w'),stderr=subprocess.STDOUT)
    # Keep failures diagnosable without exporting account data or real print jobs.
    shutil.rmtree(work,ignore_errors=True)
