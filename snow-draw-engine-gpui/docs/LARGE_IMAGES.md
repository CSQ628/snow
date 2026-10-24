# Large screenshots, scrolling captures and export

The original 16 Mi-pixel source/output cap was unsuitable for Snow Shot. Three
3840×2160 displays contain 24,883,200 pixels; a 1920×33,000 scrolling capture
contains 63,360,000 pixels. Neither should require one equally large compositor
surface. The implementation now separates owned source, query, working raster,
encoded file and output limits.

## Existing source contracts

- `snow_shot/src/presentation/capture/screenshotscrollingnativesource.cpp` accepts
  native RGBA frames with width at most `INT_MAX/4`, height at most `INT_MAX`,
  checked stride and backing length. It has no 16,384-height restriction.
- `screenshotscrollingpipeline.cpp` retains native stitch snapshots and materializes
  requested viewport regions. Exported snapshot dimensions are checked against
  `int`; viewport previews need not materialize the entire scroll result.
- `snow_shot/src/storage/capturehistoryrepository.cpp` allows 64,000,000 pixels
  per stored image, 128,000,000 per record and 32 displays. These are persistence
  limits, not an assertion that all live captures are capped at those sizes.
- `snow_image/include/snow/image/codec.h` independently defaults owned decode to
  1 GiB, working bytes to 512 MiB, input to 8 GiB, and dimensions to 1,048,576.
  Codec/format limits are separate again.

The new library does not integrate Snow Shot's native stitch object or codec
service. It accepts immutable owned RGBA source buffers and host layers; the host
can publish only its current viewport source for lazy scrolling previews. An
out-of-core row/tile source provider remains a future host integration, not a
claimed capability of `SourceImage`.

## Implemented contracts

| Resource | Limit / behavior |
| --- | --- |
| Owned immutable source | 8 GiB RGBA, width ≤ `INT_MAX/4`, height ≤ `INT_MAX`; allocation already belongs to caller. No full-size render copy. |
| PNG/JPEG/WebP decode | Default 512 MiB decoded RGBA; explicit budget up to 8 GiB. Dimensions and RGBA bytes checked before native decode/conversion. Encoded input ≤512 MiB. Codec limits also apply. |
| Layer projection | 128 layers, 1 GiB input pixels, 512 MiB per projected visual/eligible composite, 128 Mi projected sample operations. Effective coverage limits projection regions. |
| Working compositor | 16 Mi pixels, each dimension ≤16,384. This limits a tile including effect support, not the source or entire output. |
| Full-region snapshot | Coarse engine query density adapts to effect halos. Output geometry retains its requested camera/pixel grid and is recomposed at output density. |
| PNG / packed PNG writer | Up to 8 GiB decoded output RGBA, width ≤2,097,152 (8 MiB scanline), height ≤`INT_MAX`; canvas bounds within ±1e9; scale 0.1–8. Rows are streamed, not collected as one raster. |
| Bands / tiles | At most 8 MiB and 128 rows per band; tiles ≤1024 output pixels wide, plus sampling support. Local dependency expansion preserves pre-cull source-pass/effect-run identities. |
| Effect support | At most8192 physical pixels; tile plus halo must fit working limit. Large overlapping effect chains may fail explicitly even when output itself fits. Disjoint effects do not accumulate irrelevant global radii. |
| SVG writer | Same output/grid limits; streams PNG through base64 into SVG. This preserves exact composited effects, not editable vector nodes. Small ordinary `export_snapshot_svg` retains paths/outlined text. |
| Convenience PNG/SVG buffers | Encoded output≤96 MiB; writer APIs avoid this buffer. |
| Drawing files | ≤512 MiB JSON envelope with engine session and PNG assets. Capture engine state on owner thread; encode/decode assets via `DrawingSnapshot` on worker. Default save/load budgets both reject assets above512 MiB RGBA; paired `encode_with_limit` / `decode_with_limit` permit an explicit per-asset budget up to8 GiB. |
| History assets | 1 GiB /128 distinct image assets, deduplicated by immutable source identity. Metadata follows the core engine history lifetime. Untracked initial source publication uses `set_background`. |
| Smart Erase | Native donor ROI≤2 Mi pixels /512 MiB conservative scratch; only affected pixels plus bilinear guard are retained in128 MiB cache. Large source does not enlarge solve to full source. |

Uniform full-region exports require an output aspect ratio agreeing with the
requested canvas region to within one output pixel; anisotropic source layers
are supported separately. Dimension rounding is shared by full/streamed/tiled/SVG
paths: ceiling after f64 multiplication of integer extent by the stored f32 scale.
Fractional source sampling uses a fixed16-bit texel fraction to avoid translation
roundoff crossing an 8-bit channel tie differently at adjacent strip boundaries.

The limits are conservative resource guards, not a guarantee that an8 GiB source
can be allocated on every machine. Large source ownership is still contiguous and
the adapter does not duplicate it to build a full output. A future tiled source
provider can reuse the snapshot/export boundary without changing document history.

## Evidence

Focused tests cover a64×300,000 source with local Smart Erase, a32×20,000 output,
packed distant regions, fractional scales, multi-pass blur support, exact16 Mi-pixel
output plus required halos, thick connector/magnifier-leader culling, cancellation,
and full/strip pixel agreement. The optimized benchmark also streams11520×2160
and256×100,000 sources to a sink and reports peak working-surface pixels separately
from input bytes. See [VALIDATION.md](VALIDATION.md) for measured results.
