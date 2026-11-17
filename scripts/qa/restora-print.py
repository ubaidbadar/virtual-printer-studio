"""Operate existing Restora orders. Never log or export account/order contents.

Native mode must run on the Windows machine hosting the temporary VPS queue.
Reference mode renders the site's HTML in Chromium; it does NOT test a printer.
Credentials come from environment variables, never command line arguments.
"""
import argparse
import json
import os
from pathlib import Path
import re
import sys
import subprocess
import time
import unicodedata

from playwright.sync_api import sync_playwright
from pypdf import PdfReader
from pypdf.errors import PdfReadError


class CheckFailed(Exception):
    """Only static, non-sensitive messages may be used here."""


def compact(value):
    return ''.join(c for c in unicodedata.normalize('NFKC', value).casefold()
                   if c.isalnum())


def inspect_pdf(path, source, order, kind, native):
    reader = PdfReader(path)
    texts = [page.extract_text() or '' for page in reader.pages]
    method = 'pdf_text'
    lines = [compact(line) for line in source.splitlines() if compact(line)]
    if native and not all(line in compact('\n'.join(texts)) for line in lines):
        # Edge can submit vector outlines. Read their visual text using the local
        # Windows OCR API; account contents never leave this machine.
        import fitz
        document = fitz.open(path)
        texts = []
        try:
            for index, page in enumerate(document):
                image = path.with_suffix(f'.ocr-{index}.png')
                result = path.with_suffix(f'.ocr-{index}.txt')
                try:
                    page.get_pixmap(matrix=fitz.Matrix(3, 3)).save(image)
                    process = subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive',
                        '-ExecutionPolicy', 'Bypass',
                        '-File', str(Path(__file__).with_name('windows-ocr.ps1')),
                        '-Image', str(image.resolve()), '-TextOutput', str(result.resolve())],
                        capture_output=True, timeout=60)
                    if process.returncode != 0 or not result.exists():
                        raise CheckFailed('Local Windows OCR is unavailable')
                    texts.append(result.read_text(encoding='utf-8-sig'))
                finally:
                    image.unlink(missing_ok=True)
                    result.unlink(missing_ok=True)
        finally:
            document.close()
        method = 'windows_local_ocr'
    text = compact('\n'.join(texts))
    # Check against both the independent order response and every printed line.
    items = order.get('items', [])
    item_checks = [bool(compact(item.get('name', ''))) and
                   compact(item['name']) in text for item in items]
    line_checks = [compact(line) in text for line in source.splitlines() if compact(line)]
    total_ok = kind != 'receipt' or compact(str(order.get('total'))) in text
    producer_ok = 'microsoft' in str((reader.metadata or {}).get('/Producer', '')).lower()
    sizes = [{'width_mm': round(float(p.mediabox.width) * 25.4 / 72, 2),
              'height_mm': round(float(p.mediabox.height) * 25.4 / 72, 2)}
             for p in reader.pages]
    width_ok = bool(sizes) and all(abs(s['width_mm'] - 80) < 1 for s in sizes)
    # A page containing only the site's staff/printed-time footer is undesirable,
    # even when its text survived. Do not reject legitimate long-order pagination.
    last = compact(texts[-1]) if texts else ''
    footer_only = (len(texts) > 1 and len(last) < 160 and
                   ('printed' in last or 'staff' in last) and
                   not any(compact(i['name']) in last for i in items))
    content_ok = bool(item_checks) and all(item_checks) and all(line_checks) and total_ok
    return {'kind': kind, 'pages': len(texts), 'page_sizes': sizes, 'content_method': method,
            'items_checked': len(item_checks), 'content_ok': content_ok,
            'microsoft_driver': producer_ok if native else None,
            'width_80mm': width_ok, 'footer_only_page': footer_only,
            'pass': content_ok and width_ok and not footer_only and (producer_ok or not native)}


PRINT_OBSERVER = r"""
window.__vpsPrints = [];
new MutationObserver(records => {
  for (const record of records) for (const node of record.addedNodes) {
    if (node.tagName === 'IFRAME' && node.contentWindow) {
      const frame = node.contentWindow, original = frame.print;
      frame.print = function() {
        const doctype = frame.document.doctype;
        const prefix = doctype ? new XMLSerializer().serializeToString(doctype) : '';
        window.__vpsPrints.push({html: prefix + frame.document.documentElement.outerHTML,
                                text: frame.document.body.innerText,
                                mode: frame.document.compatMode});
        return original.call(frame);
      };
    }
  }
}).observe(document, {childList: true, subtree: true});
"""


def wait_pdf(folder, state_path, count, previous):
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        try:
            state = json.loads(state_path.read_text(encoding='utf-8-sig'))
            delivered = state['printers'][0]['count']
            files = set(folder.glob('*.pdf'))
            new = files - previous
            if delivered == count and len(files) == count and len(new) == 1:
                candidate = new.pop()
                # A stable, parseable file and persisted delivery count are required.
                PdfReader(candidate)
                return candidate
            if delivered > count or len(new) > 1:
                raise CheckFailed('Unexpected duplicate delivery')
        except (OSError, ValueError, KeyError, PdfReadError):
            pass
        time.sleep(0.25)
    raise CheckFailed('The Windows queue did not deliver the expected PDF')


