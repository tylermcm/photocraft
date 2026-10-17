"""Render reference10's preset view offscreen and compare a uniformly registered dock.

No desktop window is opened. Every row uses the same scale/translation. Proprietary
preset recipes are absent; this report cannot certify full behavior parity.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--references', type=Path, required=True)
    p.add_argument('--image', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--executable', type=Path, default=Path('target/debug/examples/snapshot.exe'))
    a = p.parse_args()
    repo = Path(__file__).resolve().parent.parent
    spec_path = repo / 'design/camera-raw/presets-element-regions.json'
    spec = json.loads(spec_path.read_text(encoding='utf-8'))
    source = a.references / 'image-10.png'
    if sha(source) != spec['sourceSha256']:
        p.error('reference10 content hash does not match')
    original_hash = sha(a.image)
    a.output.mkdir(parents=True, exist_ok=False)
    capture = a.output / 'capture'
    tokens = json.loads((repo / 'design/camera-raw/tokens.json').read_text(encoding='utf-8'))
    tokens.update(json.loads((repo / 'design/camera-raw/presets-tokens.json').read_text(encoding='utf-8')))
    private_tokens = a.output / 'tokens.json'
    private_tokens.write_text(json.dumps(tokens, indent=2)+'\n', encoding='utf-8')
    layout = json.loads((repo / 'design/camera-raw/layout.json').read_text(encoding='utf-8'))
    layout['presets'] = json.loads((repo / 'design/camera-raw/presets-design.json').read_text(encoding='utf-8'))
    private_layout = a.output / 'layout.json'
    private_layout.write_text(json.dumps(layout, indent=2)+'\n', encoding='utf-8')
    subprocess.run([sys.executable, str(repo / 'scripts/capture_camera_raw_offscreen.py'),
                    '--executable', str(a.executable.resolve(strict=True)), '--image', str(a.image.resolve(strict=True)),
                    '--output', str(capture), '--layout', str(private_layout), '--tokens', str(private_tokens), '--states', 'collapsed', 'presets', '--size', '1440x1297', '--scale', '2'], check=True, cwd=repo)
    if sha(a.image) != original_hash:
        raise RuntimeError('The source fixture changed during capture')
    state = json.loads((capture / 'presets-state.json').read_text(encoding='utf-8'))['result']
    design = state['cameraRawDesign']
    if any(design[k] != v for k, v in [('panelWidth',270), ('railWidth',40), ('headerHeight',39),
                                       ('histogramHeight',124), ('metadataHeight',24), ('footerHeight',60)]):
        raise RuntimeError('fixed registration requires the measured shared chrome design')
    # Right dock x1130..1400; histogram and metadata end at y187.
    # The footer begins at y1237, giving the same 1050pt body as the source crop.
    rect = [2260, 374, 2800, 2474]
    subprocess.run([sys.executable, str(repo / 'scripts/compare_camera_raw.py'), '--source', str(source),
                    '--candidate', str(capture / 'presets.png'), '--source-rect', *map(str, spec['sourceRect']),
                    '--candidate-rect', *map(str, rect), '--width', '540', '--regions', str(spec_path),
                    '--output', str(a.output / 'overlay')], check=True, cwd=repo)
    report = {'sourceImage':10, 'sourceSha256':sha(source), 'fixtureSha256':original_hash, 'originalUnchanged':True,
              'capture':json.loads((capture / 'capture-report.json').read_text(encoding='utf-8')),
              'candidateRect':rect, 'fullEveryElementParityVerified':False,
              'audit':json.loads((a.output / 'overlay/element-audit.json').read_text(encoding='utf-8')),
              'remaining':'Preset recipes are absent; Create Preset, preset management and amount stay disabled. Category disclosures are view state and show unavailable content. Rail/chrome, typography and scrollbar extent are not a full behavior or all-19 parity pass.'}
    (a.output / 'presets-audit.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')


if __name__ == '__main__':
    main()
