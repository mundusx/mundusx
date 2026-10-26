# Local image-edit quality and dimensions

Run on the contributor with the source PNG present locally:

```bash
opengpu media --image-edit --quality --preserve-size generate --input-image /home/a/artifacts/jes1.png --prompt "Replace the background with a garden; preserve the person and clothing"
```

- `--preserve-size` keeps exact original pixel dimensions. It pads to the next multiple of 64 for inference and removes only that added padding before saving. Inputs already aligned to 64 are passed through without resizing. It never silently downscales a large source.
- `--quality` uses standard sampling, CFG 4, and 30 steps, without the Lightning LoRA. It reuses the installed FP8 base model; it does not convert the model to BF16 or guarantee unchanged pixels in an edited image.
- `--steps 40` selects 20–50 steps in quality mode. More steps increase runtime and do not guarantee a better image.
- `--width 1024 --height 1536` chooses an explicit canvas instead of original size. Both values must be multiples of 64 between 64 and 2048. The input is fitted with padding, preserving its aspect ratio.
- The processing canvas is limited to 2,097,152 pixels and 2048 per side. Larger original images fail with an explanation; no lossy fallback is applied.
- Output is PNG. Source PNG remains unchanged. Generative edits do not guarantee exact identity, color, alpha, or metadata preservation.

Preview settings without a GPU generation:

```bash
opengpu media --image-edit --quality --preserve-size plan --input-image /home/a/artifacts/jes1.png
```

Local custom quality/size verification records are separate from the fixed, network-qualified 832×480 four-step profile. Network jobs retain their existing preset. This does not enable these flags in the network image-edit API.