def launch_context(playwright, args, native):
    args.work.mkdir(parents=True, exist_ok=False)
    profile = args.work / 'profile'
    profile.mkdir()
    if native:
        # Edge's kiosk-printing mode submits through its real Windows print path.
        # No headless PDF export is used in native mode.
        prefs = {'printing': {'print_preview_sticky_settings': {'appState': json.dumps({
            'version': 2, 'recentDestinations': [{'id': args.printer, 'origin': 'local'}],
            'selectedDestinationId': args.printer, 'isHeaderFooterEnabled': False})}}}
        (profile / 'Default').mkdir()
        (profile / 'Default' / 'Preferences').write_text(json.dumps(prefs), encoding='utf-8')
    options = {'headless': not native,
               'args': ['--kiosk-printing', '--no-first-run'] if native else ['--no-sandbox']}
    if native:
        options['channel'] = 'msedge'
    elif args.chromium:
        options['executable_path'] = args.chromium
    context = playwright.chromium.launch_persistent_context(
        profile, timezone_id='Asia/Karachi', **options)
    context.add_init_script(PRINT_OBSERVER)
    return context


def run(args, report):
    native = not args.reference_only
    if native and sys.platform != 'win32':
        raise CheckFailed('Native mode requires Windows')
    if native and not (args.printer and args.state and args.output):
        raise CheckFailed('Native mode requires a printer, capture state and output folder')
    email, password = os.environ.get('RESTORA_TEST_EMAIL'), os.environ.get('RESTORA_TEST_PASSWORD')
    if not email or not password:
        raise CheckFailed('Missing protected test credentials')
    with sync_playwright() as playwright:
        context = launch_context(playwright, args, native)
        try:
            page = context.new_page()
            orders, statuses = [], []

            def observe(response):
                if response.request.resource_type not in ('xhr', 'fetch'):
                    return
                if '/print-jobs' in response.url:
                    statuses.append(response.status)
                if re.search(r'/orders(?:\?|$)', response.url):
                    try:
                        data = response.json()
                        if isinstance(data, list):
                            orders.extend(o for o in data if isinstance(o, dict) and 'items' in o)
                    except Exception:
                        pass

            page.on('response', observe)
            report['stage'] = 'sign_in'
            page.goto('https://app.restora.live/login', wait_until='domcontentloaded')
            page.locator('input[name="email"]').fill(email)
            page.locator('input[name="password"]').fill(password)
            page.get_by_role('button', name='Sign in', exact=True).click()
            page.get_by_role('link', name='Orders', exact=True).wait_for(timeout=45000)
            page.get_by_role('link', name='Orders', exact=True).click()
            report['stage'] = 'existing_order'
            button = page.get_by_role('button', name=re.compile(r'^More for order \d+$')).first
            button.wait_for(timeout=45000)
            label = button.get_attribute('aria-label')
            token = int(re.search(r'\d+$', label).group())
            matching = {o['_id']: o for o in orders if o.get('dailyToken') == token}
            if len(matching) != 1:
                raise CheckFailed('Could not identify an unambiguous existing order')
            order = next(iter(matching.values()))
            if not order.get('items'):
                raise CheckFailed('Existing order has no verifiable items')
            for index, kind in enumerate(('receipt', 'kitchen', 'receipt'), 1):
                report['stage'] = f'print_{index}'
                previous = set(args.output.glob('*.pdf')) if native else set()
                page.get_by_role('button', name=label, exact=True).click()
                page.get_by_role('menuitem', name='Print receipt' if kind == 'receipt'
                                 else 'Print kitchen slip', exact=True).click()
                page.wait_for_function('(n) => window.__vpsPrints.length === n', arg=index,
                                       timeout=30000)
                source = page.evaluate('window.__vpsPrints.at(-1)')
                if native:
                    pdf = wait_pdf(args.output, args.state, index, previous)
                else:
                    render = context.new_page()
                    render.set_content(source['html'], wait_until='networkidle')
                    if render.evaluate('document.compatMode') != source['mode']:
                        raise CheckFailed('Reference render changed the source document mode')
                    pdf = args.work / f'reference-{index}.pdf'
                    render.pdf(path=str(pdf), print_background=True, prefer_css_page_size=True)
                    render.close()
                check = inspect_pdf(pdf, source['text'], order, kind, native)
                check['source_mode'] = source['mode']
                report['checks'].append(check)
            # Detect delayed duplicates before declaring success.
            if native:
                page.wait_for_timeout(3000)
                state = json.loads(args.state.read_text(encoding='utf-8-sig'))
                if (len(list(args.output.glob('*.pdf'))) != 3 or
                        state['printers'][0]['count'] != 3):
                    raise CheckFailed('Unexpected duplicate delivery')
            report['print_api_statuses'] = statuses
            report['stage'] = 'complete'
            report['pass'] = all(check['pass'] for check in report['checks'])
        finally:
            context.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--printer')
    parser.add_argument('--state', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--chromium')
    args = parser.parse_args()
    report = {'mode': 'chromium_reference' if args.reference_only else 'windows_native_queue',
              'stage': 'setup', 'pass': False, 'checks': []}
    try:
        run(args, report)
    except CheckFailed as error:
        report['failure'] = str(error)
    except Exception:
        # Playwright errors may contain page text, credentials or request bodies.
        report['failure'] = 'Test failed at the recorded stage; private diagnostics suppressed'
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps(report))
    return 0 if report['pass'] else 1


if __name__ == '__main__':
    sys.exit(main())
