"""Capture all 19 supplied references with one stable native renderer/profile.

No visible windows. Registration is shared by every region in a reference;
uncompared pixels and unavailable capabilities are never hidden or called parity.
"""
import argparse
import base64
import copy
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import sys

from PIL import Image
from audit_camera_raw_reference_views import REGISTRATIONS
from capture_camera_raw import STATES, ordered_panels, photo_identity

FRAGMENTS = {"saveOptions": "save-options", "preferences": "preferences",
             "presets": "presets", "crop": "crop", "masking": "masking", "remove": "remove"}


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def guard(pins):
    for path, digest in pins.items():
        if sha(path) != digest:
            raise RuntimeError(f"Input changed during matrix: {path.name}")


def contains(actual, expected):
    if isinstance(expected, dict):
        return isinstance(actual, dict) and all(k in actual and contains(actual[k], v) for k, v in expected.items())
    if isinstance(expected, list):
        return isinstance(actual, list) and len(actual) == len(expected) and all(contains(a, b) for a, b in zip(actual, expected))
    if isinstance(expected, float) and isinstance(actual, (float, int)) and not isinstance(actual, bool):
        return math.isclose(actual, expected, abs_tol=1e-6, rel_tol=1e-6)
    return actual == expected


def navigation_matches(actual, requested):
    expected = copy.deepcopy(requested)
    if expected.get("zoom") == "fit":
        expected["zoom"] = None  # Native serde representation of fit-to-view.
    return contains(actual, expected)


def signature(source, candidate):
    sw, sh = source[2]-source[0], source[3]-source[1]
    cw, ch = candidate[2]-candidate[0], candidate[3]-candidate[1]
    if min(sw, sh, cw, ch) <= 0 or not math.isclose(cw/sw, ch/sh, abs_tol=1e-9):
        raise ValueError("Registration must have positive dimensions and one uniform scale")
    scale = cw/sw
    return tuple(round(v, 8) for v in (scale, candidate[0]-source[0]*scale, candidate[1]-source[1]*scale))


def union_area(rects):
    xs = sorted({v for r in rects for v in (r[0], r[2])})
    area = 0
    for left, right in zip(xs, xs[1:]):
        spans = sorted((r[1], r[3]) for r in rects if r[0] < right and r[2] > left)
        length, start, end = 0, None, None
        for lo, hi in spans:
            if end is None or lo > end:
                if end is not None:
                    length += end-start
                start, end = lo, hi
            else:
                end = max(end, hi)
        if end is not None:
            length += end-start
        area += (right-left)*length
    return area


def profile(specs):
    layout, tokens = read(specs/"layout.json"), read(specs/"tokens.json")
    for key, name in FRAGMENTS.items():
        layout[key] = read(specs/f"{name}-design.json")
    for name in ["save-options", "preferences", "presets"]:
        tokens.update(read(specs/f"{name}-tokens.json"))
    layout.pop("preview", None)
    return layout, tokens


