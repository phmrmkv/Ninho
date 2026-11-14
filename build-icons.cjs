// Renders assets/ninho.svg to the PNGs and the .ico Ninho uses (window icon, exe icon). Run from any folder that has
// the sharp package: node build-icons.cjs <assets folder>
const sharp = require(require.resolve('sharp', { paths: [process.cwd()] })), fs = require('fs'), path = require('path');
(async () => {
  const dir = process.argv[2], svg = fs.readFileSync(path.join(dir, 'ninho.svg'));
  const sizes = [16, 24, 32, 48, 64, 128, 256], pngs = [];
  for (const s of sizes) {
    const b = await sharp(svg, { density: 384 }).resize(s, s).png().toBuffer();
    pngs.push(b);
    if (s === 256 || s === 64) fs.writeFileSync(path.join(dir, `ninho-${s}.png`), b);
  }
  // .ico holding PNG images (Windows Vista and later read these).
  const head = Buffer.alloc(6); head.writeUInt16LE(0, 0); head.writeUInt16LE(1, 2); head.writeUInt16LE(sizes.length, 4);
  let off = 6 + 16 * sizes.length; const dirs = [];
  for (const [i, s] of sizes.entries()) {
    const e = Buffer.alloc(16); e[0] = s >= 256 ? 0 : s; e[1] = s >= 256 ? 0 : s; e.writeUInt16LE(1, 4); e.writeUInt16LE(32, 6);
    e.writeUInt32LE(pngs[i].length, 8); e.writeUInt32LE(off, 12); off += pngs[i].length; dirs.push(e);
  }
  fs.writeFileSync(path.join(dir, 'ninho.ico'), Buffer.concat([head, ...dirs, ...pngs]));
  console.log('ok');
})();

// assets/ninho.res: the same images as a Windows resource (RT_ICON + RT_GROUP_ICON id 1), so build.rs only has to
// convert it with `windres -J res` (no C preprocessor needed).
(async () => {
  await new Promise(r => setTimeout(r, 500));
  const dir = process.argv[2], ico = fs.readFileSync(path.join(dir, 'ninho.ico'));
  const n = ico.readUInt16LE(4), imgs = [];
  for (let i = 0; i < n; i++) {
    const e = 6 + 16 * i, size = ico.readUInt32LE(e + 8), off = ico.readUInt32LE(e + 12);
    imgs.push({ w: ico[e], h: ico[e + 1], data: ico.subarray(off, off + size) });
  }
  const pad = b => b.length % 4 ? Buffer.concat([b, Buffer.alloc(4 - b.length % 4)]) : b;
  const entry = (type, name, data) => {
    const h = Buffer.alloc(32);
    h.writeUInt32LE(data.length, 0); h.writeUInt32LE(32, 4);
    h.writeUInt16LE(0xffff, 8); h.writeUInt16LE(type, 10); h.writeUInt16LE(0xffff, 12); h.writeUInt16LE(name, 14);
    h.writeUInt32LE(0, 16); h.writeUInt16LE(0x1010, 20); h.writeUInt16LE(0x0409, 22);
    return Buffer.concat([h, pad(data)]);
  };
  const parts = [Buffer.from([0, 0, 0, 0, 0x20, 0, 0, 0, 0xff, 0xff, 0, 0, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])];
  const grp = Buffer.alloc(6 + 14 * n); grp.writeUInt16LE(1, 2); grp.writeUInt16LE(n, 4);
  imgs.forEach((im, i) => {
    parts.push(entry(3, i + 1, im.data));
    const g = 6 + 14 * i; grp[g] = im.w; grp[g + 1] = im.h; grp.writeUInt16LE(1, g + 4); grp.writeUInt16LE(32, g + 6);
    grp.writeUInt32LE(im.data.length, g + 8); grp.writeUInt16LE(i + 1, g + 12);
  });
  parts.push(entry(14, 1, grp));
  fs.writeFileSync(path.join(dir, 'ninho.res'), Buffer.concat(parts));
  console.log('res ok');
})();
