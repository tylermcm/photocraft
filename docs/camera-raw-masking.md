# Camera Raw Masking: design and plan

Status 2026-10-20: **slices 1-5 are built and tested**: Linear and Radial gradients, Subject and Background (BiRefNet, found by Image Triage), Sky, Objects, Brush, Color Range and Luminance Range. Slice 6 (combining) and Landscape and People are built too (see the 2026-10-20 addendum at the end). The Masking tab layout is a general one (`camera_raw_masking_ui.rs`, geometry in `design/camera-raw/masking-design.json`); an ACR 18.6 parity pass needs screenshots.

## What exists, and what "hooking up the buttons" really needs

The editor already has the *selection algorithms* the panel's buttons stand for. They are classical (no learned models) and each returns a coverage mask:

| Panel button | Existing code | Note |
|---|---|---|
| Subject | `algo::segment::subject::select_subject(sampler, canvas)` | saliency + GrabCut; heuristic |
| Sky | `algo::segment::sky::select_sky(sampler, canvas, SkyParams)` | colour prior + guided filter |
| Background | inverse of Subject | |
| Objects | `select.object` (GrabCut from a box / quick selection) | needs an on-canvas box or brush |
| Range | `select.colorRange`, `select.loadLuminosity` | colour and luminance ranges; depth range needs a depth map (none) |
| Brush | none in the selection code; strokes painted into a coverage map | new |
| Linear / Radial Gradient | none; plain geometry | new, trivial |
| Landscape | needs scene segmentation (water, mountains, vegetation...); PhotoCraft has none (Image Triage's OneFormer is Python) | not available; Sky only |
| People | needs a person/face model | stays "not available" |

A selection algorithm only *makes* a mask. Camera Raw masking is a **list of masks in the photo's settings, each with its own local
adjustments**, applied in the preview, the export and the saved recipe, and undoable. That layer did not exist; it is slice 1 below, and
everything else (buttons, overlay, undo, export, host) sits on it. Same shape as Point Color (a list of structured items in `CameraRaw`).

## Design

* `CameraRaw.masks: Vec<LocalMask>` (`crates/algo/src/camera_raw/masks.rs`). A `LocalMask` is a `MaskShape` (`Linear`, `Radial`, or `Raster`), `invert`,
  `amount` (0..100), `enabled`, a name, and `LocalAdjust` (temperature, tint, exposure, contrast, highlights, shadows, whites, blacks, texture,
  clarity, dehaze, saturation).
* Geometry is image-relative (0..1 of width and height), so a mask means the same on the 900 px preview proxy and the full-size render.
* A raster mask (Subject, Sky, Background, Brush, Range, Objects) is an 8-bit coverage map at most 1536 px a side, stored run-length coded and base64 inside the
  settings. Flat areas are a few bytes. It is sampled bilinearly at any size. **Known limit:** at export the edge is only as sharp as the stored map;
  a guided upsample against the full-size picture (as `local_tone` already does for Shadows) is the planned refinement.
* Local adjustments run through the same `develop()` as the global ones on a copy, then blend in by coverage: a mask covering everything is
  *exactly* the global control (a test asserts this), so every future global control can be local for free. Masks apply last, after the global edits.
* `masks` is in `validate`, `is_identity`, and (because `CameraRaw` is serde) undo/redo, the recipe, and `non_defaults` for free.
  Still to add, following Calibration/Super Resolution: the private XMP extension key, the host recipe whitelist and identity check,
  a `cameraRawMasks` capability and the host gate.
* Cost: a local pass copies the picture, so each active mask costs about one `develop`. Fine on the preview proxy; a full-size export of 45 MP
  with several masks needs ~700 MB per pass. Revisit with tiling if it matters.

## Slices

1. **Engine layer (done):** model, coverage, local adjustments, validation, round trip. Tests: `crates/algo/tests/camera_raw_masks.rs` (7) and a unit test for the run-length text.
2. **Persistence and host:** XMP extension key; `photocraft_bridge.py` whitelist/identity/capability; render path check (`raw_recipe::apply` and `app.render`).
3. **Gradients end to end (first thing a user can see):** the Masking tab's Linear (G) and Radial (J) cards create a mask by dragging on the photo; on-canvas handles to
   move/resize/rotate; a mask list (select, rename, invert, amount, enable, delete); the local sliders; the red overlay while editing.
4. **Automatic masks:** Subject, Sky, Background from the existing algorithms on the preview proxy, with a progress state and Cancel (they take a moment);
   Objects with an on-canvas box.
5. **Brush (K) and Range:** painted strokes into a coverage map (size, feather, flow, erase); colour and luminance range with a sampler.
6. **Combining masks** (add, subtract, intersect), and more local controls (sharpness, noise, colour grading).

People and Landscape stay "not available" (no model for them here).

## Status of the slices (2026-10-20)

* Slices 1-3 done (engine, persistence + host gate, gradients end to end).
* Slice 4 done, with one change from the plan: **Subject and Background use BiRefNet, as the old built-in editor did**, not the classical `select_subject`. The editor writes its preview picture to a PNG and asks Image Triage over the host-job channel (`ui.hostJob`, kind `subjectMask`: `input`, `output`). The host (`subject_mask_service.py`, `subject_mask_worker.py`) runs `birefnet_worker.generate_subject_mask` under the managed torch runtime with the same progress popup and Cancel as Super Resolution, and the editor turns the grayscale PNG into a raster mask (Background is the same map, inverted). Without Image Triage the two cards say so. Sky (`segment::sky`) and Objects (`grabcut::object_select` / `object_at`, a dragged box or a click) run in the editor.
* Slice 5 done: Brush (K; size, feather, flow, erase, Alt erases; strokes edit the selected raster mask or make a new one; `camera_raw_mask_paint_ui.rs`), Color Range (click a colour; Range and Smoothness) and Luminance Range (the corner of the Range card; Min, Max, Smoothness). Range masks are evaluated from the picture's own pixels at the point they are applied.
* Known limits: the subject worker starts a fresh torch process per request (about 48 s cold on the author's GPU); Brush maps are 1024 px on the long side; Range masks have no depth range; Landscape and People stay unavailable.

## Addendum 2026-10-20: combining, Landscape, People

* **Combining masks.** `LocalMask.parts: Vec<MaskPart { op: add | subtract | intersect, shape, invert }>`, applied in order after the mask's first shape (`MAX_PARTS` = 8). The rules are the ones Image Triage's masks used: add is the larger of the two at each pixel, subtract multiplies by (1 - part) so a soft edge stays soft, intersect multiplies. A part can be a gradient, a brush map, a Range, or anything else a mask can be, and a Range part looks at the picture. In the panel the selected mask lists its shapes (click one to edit its handles and settings, x removes it) and Add / Subtract / Intersect make the next shape join it instead of becoming a new mask. Tests: `crates/algo/tests/camera_raw_masks.rs`.
* **Landscape** is OneFormer (ADE20k) through Image Triage: sky, water, mountains, vegetation (trees and foliage as one), buildings, animals, listed by how much of the picture each covers. The user ticks which to make; each becomes its own mask (parts, when Add / Subtract / Intersect is on). Hovering a row shows its coverage on the photo.
* **People** is the old pipeline: OneFormer finds where people are, SAM splits that into one mask per person (`people_instances.py`), BiRefNet polishes each person's edge (`prompt_masks._refine_with_birefnet`: SAM's mask is dilated and everything outside it greyed out so a touching neighbour cannot merge back in). Small pieces touching a much larger person (a sleeve) are dropped as fragments. All three models run in the one mask-engine host process (`mask_engine_service.py`), so only the first request in a session pays for loading them.
* Both go over the same host-job channel as Subject (`sceneMasks`, `peopleMasks`); the host writes grayscale PNGs and a `manifest.json` into a folder the editor named (`image_triage/scene_mask_jobs.py`).
* **Depth Range is not built on purpose:** it is not a Photoshop mask. `depth_worker.py` was restored only because the mask-engine host imports it.
* The old OneFormer / SAM / mask-engine modules had been deleted with the built-in editor (commit 36cf879 in Image Triage) and were restored from history, together with the download helpers in `ai_model.py` and the `scene_masks`, `subject_masks` and `sam_masks` capabilities in `ai_manifest.py` (so Set Up AI installs the torch runtime again).
* **Warm-up.** Image Triage loads OneFormer, BiRefNet and SAM in the background as soon as the editor connects (`image_triage/mask_warmup.py`, ticked from the host-job poll) and keeps the host alive while the editor is open, so the first mask after that is 0.8 to 5 s instead of a 32 s cold start (torch and CUDA importing from a cold file cache). `IMAGE_TRIAGE_MASK_WARMUP=0` turns it off.
