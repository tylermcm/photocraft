# Image Triage release integration

## 2026-11-04: v0.20.1 hosted validation completed

This supersedes the stopped checkpoint. The release merge was already published
as native 908e6e212 and paired host dbbc0503. This resume fixes issues found by
running the remaining live checks; it does not integrate unreleased upstream.

- Old integrated upstream: b1794c6e0608ccfc887213dcf58398977c5cb75e (v0.16.0).
- New integrated release: 327c4cc82e737a3e0af7c147a477126d77525495 (v0.20.1).
- Both native remotes fetched successfully. User-fork integration matched the
  published merge. Upstream main is 8e9277e20e8d5185ed4259908022a719a647237e,
  with 489 upstream-only and 83 integration-only commits at the start of this
  resume. v0.20.2 is a newer release, five commits beyond v0.20.1, not included
  in this bounded validation of the existing merge.
- The non-mutating upstream-main merge assessment reports four conflicts:
  crates/automation/src/workspace.rs, crates/io/src/lib.rs,
  crates/ui-egui/src/control/dispatch.rs, crates/ui-egui/src/jobs_ui.rs.
  No Camera Raw textual conflict; the initial 39 release-conflict decisions and
  automatic shared-file review remain recorded in Image Triage's docs/photocraft_handoff.md.

Corrections proved by this resume:

- Hosted startup removes the standalone update-check service, leaving the
  standalone app and update preference unchanged. The new v0.20.2 prompt had
  covered the embedded photo; two desktop tests cover hosted/standalone setup.
- While hosted Camera Raw owns the photo, only the read-only session.inspect
  engine query bypasses the modal command guard. Authorization still runs and
  document mutations remain blocked; two integration regressions pass.
- The host skips the covered full-editor fit command while Camera Raw is open;
  two host regressions cover open/closed behavior.
- Host mask recipes now validate bounded native geometry, adjustment ranges,
  combined parts and raster RLE without expanding pixels. Identity detection
  includes active local edits. Mask capability tests prove unsupported editors
  refuse before opening rather than losing masks.
- The real-RAW check fixture now supplies the current record/cached-thumbnail
  callbacks so native filmstrip polling does not raise UI callback exceptions.

Validation:

- Host focused bridge/recipe/render suite: 229 passed, two skipped.
- Hosted Camera Raw Workspace: 12 passed, including the corrected Luminance
  Slider query. Desktop updater and modal authorization regressions: four passed.
- Strict all-target clippy for desktop, automation and UI: passed.
- cargo xtask wasm: 24 crates plus codecs HEIF passed. A separate check in the
  native build target hit a rustc dependency access violation; the established
  tools target completed the full check. No repository build profile changed.
- cargo xtask test-corpus: 5,711 passed, zero failed, 38 ignored across
  36 test binaries. This uses environment-only release LTO=false and codegen
  units=16 plus the larger compiler stack; production profiles are unchanged.
- check_photocraft_handoff.py: hidden startup, native parent, persistent process,
  navigation/stash/restore, native menu Save, grid notifications, closed-tab
  reopening, five themes, native screenshot pixels and shutdown cleanup passed.
  Occluded desktop grabs are not counted as visible-desktop verification.
- check_camera_raw_first.py: edits survive navigation, a single document remains,
  commit keeps the full editor and closing returns to Camera Raw; passed.
- check_camera_raw_recipe.py: Canon DNG plus Nikon NEF, failed recipe write keeps
  draft, 4176x3062 live export, live/saved render equality, navigation persistence,
  full-editor continuation and native Save; passed.
- check_native_filmstrip.py: actual MainWindow passed with synthetic PNGs and
  separately with public Nikon NEF. Native clicks keep PID/grid selection,
  RAW recipe restore, ratings/reject, collapsed arrow/Home navigation,
  comparison pixels/toggle, resize, fullscreen, live theme reload and shutdown
  all passed. Originals remained byte-identical. Native captures inspected.
- Prior broad native suites remain valid: engine 2671, algo 760, I/O 493,
  RAW 140, pinned RAW corpus 104, desktop 213 + startup 14, automation 108 unit
  + 78 integration. UI library 4826 passed and integration main 131 passed;
  their 20 GPU device-creation failures all passed in fresh sequential processes.
  The cumulative GPU run is not described as an uninterrupted full-suite pass.

Local evidence is under logs/photocraft-v0201-resume/resume-20261104 in the
original Image Triage checkout (host-tests-final.log, handoff-fixed,
camera-raw-first-fixed, raw-recipe-clean, native-filmstrip-final and
native-filmstrip-raw-final). Native build/check logs are merge-*.log in the
isolated companion checkout. These local logs and public corpus files are not
published as source.

