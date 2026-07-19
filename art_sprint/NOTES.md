# Patch crewmate art sprint provenance

All four candidates were created with the built-in OpenAI image-generation tool using `assets/mascot/animations/patch-idle-blink-v3.png` and `assets/mascot/animations/patch-event-success-v3.png` as style/identity references. The final files retain the generated flat chroma-green backgrounds because this sprint explicitly prohibited running scripts, including the normal chroma-removal helper.

## Navigator concept

Working file:

```text
art_sprint/concept-navigator.png
```

Prompt:

```text
Use case: style-transfer
Asset type: fan-original game character concept candidate for the Patch OLED mascot project
Input images: Image 1 and Image 2 are style, chibi proportion, line-weight, shading, palette-warmth, and finish references for Patch; do not edit them, do not reproduce Patch, and do not copy any existing manga or anime character.
Primary request: create ONE original chibi navigator girl crewmate in a single full-body standing pose. She has an energetic, upbeat personality and holds a small plain brass compass in one hand and a compact rolled map in the other. Design a clear original pirate-adventure silhouette that complements Patch while remaining visibly distinct.
Subject: youthful chibi navigator girl; warm brown or auburn hair arranged in a practical windswept bob or ponytail; expressive bright eyes; layered seafaring outfit with orange and amber accents, warm cream fabric, deep brown boots and belt details; compact adventurer proportions; no straw hat and no teal scarf.
Style/medium: match the supplied Patch references exactly in crisp polished 2D chibi game-sprite illustration style, rounded youthful proportions, clean dark line weight, warm cel shading, subtle painted texture, readable small details, and friendly personality.
Composition: exactly one complete character, centered, single full-body standing pose, front three-quarter view, fully visible from hair to boots, generous empty padding on every side, clean isolated silhouette.
Scene/backdrop: perfectly flat uniform chroma green #00FF00 over the entire canvas.
Constraints: original character only; compass and rolled map are the only props; natural hands and feet; no cropped limbs; no extra fingers, hands, feet, characters, props, scenery, effects, shadows, floor plane, glow, gradients, texture, lighting variation, labels, readable text, logos, watermark, or borders. Transparent-ready crisp edges. Do not use #00FF00 anywhere in the character or props.
```

Design note: Orange-and-amber navigator with an outgoing silhouette, prominent brass compass, and rolled map; distinct from Patch while sharing his warm nautical finish.

## Doctor concept

Working file:

```text
art_sprint/concept-doctor.png
```

Prompt:

```text
Use case: style-transfer
Asset type: fan-original game character concept candidate for the Patch OLED mascot project
Input images: Image 1 and Image 2 are style, chibi proportion, line-weight, shading, palette-warmth, and finish references for Patch; do not edit them, do not reproduce Patch, and do not copy any existing manga or anime character.
Primary request: create ONE original small, round, huggable ship's-doctor creature in a single full-body standing pose: a chibi sea otter mascot, explicitly NOT a reindeer. Give it a kind, slightly earnest personality. It carries a tiny plain medical satchel and holds one simple thermometer.
Subject: original round sea-otter character with compact paws, small rounded ears, warm cocoa-brown fur, pale muzzle and belly, tiny whiskers, soft blue and mint accent garments, a small sailor-doctor cap with a plain blue patch containing no symbol or logo, and a tiny medical satchel with no emblem.
Style/medium: match the supplied Patch references exactly in crisp polished 2D chibi game-sprite illustration style, rounded youthful proportions, clean dark line weight, warm cel shading, subtle painted texture, readable small details, and friendly personality.
Composition: exactly one complete creature, centered, single full-body standing pose, front three-quarter view, fully visible from ears to feet and tail, generous empty padding on every side, clean isolated silhouette.
Scene/backdrop: perfectly flat uniform chroma green #00FF00 over the entire canvas.
Constraints: original animal mascot only; clearly a sea otter, never a reindeer; tiny medical satchel and thermometer are the only props; no medical cross or protected symbol; natural paws and feet; no cropped limbs; no extra paws, feet, tails, characters, props, scenery, effects, shadows, floor plane, glow, gradients, texture, lighting variation, labels, readable text, logos, watermark, or borders. Transparent-ready crisp edges. Do not use #00FF00 anywhere in the creature, garments, or props.
```

