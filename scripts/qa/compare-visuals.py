"""Measure supplied references against actual rendered client pixels, without resizing."""
import argparse
import json
from pathlib import Path
from PIL import Image, ImageChops

parser = argparse.ArgumentParser()
parser.add_argument('--actual', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--require-pixel-perfect', action='store_true')
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
reference = Path(__file__).resolve().parents[2] / 'tests/visual/reference'
results = []
for path in sorted(reference.glob('*.png')):
    actual = args.actual / path.name
    if not actual.exists():
        results.append(dict(scene=path.stem, status='not captured'))
        continue
    expected = Image.open(path).convert('RGB')
    expected = expected.crop((0, 40, expected.width, expected.height))
    observed = Image.open(actual).convert('RGB')
    row = dict(scene=path.stem, expected_size=expected.size, actual_size=observed.size)
    if expected.size != observed.size:
        row['status'] = 'size mismatch'
    else:
        diff = ImageChops.difference(expected, observed)
        channels = diff.split()
        maximum = ImageChops.lighter(ImageChops.lighter(channels[0], channels[1]), channels[2])
        histogram = maximum.histogram()
        total = expected.width * expected.height
        row.update(status='pixel perfect' if histogram[0] == total else 'different',
                   changed_percent=round(100 * (total-histogram[0])/total, 3),
                   delta_over_8_percent=round(100*sum(histogram[9:])/total, 3),
                   mean_absolute_channel_error=round(sum(i*n for channel in channels for i,n in enumerate(channel.histogram()))/(3*total), 3))
        diff.save(args.output / f'{path.stem}-difference.png')
    results.append(row)
(args.output/'results.json').write_text(json.dumps(results, indent=2))
lines = ['Visual comparison: reference title bar cropped by 40 pixels; no resizing or alignment.', '', '| Scene | Result | Changed pixels |', '|---|---|---|']
for row in results:
    lines.append(f"| {row['scene']} | {row['status']} | {row.get('changed_percent', 'N/A')} |")
(args.output/'report.md').write_text('\n'.join(lines)+'\n')
print('\n'.join(lines))
print('PIXEL-PERFECT: ' + ('YES' if all(r['status'] == 'pixel perfect' for r in results) else 'NO'))
if any(r['status'] in ('not captured', 'size mismatch') for r in results[:7]):
    raise SystemExit('Required application screenshot missing or incorrectly sized')
if args.require_pixel_perfect and any(row['status'] != 'pixel perfect' for row in results):
    raise SystemExit(1)