Validated candidate relative to the primary PhotoCraft checkout:
target/merge-v0.20.1-20261103/debug/photocraft.exe, SHA256
5e8edb349fc062e47ec713bd033aaf585c75efb29e41fecc0c97f0adce2e6ee9.
The four protected known-good executable hashes remain unchanged. This session
validates source and the explicit candidate; it does not replace an installed
editor or change the default launcher selection.

## 2026-11-03: v0.20.1 validation checkpoint

Native host base: ef1a2fd6af5a399b13f34636753f383ecae8856c.
Previous upstream: b1794c6e0608ccfc887213dcf58398977c5cb75e (v0.16.0).
Release merged in the isolated checkout: 327c4cc82e737a3e0af7c147a477126d77525495
(v0.20.1). Unreleased upstream main 37a01ca8746c121170644a07968d11263ef64407
is excluded. See Image Triage's docs/photocraft_handoff.md for all 39 conflict
paths, per-file resolutions and the reviewed automatic merges.

Host adapters follow upstream's split modules. Canonical upstream Calibration
replaces the duplicate stage; legacy recipe aliases and version-1 host keys
remain compatible. Shared translations use upstream entries, with host-only
context/source pairs retained. CPU fallback preserves settings the upstream
preview shader cannot apply. Standalone controls and localization tests follow
upstream; the embedded layout remains in its own host_controls module.

The real hosted handoff found upstream's new overwrite restriction rejecting
existing JSON manifests. The background host output service now uses a narrow
sidecar adapter: matching suffixes and versioned schemas in both old and new
JSON, bounded regular-file reads, the same write-root capability and atomic
writer. Ordinary automation writes keep upstream restrictions. All 108
automation unit tests, including this regression and denials, pass.

Native render algorithm and Image Triage RENDER_ALGORITHM are 3. Select the
paired host/editor updates together: earlier cached pictures are stale after
Calibration and RAW workflow changes. The current host base is origin/main
e1eeaf8, validated in a clean managed worktree. Existing dirty host checkouts
and the primary native checkout have been preserved.

Completed checks: engine 2671, algo 760, I/O 493, RAW 140 library tests;
public RAW corpus 104 tests; desktop 213 and control/startup 14; automation
108 unit plus 78 integration tests; current host focused/QML 272 tests.
The full UI library passed 4830 tests before the final accessibility updates;
the final combined native matrix is running. Strict all-target clippy passes
for automation, desktop and UI (the other affected crates passed earlier).
Layers 31 and wasm 24 plus HEIF pass. An offscreen Detail capture with Radius
2.8 was inspected, including strict accessibility checks.

Broader host checks reproduce 30 pre-existing failures with the original bridge:
missing mask validation/capability helpers and HostJobRunner mask/warmer APIs.
Those failures are not accepted as a green hosted handoff. The final native
matrix, optimized corpus and the rebuilt candidate's real hosted checks are
still pending. No commit, push or executable promotion at this checkpoint.

The optimized corpus build uses environment-only CARGO_PROFILE_RELEASE_LTO=false
and CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 with RUST_MIN_STACK=67108864. This
avoids the local MSVC thin-LTO test-linking bottleneck and earlier compiler
access violation; opt-level stays 3 and repository profiles are unchanged.

Candidate and snapshot outputs are under the primary companion checkout's
target/merge-v0.20.1-20261103/debug. Known-good debug, release, acr-settings-menu
and acr-calibration-sr executables are protected; original SHA-256 values and
the resolved-merge recovery patch are in the original host workspace's
logs/photocraft-v0201-resume. Do not promote before all hosted gates pass.

## Historical 2026-10-18 handoff

# Claude handoff: Camera Raw / Image Triage, 2026-10-18

## Start here

User wants a usable local testing checkpoint and continuation by Claude. Both repos
use `codex/image-triage-integration`. User authorized pushing everything to their
forks; no upstream PR, force push, release or default-editor promotion.

Local host: `USER_HOME/OneDrive/Documents/Playground`
Local editor: `USER_HOME/Documents/photocraft`
Forks: https://github.com/tylermcm/ImageTriage and https://github.com/tylermcm/photocraft.
PhotoCraft remote names differ from AGENTS examples: origin=user fork,
upstream=storytold. Never push to upstream.

## Usable development build

```powershell
& "USER_HOME/OneDrive/Documents/Playground/scripts/test_camera_raw.ps1" -EditorBuild acr-main-adjustments
```

Open photo in popout, press E. Candidate lives at
`USER_HOME/Documents/photocraft/target/acr-main-adjustments/debug/photocraft.exe`.
SHA256 `cac833711778b15699ccac589a30e1cb1ad2fbf3ad88ac0933b261b0fc0f1097`.
The existing default `acr-settings-menu` and known-good target/debug executable are
preserved. Build binaries and target/ are local, not committed to Git.