Design note: Round cocoa-brown sea otter doctor with soft blue/mint uniform accents, a thermometer, and an emblem-free satchel for a gentle thermal-reaction owner.

## Shipwright concept

Working file:

```text
art_sprint/concept-shipwright.png
```

Prompt:

```text
Use case: style-transfer
Asset type: fan-original game character concept candidate for the Patch OLED mascot project
Input images: Image 1 and Image 2 are style, chibi proportion, line-weight, shading, palette-warmth, and finish references for Patch; do not edit them, do not reproduce Patch, and do not copy any existing manga or anime character.
Primary request: create ONE original stocky chibi shipwright kid in a single full-body standing pose. The kid has an inventive, confident wildcard personality, wears practical goggles, carries a comically too-big plain wrench, and has a compact tool belt.
Subject: youthful stocky chibi shipwright kid; tousled dark brown hair; brass-and-leather goggles resting on the forehead; rolled-sleeve work jacket with teal and bronze accents over a warm cream shirt; reinforced cropped work trousers; sturdy brown boots; tool belt with a few simple non-sharp tools; no straw hat.
Style/medium: match the supplied Patch references exactly in crisp polished 2D chibi game-sprite illustration style, rounded youthful proportions, clean dark line weight, warm cel shading, subtle painted texture, readable small details, and friendly personality.
Composition: exactly one complete character, centered, single full-body standing pose, front three-quarter view, fully visible from goggles to boots; the oversized wrench is held diagonally but remains fully inside the canvas; generous empty padding on every side; clean isolated silhouette.
Scene/backdrop: perfectly flat uniform chroma green #00FF00 over the entire canvas.
Constraints: original character only; oversized plain wrench, goggles, and tool belt only; no weapons; natural hands and feet; no cropped limbs or wrench; no extra fingers, hands, feet, characters, props, scenery, effects, shadows, floor plane, glow, gradients, texture, lighting variation, labels, readable text, logos, watermark, or borders. Transparent-ready crisp edges. Do not use #00FF00 anywhere in the character or props.
```

Design note: Stocky teal-and-bronze builder with oversized wrench and large brass goggles; the clearest mechanical wildcard while remaining friendly and youthful.

## Patch micro-idles v4 candidate

Working file:

```text
art_sprint/patch-micro-idles-v4-candidate.png
```

Prompt:

