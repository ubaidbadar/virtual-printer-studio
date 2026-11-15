"""Independently parse real Windows driver output and verify printed page contents."""
import argparse
import hashlib
import json
from pathlib import Path
from pypdf import PdfReader

p = argparse.ArgumentParser()
p.add_argument('root', type=Path)
p.add_argument('--expected-count', type=int, default=6)
p.add_argument('--marker', default='Virtual Printer Studio integration test')
a = p.parse_args()
files = sorted(a.root.rglob('*.pdf'))
assert len(files) == a.expected_count, f'Expected {a.expected_count} PDFs, found {len(files)}'
results = []
for path in files:
    reader = PdfReader(path, strict=True)
    expected_pages = 3 if path.name.startswith('Multipage report') else 1
    assert len(reader.pages) == expected_pages, f'{path}: wrong page count'
    for printed_page in reader.pages:
        assert a.marker in printed_page.extract_text(), f'{path}: missing page text'
    page = reader.pages[0]
    text = page.extract_text()
    if path.name.startswith('Unicode '):
        assert 'Unicode café Ω invoice' in text, f'{path}: Unicode page text was not preserved'
    assert a.marker in text, f'{path}: missing printed text: {text!r}'
    assert float(page.mediabox.width) > 0 and float(page.mediabox.height) > 0
    results.append(dict(file=str(path.relative_to(a.root)), pages=len(reader.pages),
                        text=text.strip(), width=float(page.mediabox.width), height=float(page.mediabox.height),
                        sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
(a.root/'pdf-verification.json').write_text(json.dumps(results, indent=2))
print(f'PASS: {len(results)} PDFs independently parsed; page dimensions, page count and printed text verified.')
