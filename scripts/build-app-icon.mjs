import { mkdir, writeFile } from "node:fs/promises";

// The Echo trail master uses the same geometry as public/brand/echocord.svg.
// Compile directly at every Windows icon size to preserve small-size edges.
// Supersampling and the ICO encoder use only Node's standard library.
const output = new URL("../ui/dist/", import.meta.url);
await mkdir(output, { recursive: true });
const sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256];
const samples = 4;
const disc = (x, y, cx, radius) => (x - cx) ** 2 + (y - 32) ** 2 <= radius ** 2;
function pixel(x, y) {
  const dx = Math.max(14 - x, 0, x - 50);
  const dy = Math.max(14 - y, 0, y - 50);
  if (dx * dx + dy * dy > 14 * 14) return null;
  if (disc(x, y, 45, 14) && !disc(x, y, 32, 18)) return [0x93, 0x9c, 0xa2];
  if (disc(x, y, 32, 14) && !disc(x, y, 19, 18)) return [0xeb, 0xe5, 0xd5];
  if (disc(x, y, 19, 14)) return [0xff, 0x4b, 0x3e];
  return [0x1d, 0x22, 0x26];
}
const frames = sizes.map((size) => {
  const maskStride = Math.ceil(size / 32) * 4;
  const frame = Buffer.alloc(40 + size * size * 4 + size * maskStride);
  frame.writeUInt32LE(40, 0);
  frame.writeInt32LE(size, 4);
  frame.writeInt32LE(size * 2, 8);
  frame.writeUInt16LE(1, 12);
  frame.writeUInt16LE(32, 14);
  frame.writeUInt32LE(size * size * 4, 20);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const rgb = [0, 0, 0];
      let covered = 0;
      for (let sy = 0; sy < samples; sy++) {
        for (let sx = 0; sx < samples; sx++) {
          const colour = pixel((x + (sx + 0.5) / samples) * 64 / size,
            (y + (sy + 0.5) / samples) * 64 / size);
          if (colour) {
            covered++;
            for (let channel = 0; channel < 3; channel++) rgb[channel] += colour[channel];
          }
        }
      }
      const row = size - 1 - y;
      const offset = 40 + (row * size + x) * 4;
      if (covered) {
        frame[offset] = Math.round(rgb[2] / covered);
        frame[offset + 1] = Math.round(rgb[1] / covered);
        frame[offset + 2] = Math.round(rgb[0] / covered);
        frame[offset + 3] = Math.round(covered * 255 / (samples * samples));
      } else {
        frame[40 + size * size * 4 + row * maskStride + (x >> 3)] |= 0x80 >> (x & 7);
      }
    }
  }
  return frame;
});
const header = Buffer.alloc(6 + sizes.length * 16);
header.writeUInt16LE(1, 2);
header.writeUInt16LE(sizes.length, 4);
let offset = header.length;
for (let index = 0; index < sizes.length; index++) {
  const entry = 6 + index * 16;
  header[entry] = sizes[index] === 256 ? 0 : sizes[index];
  header[entry + 1] = header[entry];
  header.writeUInt16LE(1, entry + 4);
  header.writeUInt16LE(32, entry + 6);
  header.writeUInt32LE(frames[index].length, entry + 8);
  header.writeUInt32LE(offset, entry + 12);
  offset += frames[index].length;
}
await writeFile(new URL("echocord.ico", output), Buffer.concat([header, ...frames]));