```text
Use case: identity-preserve
Asset type: production game-sprite pose sheet candidate for the existing Patch OLED mascot
Input images: Image 1 and Image 2 are the character identity, outfit, proportions, palette, line weight, shading, and illustration-style anchors; do not edit those files and do not redesign the character.
Primary request: create a NEW exact 4-column by 2-row sprite sheet containing eight distinct full-body micro-idle poses of the same original character Patch. Read left-to-right across the top row, then left-to-right across the bottom row:
1) napping while seated against an imaginary wall, straw hat tipped down over his closed eyes, peaceful expression; show no actual wall;
2) fishing with a tiny simple rod, relaxed expression, no fish and no water;
3) peering through a small plain brass spyglass, curious adventurous expression;
4) writing in a small plain logbook with a feather quill, focused content expression;
5) happily munching one small piece of plain bread;
6) balancing playfully on one foot with both arms out, cheerful expression;
7) lying on his stomach with knees bent and feet kicking upward, chin resting in both hands, dreamy content expression;
8) a tiny harmless comic sneeze, eyes squeezed shut and teal scarf flying upward, no text and no particles.
Subject invariants: preserve Patch's exact youthful chibi face, warm skin tone, brown hair, straw hat with red band, teal scarf, navy coat with gold trim, cream shirt, rust-red shorts, belt pouch, brown boots, body proportions, line weight, shading, palette warmth, and friendly personality from the references.
Composition: exact equal-size 4-column by 2-row cells; exactly eight poses total; exactly one complete Patch pose per cell; consistent character scale and standing baseline except the seated and lying poses; generous empty padding on every side of every cell. Absolutely no body part, hat brim, scarf, rod, fishing line, spyglass, logbook, quill, bread, or stray pixel may touch or cross a cell boundary. No overlap or bleed between cells, no neighboring-pose fragments, no cropped limbs, no extra hands or feet. Keep all eight poses fully contained in their own invisible cell.
Scene/backdrop: perfectly flat uniform chroma green #00FF00 over the entire canvas and all gaps.
Constraints: crisp polished 2D chibi game-sprite illustration matching the references exactly; transparent-ready edges; props only where explicitly requested; no shadows, floor plane, wall, water, fish, scenery, glow, gradients, texture, lighting variation, dividers, borders, labels, readable text, letters, logos, watermark, extra objects, extra characters, or extra poses. Do not use #00FF00 in Patch or any prop.
```

Design note: Eight quiet, playful shipboard idles in the requested 4x2 order, with strong identity retention and each pose visually isolated in its cell.

## Navigator core sheet v1 r2

Output file:

```text
art_sprint/navigator-core-sheet-v1-r2.png
```

Prompt:

```text
Use case: identity-preserve
Asset type: production chroma-green 4x2 sprite sheet for the Patch LCD mascot project
Input images: Image 1 is the edit target and exact layout, pose, palette, style, line-weight, shading, proportions, and character-identity anchor. Image 2 is an additional character identity reference only; do not otherwise copy its single-pose composition.
Primary request: Reproduce Image 1 as closely and literally as possible, changing exactly one thing only: in the bottom row, third cell, remove all floating yellow star particles around the celebrating character. Preserve that cell's character exactly as a joyful celebration conveyed only by her raised fist, expression, stance, and held rolled map. There must be nothing in that cell except the one complete character and her held map.
Subject: The same chibi navigator girl in all eight existing poses: auburn windswept ponytail with a small side braid; layered seafaring outfit with orange and amber accents, warm cream fabric, navy vest, deep brown boots and belt; brass compass and rolled map props as shown. No straw hat. No teal scarf.
Composition/framing: Preserve Image 1's exact equal-size 4 columns by 2 rows grid and the same eight poses in the same cell order. Exactly one complete character per cell. Generous empty padding on every side of every cell. No body part, hair, clothing, compass, map, or other prop may touch or cross any cell boundary.
Scene/backdrop: perfectly flat, uniform solid chroma green #00FF00 across the entire canvas, including every cell and all gaps. No tonal variation.
Style/medium: Match Image 1 exactly: polished chibi game sprite illustration, crisp dark line art, warm cel shading, identical proportions and finish, clean transparent-ready edges.
Constraints: Change only the floating yellow star particles in bottom-row pose 3 by removing them entirely. Keep every other pose, facial expression, hand gesture, prop, costume feature, silhouette, scale, position, spacing, color, line, and shading detail as close to Image 1 as possible. Exactly eight poses in an exact equal-size 4x2 grid. Exactly one complete character per cell. No #00FF00 anywhere inside the character or props.
Avoid: any particles or stars; any extra objects; extra characters; missing or duplicated poses; shadows; contact shadows; floor plane; scenery; glow; gradients; lighting effects; motion effects; text; labels; logos; watermarks; borders; dividers; transparency; green spill; straw hat; teal scarf; cropping; overlap across cells.
```

Defect fixed: Removed all floating yellow star particles from bottom-row pose 3 while preserving the raised-fist celebration and held rolled map.