def plans(specs, inventory):
    by_hash = {r["sha256"]: r["image"] for r in inventory["references"]}
    jobs = {}
    for _, section, state, rect in REGISTRATIONS:
        spec = read(specs/f"{section}-element-regions.json")
        number = by_hash[spec["sourceSha256"]]
        job = jobs.setdefault(number, {"image": number, "state": state, "size": [1440, 1400], "audits": []})
        job["audits"].append({"spec": f"{section}-element-regions.json", "candidateRect": rect})
    for number, state, size, rect in [
        (6, "save-options", [1440, 1400], None), (7, "preferences", [1440, 1400], None),
        (10, "presets", [1440, 1297], [2260, 374, 2800, 2474]),
        (11, "settings-menu", [1440, 1400], None),
        (13, "masking", [1440, 1000], [2260, 374, 2800, 1434]),
        (14, "crop", [1440, 1000], [2260, 374, 2800, 1834]),
        (15, "remove", [1440, 1000], [2260, 374, 2800, 1464]),
    ]:
        jobs[number] = {"image": number, "state": state, "size": size,
                        "audits": [{"spec": f"{state}-element-regions.json", "candidateRect": rect}]}
    for view in read(specs/"full-window-reference-views.json")["references"]:
        number = view["image"]
        jobs[number] = {"image": number, "state": "full-client", "size": [2174, 1301], "view": view,
                        "audits": [{"spec": view["regions"], "candidateRect": [0, 0, 4348, 2602]}]}
        if number == 19:
            # Derive this crop from the same full-client transform, never a second alignment.
            film = read(specs/"filmstrip-element-regions.json")
            sx, sy = view["sourceRect"][:2]
            rect = [v-(sx if i % 2 == 0 else sy) for i, v in enumerate(film["sourceRect"])]
            jobs[number]["audits"].append({"spec": "filmstrip-element-regions.json", "candidateRect": rect})
    if set(jobs) != set(range(1, 20)):
        raise ValueError("Matrix must include every reference exactly once")
    named = [a["spec"] for j in jobs.values() for a in j["audits"]]
    if len(set(named)) != len(named) or set(named) != {p.name for p in specs.glob("*element-regions.json")}:
        raise ValueError("Matrix must compare every versioned region specification exactly once")
    for job in jobs.values():
        sigs = set()
        for audit in job["audits"]:
            spec = read(specs/audit["spec"])
            if by_hash[spec["sourceSha256"]] != job["image"]:
                raise ValueError("Specification belongs to another source image")
            if audit["candidateRect"]:
                sigs.add(signature(spec["sourceRect"], audit["candidateRect"]))
        if len(sigs) > 1:
            raise ValueError(f"Reference {job['image']} has inconsistent section registrations")
    return [jobs[n] for n in sorted(jobs)]


