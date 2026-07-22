# Sparse typed QCGA3D browser showcase

This demo presents the compiler-generated 128x128 rotated-quadric kernel through
the shared Worker, MessagePort, and main-thread WebGPU actor runtime. The canvas
scales responsively; the pixel-edge toggle and loupe inspect the actual fixed
kernel output. There are no pretend algebra controls because this first sparse
QCGA kernel has no runtime quadric or camera parameters.

Generate the five ignored assets from the reviewed local Sonatina commit and
serve the common demos root:

```sh
SONATINA_DIR=/workspace/sonatina demos/webgpu-qcga3d-quadric/serve.sh
```

Then open `http://127.0.0.1:8000/webgpu-qcga3d-quadric/`. Set
`FORCE_QCGA_REGEN=1` to regenerate an existing bundle.

The real-browser equality gate compares all 16,384 Worker-Wasm and WebGPU
pixels. It also has a presentation-only contract that fetches no Wasm/reference,
creates no Worker, and performs no readback:

```sh
CHROME_BIN=/path/to/chrome demos/webgpu-qcga3d-quadric/smoke-chrome.sh
CHROME_BIN=/path/to/chrome QCGA_MODE=off demos/webgpu-qcga3d-quadric/smoke-chrome.sh
```
