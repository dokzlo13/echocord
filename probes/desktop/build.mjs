import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";

const output = new URL("./dist/", import.meta.url);
await mkdir(output, { recursive: true });
for (const name of ["index.html", "probe.js", "style.css"]) {
  const source = new URL(`./frontend/${name}`, import.meta.url);
  if ((await readFile(source)).length === 0) {
    throw new Error(`Empty frontend asset: ${name}`);
  }
  await copyFile(source, new URL(name, output));
}

// Generate a tiny native resource icon in WSL; no image tooling is required.
// ICO header + one 16x16 BGRA bitmap + its all-opaque AND mask.
const size = 16;
const bitmapBytes = 40 + size * size * 4 + size * 4;
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
for (let index = 62; index < 62 + size * size * 4; index += 4) {
  icon.set([0x90, 0x68, 0x24, 0xff], index);
}
await writeFile(new URL("probe.ico", output), icon);
console.log("WSL frontend assets built (HTML, CSS, JS, native resource icon).");
