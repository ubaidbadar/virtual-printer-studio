"""Check snapshot dimensions and nonempty rendering at explicit scale factors."""
import argparse
import json
from PIL import Image
from pathlib import Path
p=argparse.ArgumentParser()
p.add_argument('root',type=Path)
a=p.parse_args()
heights=[760,760,960,760,1200,760,760,800,760]
results=[]
for scale in (1.0,1.25,1.5,2.0):
    folder=a.root/('visuals' if scale==1 else f'visuals-scale-{scale}')
    metadata=json.loads((folder/'capture.json').read_text())
    assert metadata['scale_factor']==scale, f'Wrong scale: {metadata}'
    files=sorted(folder.glob('*.png'))
    assert len(files)==9, f'{folder}: incomplete scenes'
    for file,height in zip(files,heights):
        with Image.open(file) as image:
            expected=(int(1280*scale),int(height*scale))
            assert image.size==expected, f'{file}: {image.size}, expected {expected}'
            assert any(low!=high for low,high in image.convert('RGB').getextrema()), f'{file}: blank image'
            results.append(dict(scene=file.stem,scale=scale,width=image.width,height=image.height))
(a.root/'dpi-verification.json').write_text(json.dumps(results,indent=2))
print(f'PASS: {len(results)} rendered scenes have expected dimensions at 100%, 125%, 150% and 200% scale.')
