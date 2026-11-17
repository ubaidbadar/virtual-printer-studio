"""Synthetic Edge-to-Windows queue test. This does not authenticate to Restora."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys

from playwright.sync_api import sync_playwright

spec = importlib.util.spec_from_file_location('restora_print', Path(__file__).with_name('restora-print.py'))
printing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(printing)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('work', 'report', 'state', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--printer', required=True)
    args = parser.parse_args()
    report = {'mode': 'synthetic_windows_browser_queue', 'pass': False, 'stage': 'setup', 'checks': []}
    try:
        if sys.platform != 'win32':
            raise printing.CheckFailed('Native mode requires Windows')
        with sync_playwright() as playwright:
            context = printing.launch_context(playwright, args, True)
            try:
                page = context.new_page()
                # A real document navigation runs the context's observer.
                page.goto('data:text/html,<button>Print fixture</button>')
                for index, kind in enumerate(('receipt', 'kitchen', 'receipt'), 1):
                    report['stage'] = f'print_{index}'
                    html = ('<!doctype html><html><head><title>Synthetic printer test</title>'
                            '<style>@page{size:80mm 150mm;margin:0}body{width:72mm;'
                            'margin:4mm;font:12px Arial}</style></head><body>'
                            f'<h1>{kind.upper()}</h1><p>Fixture item</p><p>Quantity: 2</p>'
                            '<p>Total: 42</p><p>Printer pipeline fixture</p></body></html>')
                    page.evaluate('''html => {
                      document.querySelector('button').onclick = () => {
                        const frame=document.createElement('iframe');
                        document.body.appendChild(frame);
                        frame.contentDocument.open();frame.contentDocument.write(html);
                        frame.contentDocument.close();
                        setTimeout(() => frame.contentWindow.print(), 200);
                      };
                    }''', html)
                    previous = set(args.output.glob('*.pdf'))
                    page.get_by_role('button', name='Print fixture').click()
                    page.wait_for_function('(n)=>window.__vpsPrints.length===n', arg=index)
                    source = page.evaluate('window.__vpsPrints.at(-1)')
                    pdf = printing.wait_pdf(args.output, args.state, index, previous)
                    report['checks'].append(printing.inspect_pdf(pdf, source['text'],
                        {'items': [{'name': 'Fixture item'}], 'total': 42}, kind, True))
                page.wait_for_timeout(3000)
                state = json.loads(args.state.read_text(encoding='utf-8-sig'))
                if len(list(args.output.glob('*.pdf'))) != 3 or state['printers'][0]['count'] != 3:
                    raise printing.CheckFailed('Unexpected duplicate delivery')
                report['stage'] = 'complete'
                report['pass'] = all(check['pass'] for check in report['checks'])
            finally:
                context.close()
    except printing.CheckFailed as error:
        report['failure'] = str(error)
    except Exception:
        report['failure'] = 'Test failed at the recorded stage; private diagnostics suppressed'
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps(report))
    return 0 if report['pass'] else 1


if __name__ == '__main__':
    sys.exit(main())
