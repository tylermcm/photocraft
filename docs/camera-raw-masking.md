# Camera Raw Masking: design and plan

Status 2026-10-20: **slices 1-5 are built and tested**: Linear and Radial gradients, Subject and Background (BiRefNet, found by Image Triage), Sky, Objects, Brush, Color Range and Luminance Range. Not built: Landscape and People (no model here), combining masks (slice 6). The Masking tab layout is a general one (`camera_raw_masking_ui.rs`, geometry in `design/camera-raw/masking-design.json`); an ACR 18.6 parity pass needs screenshots.

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
