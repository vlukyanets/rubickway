# Quickstart: GPU Buffers

Manual checks for behaviour that exists only on real hardware (Principle III), including real image
imports, which no automated test does (research G11). Automated tests cover everything else:
`cargo test`, with no device needed.

## Prerequisites

- Logged in on a text console, no desktop running; SSH session open for recovery.
- `cargo build --release`.
- Keep the log: `RUST_LOG=rubickway=debug cargo run --release 2> /tmp/rubickway.log`.
- A renderer-reporting program: `eglinfo -B -p wayland` (package `mesa-utils`), run inside a kitty
  window. Below, "reports X" means its `OpenGL ES profile renderer` line.
- For a GPU load: `eglgears_wayland` (also `mesa-utils`). For a video: `mpv`, if installed.
- To see whether RubickWay opened the second GPU, from SSH or tty2:
  `ls -l /proc/$(pidof rubickway)/fd | grep nvidia`, and the NVIDIA render node from
  `/dev/dri/by-path/` (card and render numbers change between boots).

## US1: applications on the GPU

| # | Do | Expect |
| --- | --- | --- |
| Q1 | Start RubickWay. Read the log. | `composing on Mesa Intel(R) UHD Graphics 620 ..., offering applications <n> image formats`. |
| Q2 | Open kitty, run `eglinfo -B -p wayland`, 10 times. | Reports the Intel GPU, not llvmpipe (SC-002). If not, stop here: `EGL_LOG_LEVEL=debug WAYLAND_DEBUG=1 eglinfo -B -p wayland 2>&1 \| less` shows which step failed. |
| Q3 | Time kitty's start, 5 times, one at a time: the log's `application <id> connected` against `window <id>: first frame presented`. | Under 200 ms (SC-001). |
| Q4 | Run `eglgears_wayland` on a window showing on both monitors. Pan and zoom out to 25%. | Real imports: correct colours and transparency on both displays; smooth at low zoom, not sparkling. |
| Q5 | Move, resize, fullscreen and close kitty. | The same as before this feature. |
| Q6 | If a player is available, play a video with `mpv --vo=dmabuf-wayland --hwdec=vaapi`: that hands RubickWay the decoder's two-plane images, while `--vo=gpu-next` draws them into its own RGB image first. | Two-plane import works: colours not green or pink. |
| Q7 | Leave `eglgears_wayland` running for 10 minutes. Compare from tty2: `ls /proc/$(pidof rubickway)/fd \| wc -l` at the start and the end. | No flicker, no growth (SC-005). |
| Q8 | Ctrl+Alt+F2 and back; suspend and resume. | All windows correct without redrawing (SC-007). |
| Q9 | Run a session: kitty, `eglgears_wayland`, quit. Check fds during it. | No `/dev/nvidia*` and no NVIDIA render node (SC-008). |
| Q10 | `__NV_PRIME_RENDER_OFFLOAD=1 eglinfo -B -p wayland`, then `__NV_PRIME_RENDER_OFFLOAD=1 eglgears_wayland` (what `prime-run` sets for EGL; no package needed). | Reports NVIDIA; the window shows correctly with no flicker, through the NVIDIA driver's explicit synchronisation if it uses it (FR-009, FR-011). RubickWay itself still opens no NVIDIA device. `WAYLAND_DEBUG=1` with the same command shows whether `wp_linux_drm_syncobj_surface_v1` is in use; record which. Seen on 2026-11-04 with driver 580.196.07 on the MX130: `eglgears_wayland` only advances while the pointer moves over its window, in niri too, while with driver 615.78.08 on an RTX 4060 it runs by itself, in niri and in RubickWay. So check a real program as well: `mpv --vo=gpu-next --gpu-api=opengl --gpu-context=wayland` plays smoothly with the pointer away. Seen on 2026-11-05, not looked into: on the MX130 it gave 2 frames a second with the pointer still at 100% zoom, yet seemingly ran by itself zoomed out, if the pointer was indeed still then. |

## Distant windows (FR-017)

Compare by eye and in the frame-time lines, zoomed out to 25%, with `glmark2-es2-wayland -b terrain:duration=20 --size 3840x2160` running and the pointer moving.

| # | Do | Expect |
| --- | --- | --- |
| Q13 | No `distant-windows` in `config.kdl`. | As it was before this block: no startup line about distant windows. |
| Q14 | `distant-windows { below-zoom-percent 50; max-fps 15; }`. Restart. | The startup line names it. Zoomed out, the window updates about 15 times a second and the desktop is smoother. At 100% it runs as usual. |
| Q15 | `distant-windows { below-zoom-percent 50; copy-detail-percent 100; }`. Restart. | Zoomed out, the desktop is smoother, the window slightly less sharp and it may shimmer on fine detail. Back at 100% it is sharp. |

## US2: correct frames, slow applications

| # | Do | Expect |
| --- | --- | --- |
| Q11 | Run a slow program next to `eglgears_wayland`: `glmark2-es2-wayland -b terrain --size 7680x4320` (package `glmark2`), or any program below 6 fps. Keep panning and moving the pointer at 100%, then zoomed out to 25%, with and without `distant-windows`. Log at debug level. | The slow one updates at its own pace, in order, with no half-drawn frames. The frame-time lines give the frames a second of SC-003; no `libinput error: ... lagging behind` line and no `frame work` line of 10 ms or more, except one for each new size of a copy. Read stderr itself: libinput writes there without our log format. |
| Q12 | A misbehaving case that needs a real GPU: start a program whose format the GPU refuses, if one is at hand; otherwise record that case 5 was checked only through its automated double. | One log line naming the application; nothing else happens (SC-006). |

A program that never finishes a frame cannot be produced by hand: SC-004 is automated
(`tests/protocol/misbehaving.rs`, case 7).

## US3: without GPU buffers

Automated: the offer decision, and shared-memory windows next to GPU images through the protocol tests.
A shared-memory program next to GPU ones is seen in Q5 if `foot` or another shared-memory client is at hand.