def workspace(job):
    value = {"tab": "edit", "info": False, "panels": {"open": [], "single": False}}
    if job["state"] == "full-client":
        value.update(job["view"]["workspace"])
    elif job["state"] == "preferences":
        value["preferences"] = True
    elif job["state"] == "settings-menu":
        value.update(tab="presets", settingsMenu=True)
    else:
        value.update(STATES[job["state"]][1])
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--references", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--executable", type=Path, default=Path("target/debug/examples/snapshot.exe"))
    args = parser.parse_args()
    repo = Path(__file__).resolve().parent.parent
    specs = repo/"design/camera-raw"
    inventory = read(specs/"reference-inventory.json")
    jobs = plans(specs, inventory)
    layout, tokens = profile(specs)
    executable = args.executable.resolve(strict=True)
    pins = {p: sha(p) for p in specs.glob("*.json")}
    pins[executable] = sha(executable)
    for name in ["audit_camera_raw_matrix.py", "audit_camera_raw_reference_views.py", "capture_camera_raw.py", "compare_camera_raw.py"]:
        path = repo/"scripts"/name
        pins[path] = sha(path)
    for ref in inventory["references"]:
        path = args.references/ref["file"]
        if sha(path) != ref["sha256"]:
            parser.error(f"Wrong reference content: {ref['image']}")
        pins[path] = ref["sha256"]
    args.output.mkdir(parents=True, exist_ok=False)
    fixture = args.output/"geometry.png"
    Image.new("RGB", (6880, 5504), "#808080").save(fixture)
    pins[fixture] = sha(fixture)
    (args.output/"layout.json").write_text(json.dumps(layout, indent=2)+"\n", encoding="utf-8")
    token_path = args.output/"tokens.json"
    token_path.write_text(json.dumps(tokens, indent=2)+"\n", encoding="utf-8")
    pins[token_path] = sha(token_path)
    report = {"capturesComplete": False, "fullEveryElementParityVerified": False,
              "executableSha256": pins[executable], "baseLayoutSha256": sha(args.output/"layout.json"),
              "tokensSha256": pins[token_path], "fixtureSha256": pins[fixture], "references": [],
              "inputPins": [{"path": str(p.resolve()), "sha256": v} for p, v in pins.items()],
              "notes": "Named-region measurements only. Crop coverage is not element completeness. Synthetic photo/EXIF/output/capability differences are retained. No visible desktop launch or editor promotion."}
    baseline_identity = None

    def capture(job, folder, requested, menu_open=None):
        guard(pins)
        design = copy.deepcopy(layout)
        state = job["state"]
        if state in STATES:
            design["panelOrder"] = ordered_panels(layout["panelOrder"], STATES[state][0])
        design["preview"] = copy.deepcopy(requested)
        if state == "settings-menu":
            design["preview"]["settingsMenu"] = False
        private = folder/"layout.json"
        private.write_text(json.dumps(design, indent=2)+"\n", encoding="utf-8")
        commands = []
        if state == "settings-menu":
            commands.append(["ui.menu.invoke", {"id": "filter.cameraRaw", "params": {"ui": {"set": {"exposure": 1.0}, "commit": True}}}])
        ui = {"workspace": requested}
        if state == "full-client":
            ui.update(scope=job["view"]["scope"], view=job["view"]["navigation"])
        if menu_open is not None:
            ui["workspace"] = dict(requested, settingsMenu=menu_open)
        commands.append(["ui.menu.invoke", {"id": "filter.cameraRaw", "params": {"ui": ui}}])
        if state == "full-client":
            thumb = {"width": 40, "height": 32, "rgba": base64.b64encode(bytes([128, 128, 128, 255])*40*32).decode()}
            commands.append(["ui.filmstrip", {"generation": 1, "total": 1, "current": 0, "collapsed": job["view"]["collapsed"],
                             "items": [{"index": 0, "id": "standalone", "name": fixture.name, "version": "standalone", "thumbnail": thumb}]}])
        env = dict(os.environ, PHOTOCRAFT_CAMERA_RAW_LAYOUT_FILE=str(private.resolve()), PHOTOCRAFT_THEME_FILE=str(token_path.resolve()),
                   PHOTOCRAFT_CONFIG_DIR=str((folder/"profile").resolve()), PHOTOCRAFT_LOCALE="en")
        run = subprocess.run([str(executable), "--raw-workspace", "--size", "x".join(map(str, job["size"])), "--scale", "2",
                              "--open", str(fixture.resolve()), "--script", json.dumps(commands), "--out", str(folder/"native.png"),
                              "--inspect-out", str(folder/"state.json")], env=env, capture_output=True, text=True, timeout=240)
        (folder/"native.log").write_text(run.stdout+run.stderr, encoding="utf-8")
        if run.returncode:
            raise RuntimeError(f"Reference {job['image']} snapshot failed")
        guard(pins)
        actual = read(folder/"state.json")["result"]
        expected = ui["workspace"]
        if not contains(actual["cameraRaw"]["workspace"], expected):
            raise RuntimeError(f"Reference {job['image']} rendered the wrong view")
        for key in FRAGMENTS:
            if not contains(actual["cameraRawDesign"].get(key), layout[key]):
                raise RuntimeError(f"Reference {job['image']} did not load profile fragment {key}")
        if actual["cameraRawDesign"]["panelOrder"] != design["panelOrder"]:
            raise RuntimeError("Wrong panel ordering")
        with Image.open(folder/"native.png") as im:
            if list(im.size) != [n*2 for n in job["size"]]:
                raise RuntimeError("Wrong native pixel dimensions")
        if state == "full-client":
            view = job["view"]
            if not contains(actual["cameraRaw"]["scope"], view["scope"]) or not navigation_matches(actual["cameraRaw"]["view"], view["navigation"]) or actual["filmstrip"]["collapsed"] != view["collapsed"]:
                raise RuntimeError("Wrong scope, navigation or filmstrip view")
        return actual

    try:
        # A standalone baseline proves that view changes do not edit the photo.
        first = {"image": 0, "state": "collapsed", "size": [1440, 1400]}
        folder = args.output/"baseline"
        folder.mkdir()
        baseline_identity = photo_identity(capture(first, folder, workspace(first)))
        for job in jobs:
            folder = args.output/f"ref{job['image']:02}"
            folder.mkdir()
            requested = workspace(job)
            menu_identity = None
            if job["state"] == "settings-menu":
                closed = folder/"closed"
                closed.mkdir()
                menu_identity = photo_identity(capture(job, closed, requested, False))
            actual = capture(job, folder, requested)
            identity = photo_identity(actual)
            if identity != (menu_identity or baseline_identity):
                raise RuntimeError(f"Reference {job['image']} changed photo/document/recipe identity")
            if menu_identity and (actual["cameraRawMenu"]["previous"]["exposure"] != 1.0 or actual["cameraRaw"]["params"]["exposure"] != 0.0):
                raise RuntimeError("Previous settings were not created by the real setup commit")
            result = {"image": job["image"], "matchedRequestedView": True, "imageIdentityUnchanged": True,
                      "setupCommit": menu_identity is not None, "fullReferenceVerified": False, "audits": []}
            sigs, crops = set(), []
            for audit in job["audits"]:
                spec = read(specs/audit["spec"])
                rect = audit["candidateRect"]
                if rect is None:
                    if job["state"] == "settings-menu":
                        r = actual["cameraRawMenu"]["rect"]
                        rect = [round(r[0]*2)-2, round(r[1]*2)-2, round(r[2]*2), round(r[3]*2)+2]
                    else:
                        r = actual["cameraRawPreferences"]["rect"] if job["state"] == "preferences" else actual["cameraRawSaveOptions"]["state"]["rect"]
                        rect = [round(v*2) for v in r]
                sigs.add(signature(spec["sourceRect"], rect))
                out = folder/Path(audit["spec"]).stem
                command = [sys.executable, str(repo/"scripts/compare_camera_raw.py"), "--source", str(args.references/f"image-{job['image']}.png"),
                           "--candidate", str(folder/"native.png"), "--source-rect", *map(str, spec["sourceRect"]), "--candidate-rect", *map(str, rect),
                           "--width", str(spec["normalizedWidth"]), "--regions", str(specs/audit["spec"]), "--output", str(out)]
                subprocess.run(command, check=True, capture_output=True, text=True)
                measured = read(out/"element-audit.json")
                if not all(e["candidateCoverageComplete"] for e in measured["elements"]):
                    raise RuntimeError(f"Reference {job['image']} has missing candidate coverage")
                result["audits"].append({"spec": audit["spec"], "sourceRect": spec["sourceRect"], "candidateRect": rect,
                                         "comparison": str((out/"comparison.png").relative_to(args.output)), **measured})
                crops.append(spec["sourceRect"])
            if len(sigs) != 1:
                raise RuntimeError("Sections in one reference use different transforms")
            result["sourceToCandidateTransform"] = list(next(iter(sigs)))
            source = args.references/f"image-{job['image']}.png"
            with Image.open(source) as im:
                result["comparedSourceCropFraction"] = union_area(crops)/(im.width*im.height)
            result["allSourceElementsAccountedFor"] = False
            report["references"].append(result)
            print(f"reference {job['image']}: state/profile/identity verified; {sum(len(a['elements']) for a in result['audits'])} regions", flush=True)
        guard(pins)
        report["capturesComplete"] = True
    finally:
        report["auditedElementCount"] = sum(len(a["elements"]) for r in report["references"] for a in r["audits"])
        report["regionSpecificationCount"] = sum(len(r["audits"]) for r in report["references"])
        (args.output/"matrix-audit.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")


if __name__ == "__main__":
    main()
