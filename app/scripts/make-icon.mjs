// Generates app-icon.png (1024x1024) with zero dependencies: raw RGBA -> zlib -> PNG.
// `tauri icon` then derives every .ico/.png size from it.
import { deflateSync } from 'node:zlib';
import { writeFileSync } from 'node:fs';

const S = 1024;
const px = Buffer.alloc(S * S * 4);

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
};

// Rounded-rectangle signed distance (negative inside) for anti-aliased edges.
const rr = (x, y, hw, hh, r) => {
  const qx = Math.abs(x) - hw + r;
  const qy = Math.abs(y) - hh + r;
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r;
};
const cover = (d) => Math.min(Math.max(0.5 - d, 0), 1);
const mix = (a, b, t) => a + (b - a) * t;

for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    const cx = x - S / 2 + 0.5;
    const cy = y - S / 2 + 0.5;
    const a = cover(rr(cx, cy, 470, 470, 200));
    // teal -> indigo diagonal gradient
    const t = (x + y) / (2 * S);
    let r = mix(20, 79, t), g = mix(184, 70, t), b = mix(166, 229, t);
    // three left-aligned bars, shrinking downward: the "cleaned-up" stack
    for (const [by, w] of [[-200, 560], [0, 400], [200, 240]]) {
      const ba = cover(rr(cx - (-280 + w / 2), cy - by, w / 2, 55, 55));
      r = mix(r, 255, ba); g = mix(g, 255, ba); b = mix(b, 255, ba);
    }
    const i = (y * S + x) * 4;
    px[i] = r; px[i + 1] = g; px[i + 2] = b; px[i + 3] = a * 255;
  }
}

const raw = Buffer.alloc((S * 4 + 1) * S);
for (let y = 0; y < S; y++) px.copy(raw, y * (S * 4 + 1) + 1, y * S * 4, (y + 1) * S * 4);
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(S, 0);
ihdr.writeUInt32BE(S, 4);
ihdr[8] = 8; ihdr[9] = 6; // 8-bit RGBA
writeFileSync('app-icon.png', Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', deflateSync(raw, { level: 9 })),
  chunk('IEND', Buffer.alloc(0)),
]));
console.log('wrote app-icon.png');
