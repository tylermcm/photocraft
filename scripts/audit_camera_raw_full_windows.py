"""Capture and audit full-window references with one fixed client registration.

Synthetic pixels are never substituted for the reference photo in the product.
Raw overlays retain content differences; the report does not certify parity.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

from PIL import Image
from capture_camera_raw import photo_identity


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--references", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--executable", type=Path, default=Path("target/debug/examples/snapshot.exe"))
    args = parser.parse_args()
    repo = Path(__file__).resolve().parent.parent
    specs = repo / "design/camera-raw"
    views = json.loads((specs / "full-window-reference-views.json").read_text(encoding="utf-8"))
    layout_bytes = (specs / "layout.json").read_bytes()
    token_bytes = (specs / "tokens.json").read_bytes()
    executable = args.executable.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    fixture = output / "geometry.png"
    Image.new("RGB", (6880, 5504), "#808080").save(fixture)
    fixture_hash = hashlib.sha256(fixture.read_bytes()).hexdigest()
    report = {"fullEveryElementParityVerified": False, "capturesComplete": False,
              "executableSha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
              "layoutSha256": hashlib.sha256(layout_bytes).hexdigest(),
              "tokensSha256": hashlib.sha256(token_bytes).hexdigest(),
              "fixtureSha256": fixture_hash, "references": [],
              "notes": "One client translation and uniform scale per screenshot. Photo, EXIF, file kind and output settings differ. Every region remains unverified as a parity pass."}
    identity = None
    thumbnail = {"width": 40, "height": 32, "rgba": base64.b64encode(bytes([128, 128, 128, 255]) * 40 * 32).decode("ascii")}
    try:
        for view in views["references"]:
            if hashlib.sha256(executable.read_bytes()).hexdigest() != report["executableSha256"]:
                raise RuntimeError("Renderer changed during capture; rerun one stable build")
            number = view["image"]
            source = args.references / f"image-{number}.png"
            if hashlib.sha256(source.read_bytes()).hexdigest() != view["sha256"]:
                raise RuntimeError(f"reference {number}: wrong content hash")
            if (specs / "layout.json").read_bytes() != layout_bytes or (specs / "tokens.json").read_bytes() != token_bytes:
                raise RuntimeError("Design changed during capture; rerun one stable revision")
            design = json.loads(layout_bytes)
            design["preview"] = view["workspace"]
            private_layout = output / f"ref{number}-layout.json"
            private_layout.write_text(json.dumps(design), encoding="utf-8")
            script = [
                ["ui.menu.invoke", {"id": "filter.cameraRaw", "params": {"ui": {"workspace": view["workspace"], "scope": view["scope"], "view": view["navigation"]}}}],
                ["ui.filmstrip", {"generation": 1, "total": 1, "current": 0,
                                  "items": [{"index": 0, "id": "standalone", "name": fixture.name, "version": "standalone", "thumbnail": thumbnail}], "collapsed": view["collapsed"]}],
            ]
            environment = dict(os.environ, PHOTOCRAFT_CAMERA_RAW_LAYOUT_FILE=str(private_layout),
                               PHOTOCRAFT_THEME_FILE=str(specs / "tokens.json"),
                               PHOTOCRAFT_CONFIG_DIR=str(output / "profile"), PHOTOCRAFT_LOCALE="en")
            candidate = output / f"ref{number}.png"
            inspection = output / f"ref{number}-state.json"
            captured = subprocess.run([str(executable), "--raw-workspace", "--size", "2174x1301", "--scale", "2",
                                       "--open", str(fixture), "--script", json.dumps(script), "--out", str(candidate),
                                       "--inspect-out", str(inspection)], env=environment, capture_output=True, text=True, timeout=180)
            (output / f"ref{number}.log").write_text(captured.stdout + captured.stderr, encoding="utf-8")
            if captured.returncode:
                raise RuntimeError(f"reference {number}: render failed; see capture log")
            state = json.loads(inspection.read_text(encoding="utf-8"))["result"]
            raw = state["cameraRaw"]
            for key, expected in view["workspace"].items():
                if raw["workspace"].get(key) != expected:
                    raise RuntimeError(f"reference {number}: workspace {key} did not match")
            for key, expected in view["scope"].items():
                if raw["scope"].get(key) != expected:
                    raise RuntimeError(f"reference {number}: scope {key} did not match")
            if state["filmstrip"]["collapsed"] != view["collapsed"]:
                raise RuntimeError("Wrong filmstrip state")
            with Image.open(candidate) as image:
                if image.size != (4348, 2602):
                    raise RuntimeError("Wrong native capture size")
            current = photo_identity(state)
            if identity is None:
                identity = current
            if current != identity:
                raise RuntimeError("View switching changed the image recipe or document")
            regions = specs / view["regions"]
            comparison = output / f"ref{number}-overlay"
            subprocess.run([sys.executable, str(repo / "scripts/compare_camera_raw.py"), "--source", str(source),
                            "--candidate", str(candidate), "--source-rect", *map(str, view["sourceRect"]),
                            "--candidate-rect", "0", "0", "4348", "2602", "--width", "2174",
                            "--regions", str(regions), "--output", str(comparison)], check=True, capture_output=True)
            audit = json.loads((comparison / "element-audit.json").read_text(encoding="utf-8"))
            report["references"].append({"image": number, "sha256": view["sha256"],
                                         "matchedRequestedView": True, "imageIdentityUnchanged": True,
                                         "fullReferenceVerified": False, "sourceRect": view["sourceRect"],
                                         "viewer": f"{comparison.name}/index.html", **audit})
            print(f"reference {number}: native state verified; {len(audit['elements'])} regions measured", flush=True)
        if hashlib.sha256(fixture.read_bytes()).hexdigest() != fixture_hash:
            raise RuntimeError("Synthetic original changed")
        report["capturesComplete"] = True
    finally:
        report["auditedElementCount"] = sum(len(item["elements"]) for item in report["references"])
        (output / "full-window-audit.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
