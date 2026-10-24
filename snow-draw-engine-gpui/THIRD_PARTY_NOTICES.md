# Third-party notices

This project is licensed under Apache-2.0; see [LICENSE](LICENSE). It uses the
Apache-2.0 UI-independent Snow Draw Engine crates from `../snow_draw_engine_qt/crates`.
The Smart Erase implementation adapts the surface, periodic, perceptual guide and
PatchMatch algorithm in that project's `src/rendering/snow_canvas_smart_erase_algorithm.cpp`.
It is independently expressed in Rust and links neither Qt nor OpenCV.

Direct dependencies include GPUI (`gpui-pre` / platform 0.3.9, Apache-2.0),
resvg/usvg 0.45.1 (Apache-2.0 OR MIT), tiny-skia (BSD-3-Clause), png 0.18.1 (MIT OR Apache-2.0), image
(MIT OR Apache-2.0), fontdb (MIT), Rustybuzz (MIT), serde/serde_json
(MIT OR Apache-2.0), base64 (MIT OR Apache-2.0), anyhow (MIT OR Apache-2.0),
unicode-segmentation (MIT OR Apache-2.0), and unicode-bidi (MIT OR Apache-2.0).
GPUI also resolves its own resvg/usvg 0.46.0. The full resolved graph and the
original license texts, rather than this summary, control redistribution.

The repository collector was run against this Cargo manifest on 2026-10-23 with
`--locked --offline` graph resolution. Its scoped output is:

`../artifacts/snow-draw-gpui-licenses-streaming/third-party/INDEX.md`

It collected 512 notice files for 284 external Rust packages, plus 21 vcpkg entries
and repository attribution notices. The collector is shared with Snow Shot and its
index retains its generic Snow Media title and superset vcpkg/project entries;
this GPUI binary does not link Qt or that vcpkg package set.

Four published crate archives omit their workspace license files. Exact upstream
texts and provenance are supplied in `licenses/cargo/`. They were copied from the
existing audited reference checkout; `SOURCE.md` records revision-specific upstream
URLs. `seahash`'s provenance explicitly records the later upstream license revision.
A canonical Apache-2.0 text supports dual-licensed archives that omit a notice.
No license files or package cache contents were changed in the reference checkout.

System fonts are loaded from the user's operating system; no proprietary font
files are distributed. The procedural showcase background and sample annotations
are generated within this project and require no external image attribution.
