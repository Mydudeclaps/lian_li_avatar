# Patch v3 image-generation provenance

Both v3 pose sheets were created with the built-in OpenAI image-generation
tool in identity-preservation mode, using
`source/patch-pose-sheet.png` as the character reference. The generated
working copies are versioned under `assets/mascot/source/` and were
chroma-keyed with the image-generation workflow's `remove_chroma_key.py`
helper. Machine-local generation paths and IDs are intentionally not
published.

## Micro-idles and farewell sheet

Working files:

```text
assets/mascot/source/patch-micro-idles-v3-chroma.png
assets/mascot/source/patch-micro-idles-v3.png
```

Prompt:

```text
Use case: identity-preserve
Asset type: production game-sprite pose sheet for the existing Patch OLED mascot
Input image: the supplied image is the character identity, outfit, proportions, palette, and illustration-style anchor; do not edit that file and do not redesign the character.
Primary request: create a NEW exact 4-column by 2-row sprite sheet containing eight distinct full-body micro-idle and farewell poses of the same original character Patch. Read left-to-right across the top row, then left-to-right across the bottom row:
1) neutral standing blink pose, both eyes gently closed, tiny content smile;
2) adjusting the brim of his straw hat with one hand;
3) cheerful full-body stretch with both arms overhead, feet grounded;
4) seated comfortably cross-legged holding a small plain warm mug with both hands, no steam;
5) leaning slightly toward and pointing at a small round brass temperature gauge, curious focused expression;
6) bracing playfully against a light breeze with one hand on his hat and teal scarf fluttering;
7) friendly goodbye wave with one hand, warm smile, feet grounded;
8) polite straw-hat tip/bow, one hand on brim, calm proud smile.
Subject invariants: preserve Patch's exact youthful chibi face, brown hair, straw hat with red band, teal scarf, navy coat with gold trim, cream shirt, rust-red shorts, belt pouch, brown boots, body proportions, line weight, shading, and personality from the reference.
Composition: exact equal-size 4x2 cells; exactly one complete Patch pose per cell; consistent character scale and baseline except the seated pose; generous empty padding on every side. Absolutely no body part, hat brim, scarf, prop, or stray pixel may touch or cross a cell boundary. No overlap or bleed between cells, no neighboring-pose fragments, no cropped limbs, no extra hands or feet.
Scene/backdrop: perfectly flat uniform chroma green #00FF00 over the entire canvas and all gaps.
Constraints: crisp polished 2D chibi game-sprite illustration; transparent-ready edges; small props only where explicitly requested; no shadows, floor plane, glow, gradients, texture, lighting variation, dividers, borders, labels, text, logos, watermark, particles, or extra objects. Do not use #00FF00 in the character or props.
```

## Activity, notification, and thermal sheet

Working files:

```text
assets/mascot/source/patch-events-v3-chroma.png
assets/mascot/source/patch-events-v3.png
```

Prompt:

```text
Use case: identity-preserve
Asset type: production game-sprite pose sheet for the existing Patch OLED mascot
Input image: the supplied image is the character identity, outfit, proportions, palette, and illustration-style anchor; do not edit that file and do not redesign the character.
Primary request: create a NEW exact 4-column by 2-row sprite sheet containing eight distinct full-body activity, notification, and thermal-reaction poses of the same original character Patch. Read left-to-right across the top row, then left-to-right across the bottom row:
1) overheated and fanning his face with one hand, a few simple blue sweat drops, tired-but-comical expression;
2) confidently using a small plain brass wrench on a tiny abstract mechanism, energetic focused expression, subtle cyan accent sparks;
3) startled attention pose beside a small plain brass ship bell, one hand raised, wide alert eyes;
4) harmless error reaction, puzzled expression while holding one short loosely tangled red cable, no text and no danger;
5) proud confirmed-completion pose with a warm smile and one thumbs-up while tipping the straw hat, a few tiny gold celebration stars;
6) studying through a small round magnifying glass, curious concentrated expression, subtle amber accent motes;
7) listening carefully with one hand cupped behind one ear, patient expectant expression;
8) cooled-down cozy relief pose hugging a small pale-blue cold-water flask, relaxed content smile and one tiny snowflake accent.
Subject invariants: preserve Patch's exact youthful chibi face, brown hair, straw hat with red band, teal scarf, navy coat with gold trim, cream shirt, rust-red shorts, belt pouch, brown boots, body proportions, line weight, shading, and personality from the reference.
Composition: exact equal-size 4x2 cells; exactly one complete Patch pose per cell; consistent character scale and baseline; generous empty padding on every side. Absolutely no body part, hat brim, scarf, prop, effect, or stray pixel may touch or cross a cell boundary. No overlap or bleed between cells, no neighboring-pose fragments, no cropped limbs, no extra hands or feet.
Scene/backdrop: perfectly flat uniform chroma green #00FF00 over the entire canvas and all gaps.
Constraints: crisp polished 2D chibi game-sprite illustration; transparent-ready edges; small props/effects only where explicitly requested; no shadows, floor plane, glow, gradients, texture, lighting variation, dividers, borders, labels, readable text, logos, watermark, or extra objects. Do not use #00FF00 in the character, props, or effects.
```
