# Echocord brand assets

The Echo trail mark represents an original sound followed by two equal-sized echoes. It uses coral #FF4B3E, warm paper #EBE5D5 and grey #939CA2 on slate #1D2226, matching the desktop interface. It does not use Discord's brand assets.

- `ui/public/brand/echocord-mark.png`: generated raster logo, used in the README.
- `ui/public/brand/echocord.svg`: precise64-unit tile master.
- `ui/public/brand/echocord-symbol.svg`: transparent symbol for the window title bar.
- `scripts/build-app-icon.mjs`: dependency-free Windows ICO export from the same geometry at16,20,24,32,40,48,64,128 and256pixels.

The logo direction was reviewed with Claude Opus. The raster was produced with the built-in image-generation tool; deterministic vector/icon masters keep small desktop icons crisp. The project license covers these original assets. Keep the mark's three shapes and spacing intact; it should remain static and must not impersonate the live audio status indicator.

## Generation brief

Original minimal Echo trail app icon on a transparent canvas: rounded slate tile, a solid coral disc, then two same-height right-hand crescent echoes in warm off-white and cool grey. On a64-unit master, the disc center is(19,32), radius14; the echo circles are centered at(32,32) and(45,32), radius14, each subtracting the preceding center's radius18circle. Tile corner radius14. Flat fills, balanced margins, crisp geometric edges and strong16pixel legibility. No text, microphone/headphones, Discord logo, game imagery, gradients, glow, shadows, texture, bevel or3D mockup.