User is gaming: do not open visible apps or steal focus. Offscreen egui snapshots
are safe. Ask/coordinate before any live hosted test; secondary monitor and no
activation are required if launching is later authorized.

## Implemented

- Accepted ACR visual work: filmstrip, Mixer layout, Effects spacing/Glow row,
  Vibrance/Saturation gradients; leave negligible cosmetic differences alone.
- Glow processing: Diffusion/Bloom/Halation, amount, Range, Spread, Warmth.
- Color Mixer: All Colors displays all eight working HSL band controls.
- Point Color: up to eight samples, click picker, swatch select/delete, Hue,
  Saturation, Luminance, Variance, Range; cached view-only range visualization.
- Shared CameraRaw params support preview, commit/cancel, undo/redo, saved native
  projects, and per-photo recipes. New finishing params strictly validated.
- Private PhotoCraft XMP extension preserves settings; Adobe does not interpret
  it. PSD export explicitly refuses unsupported adjustments.
- `cameraRawFinishing:1` handoff capability; host refuses incompatible editors
  and RAW-recipe renderers rather than silently dropping new controls.

Core source: `crates/algo/src/camera_raw/finishing.rs`,
`crates/ui-egui/src/camera_raw_finishing_ui.rs`, `camera_raw_mixer_ui.rs`,
`camera_raw_ui.rs`, `camera_raw_workspace_ui.rs`; engine lens_cmds, I/O
camera_raw_xmp, UI raw_recipe, host photocraft_bridge and full_render.
Other preexisting host work included in checkpoint: narrow Qt TIFF warning filter
and tests, fork strategy and RAW smart-object planning documents, toolbar reference.
Those planning docs are historical proposals, not current implementation claims.

## Validation and limits

Host focused suite111 passed/2 skipped; Qt warning tests2 passed. UI library3127
passed/13 ignored before last narrow fixes; focused UI26 passed; final Mixer6
passed. Finishing algorithm5, engine finishing2 +noise3, adversarial command test,
I/O13 passed/1 ignored. Final native app and snapshot built. Affected-crate
all-target clippy, layers and wasm passed. Diff whitespace and launcher parse checked.
No claim of complete crate-wide final test matrix or Adobe pixel-output parity.

Optimized Rust compiler stalled/crashed locally. Temporary external Cargo config
uses opt-level0 for algorithm and engine in dev/test, without changing repository
Cargo profiles. Copy of override in `docs/handoff/2026-10-18-main-adjustments/debug-profile.toml`.
Use `cargo --config <absolute-config-path> build -p photocraft -j 1` and
`RUST_MIN_STACK=33554432` if reproducing. Preserve known-good executable before
building into target/debug; do not overwrite selected build until gates pass.
The selected candidate also has `validated-design/layout.json` and `tokens.json`.

Proof images and metadata are in PhotoCraft `docs/handoff/2026-10-18-main-adjustments/`.
They are real offscreen native captures; captures precede the final small SDR
point-selection clamp fix, covered by the final algorithm regression and Mixer tests.
Detailed logs/proof remain locally under
`USER_HOME/.codex/visualizations/2026/10/17/01a14901-444b-7781-b5e6-48730a8f5183/acr-filmstrip`.

Full real-file corpus attempt blocked: no local photocraft-corpus authoring checkout.
Live hosted gates still pending: persistent filmstrip navigation, automatic stash/
restore, native Save/grid propagation, closed-tab reopen, hidden startup, theme/
resize/fullscreen, shutdown and child-process cleanup. Do these before promotion.

Remaining disabled main controls include Optics, Lens Blur, Calibration and AI
Detail. The three requested features are complete for testing, not the entire
adjustment window. Existing Red Eye handoff describes durable recipe work still
needed. Inspect actual enabled controls before declaring another missing list.

## Upstream state and next actions

Fresh fetch succeeded for both forks and PhotoCraft upstream during checkpoint.
Upstream main advanced from last recorded5c89f16b to56dbc89a. Before checkpoint,
local native HEAD was dcda93d8: 70 local /215 upstream commits divergent;
origin integration had no fork-only commits and was87 commits behind local.
Host pre-checkpoint HEAD4fec508. This is an intentional preservation checkpoint:
upstream merge is deferred to Claude so the tested candidate is not replaced by
an unvalidated merge. No conflict resolutions or upstream-merge validation claimed.
AGENTS requires upstream integration in normal development; reconcile it deliberately,
review automatic merges in shared host/control/theme/window files, and retain ours
only for host functionality upstream lacks. Read docs/photocraft_handoff.md first.

Priority: user testing of this candidate, fix concrete functional regressions,
then remaining tools. No more pixel polishing or upstream-contribution preparation
unless requested. Do not create new goals or schedule background work.
