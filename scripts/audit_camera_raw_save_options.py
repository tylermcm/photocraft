"""Render Save Options offscreen and compare reference6 with one full-dialog registration."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from PIL import Image
from capture_camera_raw import photo_identity


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--references', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--executable', type=Path, default=Path('target/debug/examples/snapshot.exe'))
    a = parser.parse_args()
    repo = Path(__file__).resolve().parent.parent
    design_dir = repo / 'design/camera-raw'
    spec_path = design_dir / 'save-options-element-regions.json'
    spec = json.loads(spec_path.read_text(encoding='utf-8'))
    source = a.references / 'image-6.png'
    if sha(source) != spec['sourceSha256']:
        parser.error('reference6 does not match the pinned content hash')
    a.output.mkdir(parents=True, exist_ok=False)
    layout = json.loads((design_dir / 'layout.json').read_text(encoding='utf-8'))
    layout['saveOptions'] = json.loads((design_dir / 'save-options-design.json').read_text(encoding='utf-8'))
    tokens = json.loads((design_dir / 'tokens.json').read_text(encoding='utf-8'))
    tokens.update(json.loads((design_dir / 'save-options-tokens.json').read_text(encoding='utf-8')))
    private_layout, private_tokens = a.output / 'layout.json', a.output / 'tokens.json'
    private_layout.write_text(json.dumps(layout, indent=2)+'\n', encoding='utf-8')
    private_tokens.write_text(json.dumps(tokens, indent=2)+'\n', encoding='utf-8')
    fixture = a.output / 'geometry.png'
    Image.new('RGB', (1200,960), '#808080').save(fixture)
    executable = a.executable.resolve(strict=True)
    report = {'complete':False, 'sourceImage':6, 'sourceSha256':sha(source),
              'executableSha256':sha(executable), 'layoutSha256':sha(private_layout), 'tokensSha256':sha(private_tokens),
              'fixtureSha256':sha(fixture), 'fullEveryElementParityVerified':False, 'captures':{},
              'notes':'Native DNG export is absent, so Save remains disabled. Editable choices are export draft/view state only. The actual document name/folder are used; no reference path or account identity is copied. Dynamic values and unavailable capabilities are retained as measured differences.'}
    identity = None
    try:
        for name, opened in [('closed',False), ('open',True)]:
            env = dict(os.environ, PHOTOCRAFT_CAMERA_RAW_LAYOUT_FILE=str(private_layout), PHOTOCRAFT_THEME_FILE=str(private_tokens),
                       PHOTOCRAFT_CONFIG_DIR=str(a.output / 'profile'), PHOTOCRAFT_LOCALE='en')
            script = [['ui.menu.invoke', {'id':'filter.cameraRaw', 'params':{'ui':{'workspace':{'saveOptions':opened}}}}]]
            png, state_file = a.output / f'{name}.png', a.output / f'{name}.json'
            run = subprocess.run([str(executable), '--raw-workspace', '--size','1440x1400','--scale','2', '--open',str(fixture),
                                  '--script',json.dumps(script),'--out',str(png),'--inspect-out',str(state_file)],env=env,
                                 capture_output=True,text=True,timeout=180)
            (a.output / f'{name}.log').write_text(run.stdout+run.stderr,encoding='utf-8')
            if run.returncode:
                raise RuntimeError(f'{name}: native snapshot failed')
            state = json.loads(state_file.read_text(encoding='utf-8'))['result']
            if state['cameraRaw']['workspace']['saveOptions'] != opened or state['cameraRawSaveOptions']['canSave']:
                raise RuntimeError('Save Options visibility or capability differs from the requested view')
            photo = photo_identity(state)
            if identity is None:
                identity = photo
            if photo != identity:
                raise RuntimeError('Opening Save Options changed the photo, recipe or document')
            report['captures'][name] = {'matchedRequestedView':True,'imageIdentityUnchanged':True,'image':png.name,'state':state_file.name}
            if opened:
                r = state['cameraRawSaveOptions']['state']['rect']
                crop = [round(v*2) for v in r]
                if [crop[2]-crop[0],crop[3]-crop[1]] != [1574,2256]:
                    raise RuntimeError(f'Native dialog differs from the fixed 787x1128 design: {r}')
                subprocess.run([sys.executable, str(repo/'scripts/compare_camera_raw.py'), '--source',str(source), '--candidate',str(png),
                                '--source-rect',*map(str,spec['sourceRect']),'--candidate-rect',*map(str,crop),'--width','1574',
                                '--regions',str(spec_path),'--output',str(a.output/'overlay')],check=True,cwd=repo)
                report['candidateRect'] = crop
                report['audit'] = json.loads((a.output/'overlay/element-audit.json').read_text(encoding='utf-8'))
        if sha(fixture) != report['fixtureSha256'] or sha(executable) != report['executableSha256']:
            raise RuntimeError('Original fixture or renderer changed during capture')
        report['complete'] = True
    finally:
        (a.output/'save-options-audit.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')


if __name__ == '__main__':
    main()
