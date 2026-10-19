# The renderer comparison

M1's stage 3 ([docs/ROADMAP.md](../../docs/ROADMAP.md)): the measurements
behind decision D7, the 2D renderer. A standalone project, outside the
workspace, as `prototypes/decimal` is.

```sh
./fetch-fonts.sh                          # Noto Serif and Sans, checked by hash
cargo run --release -- render             # every candidate's renders, to target/out/
cargo run --release --features fallback -- hashes
cargo run --release -- bench [FRAMES]     # frame times, CPU and GPU
cargo run --release -- diff               # each render against vello_cpu's
```

**GPU measurements run on Windows,** from WSL: WSL's own Linux sees only
llvmpipe, a software rasterizer, while a Windows build sees the real GPU.
Build it with `cargo xwin build --release --target x86_64-pc-windows-msvc`
and run `target/x86_64-pc-windows-msvc/release/renderer-prototype.exe` from
WSL. `WGPU_BACKEND=dx12 WSLENV=WGPU_BACKEND` picks DirectX 12 (default:
Vulkan).

## Candidates

| Candidate | What it is |
|---|---|
| **Sparse strips:** `vello_cpu` 0.3 + `vello_gpu` 0.3 | One architecture in two renderers: the CPU one draws everything; the GPU one prepares strips on the CPU and rasterizes and composites them on the GPU. Same drawing API. `vello_gpu` was `vello_hybrid`, and has a WebGL backend as well as wgpu |
| **Research:** `vello` 0.11, with `vello_cpu` as reference | Vello's compute-centric GPU renderer, now called Vello Research. Needs compute shaders (WebGPU on the web) |
| **tiny-skia** 0.12 | A CPU renderer after Skia's; the GPU only receives finished frames |

## Corpus

Three pages, described once (`src/corpus.rs`) and drawn by every candidate.
Glyphs are drawn from outlines, unhinted. Text is laid out a glyph per
character, since shaping is stage 5's work.

| Page | Size | Content |
|---|---|---|
| text | 1588 × 2245 (A4 at 192 dpi) | a heading and eleven paragraphs of 11-point serif: 6,849 glyphs |
| grid | 1920 × 1080 | a sheet's grid: lines, shaded headers, a 12-pixel number in each of 1,300 cells: 10,263 glyphs |
| slide | 1920 × 1080 | gradients, a radial highlight, translucent curves, strokes, a title and bullets |

## Results (2026-10-19)

Measured on the project owner's workstation: AMD Ryzen 7 1800X (8 cores,
AVX2), NVIDIA GeForce GTX 1070 (driver 560.94), Windows 10 build 19041,
through WSL2.

### Frame time, CPU

Median of 20 frames, Windows build. "Glyph cache" is the sparse-strips
renderers' glyph atlas, kept across frames; the crates call it
experimental.

| Candidate | text | grid | slide |
|---|---|---|---|
| `vello_cpu`, 1 thread | 28.3 ms | 33.0 ms | 11.3 ms |
| `vello_cpu`, 8 threads | 23.6 ms | 25.8 ms | 9.2 ms |
| `vello_cpu`, 1 thread, glyph cache | 10.9 ms | 15.1 ms | 5.7 ms |
| `vello_cpu`, 8 threads, glyph cache | 6.6 ms | 11.8 ms | 2.0 ms |
| tiny-skia, 1 thread | 66.0 ms | 113.0 ms | 25.7 ms |

### Frame time, GPU (GTX 1070)

From building the scene to the GPU finishing, median of 20 frames.

| Candidate | text | grid | slide |
|---|---|---|---|
| `vello_gpu` | 15.1 ms | 24.3 ms | 1.9 ms |
| `vello_gpu`, glyph cache | 3.5 ms | 8.4 ms | 1.4 ms |
| `vello` | 4.4 ms | 5.6 ms | 1.8 ms |
| tiny-skia, then uploaded | 65.2 ms | 115.7 ms | 27.3 ms |

DirectX 12 gives the same frame times within a few percent.

### Start-up

Creating the GPU renderer, shaders included, before the first frame:

| Candidate | Vulkan (driver's shader cache warm) | DirectX 12, first run |
|---|---|---|
| `vello_gpu` | 26 ms | 594 ms |
| `vello` | 16 ms | 4,809 ms |

### How far each render is from the CPU reference

Against `vello_cpu`'s render at the detected SIMD level (AVX2): the largest
difference in any channel (0 to 255), and the share of pixels differing by
more than 1 and by more than 16. Identical on Vulkan, DirectX 12 and
llvmpipe.

| Candidate | text | grid | slide |
|---|---|---|---|
| `vello_gpu` | 1; 0.00%; 0% | 1; 0.00%; 0% | 6; 0.10%; 0% |
| `vello_gpu` with glyph cache, against `vello_cpu` with glyph cache | 1; 0.00%; 0% | 2; 0.00%; 0% | 6; 0.17%; 0% |
| `vello` | 60; 2.97%; 0.09% | 46; 4.40%; 0.20% | 55; 5.80%; 0.09% |
| tiny-skia | 65; 7.22%; 1.67% | 55; 15.86%; 4.79% | 74; 2.72%; 0.40% |
| `vello_cpu` with glyph cache | 57; 6.58%; 1.77% | 28; 13.26%; 2.78% | 50; 0.69%; 0.16% |

The glyph cache changes how glyphs are positioned, so cached renders differ
from uncached ones; cached CPU and cached GPU renders still agree.

### The same pixels everywhere?

Hashes of the CPU renders (`hashes`):

- **Threads:** `vello_cpu` gives identical pixels on 1 thread and on 4.
- **SIMD levels:** `vello_cpu`'s text page is identical at AVX2, SSE2 and
  the scalar fallback. Its grid and slide differ between AVX2 and SSE2, by
  at most 1 in a channel, on a handful of pixels. SSE2 matches the
  fallback.
- **Operating systems and processors:** CI's Linux and Windows runners
  (x86-64) and macOS runner (Arm, NEON), and the owner's workstation, give
  identical hashes level for level (the `Renderer prototype` workflow, run
  39322804148). NEON draws exactly what AVX2 draws, and the scalar
  fallback exactly what SSE2 draws, on every platform. tiny-skia is the
  same everywhere.

So `vello_cpu` has exactly two outputs: one for the wide SIMD paths (AVX2,
NEON) and one for the narrow ones (SSE2, the fallback), at most 1 apart in
any channel. A golden pinned to either reproduces on every platform.

### Size and WebAssembly

Each stack built alone (with wgpu, which tiny-skia also needs, to present
frames):

| Stack | Linux, stripped | Windows |
|---|---|---|
| `vello_cpu` + `vello_gpu` | 8.6 MB | 9.3 MB |
| `vello` + `vello_cpu` | 7.9 MB | 8.7 MB |
| tiny-skia | 5.7 MB | 6.4 MB |

All three build for `wasm32-unknown-unknown`.
