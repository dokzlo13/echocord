import { mkdir, writeFile } from "node:fs/promises";

// Small code-native waveform mark, generated with the rest of the build assets.
const output = new URL("../ui/dist/", import.meta.url);
await mkdir(output, { recursive: true });
const size = 32;
const maskRowBytes = 4;
const bitmapBytes = 40 + size * size * 4 + size * maskRowBytes;
const icon = Buffer.alloc(22 + bitmapBytes);
icon.writeUInt16LE(1, 2);
icon.writeUInt16LE(1, 4);
icon[6] = size;
icon[7] = size;
icon.writeUInt16LE(1, 10);
icon.writeUInt16LE(32, 12);
icon.writeUInt32LE(bitmapBytes, 14);
icon.writeUInt32LE(22, 18);
icon.writeUInt32LE(40, 22);
icon.writeInt32LE(size, 26);
icon.writeInt32LE(size * 2, 30);
icon.writeUInt16LE(1, 34);
icon.writeUInt16LE(32, 36);
icon.writeUInt32LE(size * size * 4, 42);
for (let y = 0; y < size; y += 1) {
  for (let x = 0; x < size; x += 1) {
    const heights = [10, 20, 26, 16];
    const column = Math.floor((x - 6) / 5);
    const bar = column >= 0 && column < 4 && (x - 6) % 5 < 3
      && Math.abs(y - 15.5) < heights[column] / 2;
    const color = bar ? [0xd3, 0xcb, 0x72, 0xff] : [0x18, 0x18, 0x18, 0xff];
    icon.set(color, 62 + ((size - y - 1) * size + x) * 4);
  }
}
await writeFile(new URL("echocord.ico", output), icon);