## patch-adventure-sheet-v5-candidate-r2.png

Built-in image generation mode: identity-preserve edit

Exact image prompt:

```text
Use case: identity-preserve
Asset type: production chroma-green sprite sheet for the Patch LCD mascot project
Input images: Image 1 is the primary edit target and exact anchor for canvas, 4x2 layout, all eight poses, palette, style, line weight, proportions, and character identity. Images 2 and 3 are additional identity/style references only; do not reproduce their black/transparent presentation or add their effects.
Primary request: Reproduce Image 1 as closely and faithfully as possible, changing exactly two defects and nothing else.

EXACT TWO EDITS:
1. Top row, cell 4: Patch kneeling at an open treasure chest. Repaint the ENTIRE open chest interior, every inner cavity, and the full inner face of the raised lid in opaque warm brown wood tones, with a small amber-gold coin glint. Absolutely no green, lime, chartreuse, teal-green, green reflection, green tint, green glow, or background-colored holes anywhere on, within, behind-visible-through, or inside the chest. The chest must remain fully opaque, solid, readable wood with dark brown outlines and warm amber-gold metal/coin accents.
2. Top row, cell 3: Redraw Patch as a clearly self-supporting seated pose with no ground plane. Use a compact cross-legged seated pose, with both legs naturally folded and the body visibly resting on the crossed legs; hands may rest naturally beside him or on his knees. It must not look hovering, floating, or like dangling unsupported legs. Preserve the same happy expression, outfit, hat, identity, scale, and cell placement.

SHEET INVARIANTS — MANDATORY:
- Exact equal-size 4 columns x 2 rows grid, exactly eight pose cells, exactly one complete Patch character per cell.
- Preserve the other six poses from Image 1 as closely as possible: top 1 flag-holding, top 2 binocular/lookout gesture, bottom 1 running, bottom 2 winking hat-tip, bottom 3 coin juggling, bottom 4 happy cross-legged seated/meditating.
- Preserve Patch's exact identity and design: chibi boy, warm tan skin, large brown eyes, tousled dark brown hair, woven straw hat with rust-red band, teal scarf, navy long coat with gold trim, cream shirt, rust-red shorts, brown belt and belt pouch, brown boots.
- Match the references exactly in warm polished cel-shaded game-sprite illustration, crisp dark-brown line art, consistent line weight, compact chibi proportions, texture detail, and warm shading.
- Generous empty padding on every side inside every cell. No body part, prop, flag, chest, or coin may touch or cross any cell boundary or canvas edge.
- Entire canvas and every gap must be one perfectly flat, uniform, exact chroma green #00FF00. No background variation at all: no gradient, glow, texture, lighting change, vignette, shadow, seam, panel, border, or floor.
- Do not use exact #00FF00 anywhere in Patch, clothing, skin, hair, props, flag, coins, chest, outlines, or antialiased subject edges.
- No shadows, contact shadows, ground plane, scenery, glow, aura, gradients/effects outside normal opaque cel shading on the character art, text, labels, logos, watermarks, borders, grid lines, or dividers.
- Crisp transparent-ready edges. All characters and props fully opaque and visually separated from the chroma background.
- Keep the original landscape canvas composition and pose ordering. Do not crop, omit, duplicate, merge, or add poses.

Change only the two specified defects. Preserve everything else from Image 1 with maximum fidelity.
```

Defect fixed: Repainted the top-row pose 4 chest interior and inner lid in opaque warm brown wood tones with amber-gold coin glints, removing the green tint/glow.

Defect fixed: Redrew top-row pose 3 as a compact, clearly self-supporting cross-legged seated pose with no ground plane.

## Patch micro-idles v4 candidate r3

Output file:

```text
art_sprint/patch-micro-idles-v4-candidate-r3.png
```

Prompt:

