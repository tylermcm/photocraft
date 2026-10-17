"""Audit screenshot 11 using the native settings menu and one fixed registration.

A previous recipe is created through an actual commit before opening the menu.
No reference photo, font or application asset is copied into the product.
"""
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
    with path.open('rb') as f:
        return hashlib.file_digest(f,'sha256').hexdigest()

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--references',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--executable',type=Path,default=Path('target/debug/examples/snapshot.exe'))
    a=p.parse_args()
    repo=Path(__file__).resolve().parent.parent
    specs=repo/'design/camera-raw'
    regions=specs/'settings-menu-element-regions.json'
    spec=json.loads(regions.read_text(encoding='utf-8'))
    source=a.references/'image-11.png'
    if sha(source)!=spec['sourceSha256']:p.error('Reference 11 content hash does not match')
    executable=a.executable.resolve(strict=True)
    output=a.output.resolve();output.mkdir(parents=True,exist_ok=False)
    layout=specs/'layout.json';tokens=specs/'tokens.json'
    original_layout=layout.read_bytes();original_tokens=tokens.read_bytes()
    private=output/'layout.json';private.write_bytes(original_layout)
    fixture=output/'geometry.png';Image.new('RGB',(1200,960),'#808080').save(fixture)
    report={'complete':False,'fullEveryElementParityVerified':False,'sourceSha256':sha(source),'executableSha256':sha(executable),'layoutSha256':sha(layout),'tokensSha256':sha(tokens),'fixtureSha256':sha(fixture),'captures':{},'notes':'The previous recipe is real. Initial clipboard and snapshot lists are empty. Current underlying tab is Edit; source shows Presets. Preset/AI/DNG/export/defaults actions and selected-settings copy are not implemented, so enabled-state differences are retained. Clipboard currently uses native recipe JSON and accepts supported XMP; first external clipboard paste is available by shortcut, not pre-enabled in the menu.'}
    identity=None
    try:
        for name,opened in [('closed',False),('open',True)]:
            if layout.read_bytes()!=original_layout or tokens.read_bytes()!=original_tokens or sha(executable)!=report['executableSha256']:raise RuntimeError('Design or renderer changed during capture')
            script=[['ui.menu.invoke',{'id':'filter.cameraRaw','params':{'ui':{'set':{'exposure':1.0},'commit':True}}}],['ui.menu.invoke',{'id':'filter.cameraRaw','params':{'ui':{'workspace':{'settingsMenu':opened}}}}]]
            env=dict(os.environ,PHOTOCRAFT_CAMERA_RAW_LAYOUT_FILE=str(private),PHOTOCRAFT_THEME_FILE=str(tokens),PHOTOCRAFT_CONFIG_DIR=str(output/'profile'),PHOTOCRAFT_LOCALE='en')
            inspection=output/f'{name}.json';candidate=output/f'{name}.png'
            result=subprocess.run([str(executable),'--raw-workspace','--size','1440x1400','--scale','2','--open',str(fixture),'--script',json.dumps(script),'--out',str(candidate),'--inspect-out',str(inspection)],env=env,capture_output=True,text=True,timeout=180)
            (output/f'{name}.log').write_text(result.stdout+result.stderr,encoding='utf-8')
            if result.returncode:raise RuntimeError(f'{name}: native render failed')
            state=json.loads(inspection.read_text(encoding='utf-8'))['result']
            if state['cameraRawMenu']['previous']['exposure']!=1.0:raise RuntimeError('Previous settings were not retained from the real commit')
            if state['cameraRaw']['workspace']['settingsMenu']!=opened:raise RuntimeError('Requested menu state did not render')
            if state['cameraRaw']['params']['exposure']!=0.0:raise RuntimeError('Opening menu applied previous settings without a request')
            current=photo_identity(state)
            if identity is None:identity=current
            if current!=identity:raise RuntimeError('Menu changed the image recipe, document or revision')
            report['captures'][name]={'matchedRequestedView':True,'imageIdentityUnchanged':True,'state':inspection.name,'image':candidate.name}
            if opened:
                rect=state['cameraRawMenu']['rect']
                # One common translation includes the same cropped outer border/shadow
                # margin as the source: 2 physical pixels on left/top/bottom, none on right.
                registered=[round(rect[0]*2)-2,round(rect[1]*2)-2,round(rect[2]*2),round(rect[3]*2)+2]
                if [registered[2]-registered[0],registered[3]-registered[1]]!=[540,1234]:raise RuntimeError(f'Menu geometry differs from the measured source registration: {registered}')
                overlay=output/'overlay'
                compared=subprocess.run([sys.executable,str(repo/'scripts/compare_camera_raw.py'),'--source',str(source),'--candidate',str(candidate),'--source-rect',*map(str,spec['sourceRect']),'--candidate-rect',*map(str,registered),'--width','540','--regions',str(regions),'--output',str(overlay)],capture_output=True,text=True)
                (output/'overlay.log').write_text(compared.stdout+compared.stderr,encoding='utf-8')
                if compared.returncode:raise RuntimeError('Menu overlay failed')
                report['audit']=json.loads((overlay/'element-audit.json').read_text(encoding='utf-8'));report['candidateRect']=registered
            print(f'{name}: native state and image identity verified',flush=True)
        if sha(fixture)!=report['fixtureSha256']:raise RuntimeError('Original fixture changed')
        report['complete']=True
    finally:(output/'settings-menu-audit.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')

if __name__=='__main__':main()