```text
Use case: identity-preserve
Asset type: Patch LCD mascot chroma-green sprite sheet final revision
Primary request: Edit Image 1 conservatively. Image 1 already contains the correct eight poses and all three requested defect fixes. Preserve every character, prop, pose, edge, scale, position, costume detail, facial expression, line, and cel-shaded color exactly. Change only the green backdrop into one perfectly flat, numerically uniform digital fill of RGB (0,255,0), hexadecimal #00FF00, across the entire canvas and all gaps.
Input images: Image 1 is the corrected edit target and authoritative layout. Image 2 is the original sheet for exact 4x2 grid and pose-order reference. Images 3 and 4 are identity/style references only.
Scene/backdrop: a single solid digital color field #00FF00, RGB 0 255 0. Every background pixel must be exactly identical. No lighting variation, noise, texture, vignette, gradient, shading, glow, shadow, floor, scenery, or compression artifacts.
Subject: exactly one complete chibi boy Patch per cell, wearing straw hat with red band, teal scarf, navy coat with gold trim, cream shirt, rust-red shorts, belt pouch, and brown boots.
Composition/framing: exact equal-size 4x2 grid and exact pose order from Image 1. Generous empty padding on all sides. Nothing touches or crosses a cell boundary.
Required fixed states to preserve:
1. Top row pose 2 fishing has the rod only and no detached fishing line.
2. Bottom row pose 3 lying on stomach has both raised boots visibly connected through bent legs to the hips.
3. Bottom row pose 4 sneeze has empty hands, eyes squeezed shut, teal scarf flying upward, and no props or particles.
Constraints: exactly eight poses; one complete Patch per cell; do not alter the characters; no #00FF00 in any character or prop; crisp transparent-ready character edges.
Avoid: any change beyond the background fill; nonuniform green; gradients; shadows; floor plane; scenery; glow; effects; text; labels; logos; watermark; borders; particles; new props; changed identity; changed costume; changed poses; changed crop.
```

Defect fixed 1: Removed the detached straight white fishing line from top-row pose 2, leaving only the rod within its cell.

Defect fixed 2: Redrew bottom-row pose 3 with continuous bent legs connecting the hips to both raised boots.

Defect fixed 3: Removed the white fist object from bottom-row pose 4; both hands are empty in the eyes-closed sneeze, with the teal scarf flying upward and no particles.

## navigator-core-sheet-v1-r3.png

Built-in image generation mode: conservative background-uniformity edit

Exact image prompt:

```text
Use case: background-extraction
Asset type: Patch LCD mascot chroma-green sprite sheet
Input images: Image 1: authoritative edit target and sole source of layout/content; Image 2: identity reference only, do not edit it and do not copy its pose or composition into Image 1.
Primary request: Edit Image 1 conservatively. Change ONLY its existing green backdrop into one perfectly flat, numerically uniform digital fill of RGB (0,255,0), hexadecimal #00FF00, across the entire canvas and every gap between all eight sprite cells.
Subject and composition invariants: Preserve all eight characters exactly as they appear in Image 1: every pose, prop, edge, scale, position, costume detail, facial expression, line, and cel-shaded color. Preserve the exact canvas dimensions and the exact 4-by-2 layout. The bottom-row celebration pose has no star particles; keep it that way and add nothing.
Backdrop constraints: Every background pixel must be the identical solid color #00FF00. No lighting variation, noise, speckle, texture, vignette, gradient, glow, shadow, floor plane, reflection, transparency, or compression artifacts.
Edge constraints: Keep crisp, transparent-ready character and prop edges with no dark speckle, fringe, halo, erosion, expansion, redraw, or restyling. Do not use #00FF00 anywhere inside any character or prop.
Avoid: any change whatsoever outside the green backdrop; no new objects, particles, text, watermark, or decoration.
```

Background-uniformity pass: Replaced the noisy chroma-green backdrop with a flat #00FF00 field while preserving the eight-pose sheet and keeping the bottom-row celebration pose free of star particles.
