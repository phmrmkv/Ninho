//! Small QR Code encoder (ISO/IEC 18004), byte mode, versions 1-6,
//! error correction level M (falls back to L when the text does not fit M).
//! Meant for short URLs (up to ~130 bytes at level L, ~106 at level M).

use eframe::egui::{Color32, CornerRadius, Painter, Rect, vec2};

/// Square matrix of modules (true = dark), without the quiet zone.
pub struct Qr {
    pub size: usize,
    pub modules: Vec<bool>,
}

impl Qr {
    pub fn get(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.modules[y * self.size + x]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Ecc {
    L,
    M,
}

impl Ecc {
    /// The two EC indicator bits used in the format information.
    fn format_bits(self) -> u32 {
        match self {
            Ecc::L => 0b01,
            Ecc::M => 0b00,
        }
    }
}

const MAX_VERSION: usize = 6;

/// (total codewords, EC codewords per block, number of blocks)
fn params(version: usize, ecc: Ecc) -> (usize, usize, usize) {
    const TOTAL: [usize; MAX_VERSION] = [26, 44, 70, 100, 134, 172];
    const EC_M: [(usize, usize); MAX_VERSION] = [(10, 1), (16, 1), (26, 1), (18, 2), (24, 2), (16, 4)];
    const EC_L: [(usize, usize); MAX_VERSION] = [(7, 1), (10, 1), (15, 1), (20, 1), (26, 1), (18, 2)];
    let i = version - 1;
    let (ec, blocks) = match ecc {
        Ecc::L => EC_L[i],
        Ecc::M => EC_M[i],
    };
    (TOTAL[i], ec, blocks)
}

fn data_capacity(version: usize, ecc: Ecc) -> usize {
    let (total, ec, blocks) = params(version, ecc);
    total - ec * blocks
}

// ---------------------------------------------------------------- GF(256)

/// Multiplication in GF(2^8) with the QR polynomial x^8+x^4+x^3+x^2+1 (0x11D).
fn gf_mul(x: u8, y: u8) -> u8 {
    let mut z: u32 = 0;
    for i in (0..8).rev() {
        z = (z << 1) ^ ((z >> 7) * 0x11D);
        z ^= ((y as u32 >> i) & 1) * x as u32;
    }
    z as u8
}

/// Generator polynomial of the given degree (coefficients from the highest
/// power down, without the leading 1): prod (x - a^i), i = 0..degree.
fn rs_generator(degree: usize) -> Vec<u8> {
    let mut result = vec![0u8; degree];
    result[degree - 1] = 1;
    let mut root = 1u8;
    for _ in 0..degree {
        for j in 0..degree {
            result[j] = gf_mul(result[j], root);
            if j + 1 < degree {
                result[j] ^= result[j + 1];
            }
        }
        root = gf_mul(root, 2);
    }
    result
}

/// Remainder of data(x) * x^degree divided by the generator = EC codewords.
fn rs_remainder(data: &[u8], generator: &[u8]) -> Vec<u8> {
    let mut result = vec![0u8; generator.len()];
    for &b in data {
        let factor = b ^ result.remove(0);
        result.push(0);
        for (r, &g) in result.iter_mut().zip(generator) {
            *r ^= gf_mul(g, factor);
        }
    }
    result
}

// ---------------------------------------------------------------- data

fn push_bits(bits: &mut Vec<bool>, value: u32, len: usize) {
    for i in (0..len).rev() {
        bits.push((value >> i) & 1 == 1);
    }
}

/// Byte-mode bit stream padded to exactly `capacity` data codewords.
fn data_codewords(bytes: &[u8], capacity: usize) -> Option<Vec<u8>> {
    let mut bits: Vec<bool> = Vec::new();
    push_bits(&mut bits, 0b0100, 4);
    push_bits(&mut bits, bytes.len() as u32, 8); // 8-bit count for versions 1-9
    if bytes.len() > 255 {
        return None;
    }
    for &b in bytes {
        push_bits(&mut bits, b as u32, 8);
    }
    let cap_bits = capacity * 8;
    if bits.len() > cap_bits {
        return None;
    }
    let term = (cap_bits - bits.len()).min(4);
    push_bits(&mut bits, 0, term);
    while bits.len() % 8 != 0 {
        bits.push(false);
    }
    let mut out: Vec<u8> = bits
        .chunks(8)
        .map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | b as u8))
        .collect();
    let mut pad = 0xEC;
    while out.len() < capacity {
        out.push(pad);
        pad = if pad == 0xEC { 0x11 } else { 0xEC };
    }
    Some(out)
}

/// Split into blocks, append RS codewords and interleave.
fn add_ec_and_interleave(data: &[u8], version: usize, ecc: Ecc) -> Vec<u8> {
    let (total, ec, blocks) = params(version, ecc);
    let num_short = blocks - total % blocks;
    let short_data = total / blocks - ec;
    let generator = rs_generator(ec);
    let mut data_blocks: Vec<&[u8]> = Vec::new();
    let mut ec_blocks: Vec<Vec<u8>> = Vec::new();
    let mut pos = 0;
    for i in 0..blocks {
        let len = short_data + usize::from(i >= num_short);
        let chunk = &data[pos..pos + len];
        pos += len;
        ec_blocks.push(rs_remainder(chunk, &generator));
        data_blocks.push(chunk);
    }
    let mut out = Vec::with_capacity(total);
    for k in 0..=short_data {
        for b in &data_blocks {
            if k < b.len() {
                out.push(b[k]);
            }
        }
    }
    for k in 0..ec {
        for b in &ec_blocks {
            out.push(b[k]);
        }
    }
    debug_assert_eq!(out.len(), total);
    out
}

// ---------------------------------------------------------------- matrix

struct Matrix {
    size: usize,
    modules: Vec<bool>,
    is_fn: Vec<bool>,
}

impl Matrix {
    fn new(version: usize) -> Self {
        let size = 17 + 4 * version;
        let mut m = Matrix { size, modules: vec![false; size * size], is_fn: vec![false; size * size] };
        m.draw_function_patterns(version);
        m
    }

    fn set_fn(&mut self, x: i32, y: i32, dark: bool) {
        if x >= 0 && y >= 0 && (x as usize) < self.size && (y as usize) < self.size {
            let i = y as usize * self.size + x as usize;
            self.modules[i] = dark;
            self.is_fn[i] = true;
        }
    }

    fn draw_finder(&mut self, cx: i32, cy: i32) {
        for dy in -4i32..=4 {
            for dx in -4i32..=4 {
                let dist = dx.abs().max(dy.abs());
                self.set_fn(cx + dx, cy + dy, dist != 2 && dist != 4);
            }
        }
    }

    fn draw_alignment(&mut self, cx: i32, cy: i32) {
        for dy in -2i32..=2 {
            for dx in -2i32..=2 {
                let dist = dx.abs().max(dy.abs());
                self.set_fn(cx + dx, cy + dy, dist != 1);
            }
        }
    }

    fn draw_function_patterns(&mut self, version: usize) {
        let n = self.size as i32;
        for i in 0..n {
            self.set_fn(6, i, i % 2 == 0);
            self.set_fn(i, 6, i % 2 == 0);
        }
        self.draw_finder(3, 3);
        self.draw_finder(n - 4, 3);
        self.draw_finder(3, n - 4);
        if version >= 2 {
            // Versions 2-6 have exactly two alignment coordinates: 6 and size-7.
            let pos = [6, n - 7];
            for (i, &px) in pos.iter().enumerate() {
                for (j, &py) in pos.iter().enumerate() {
                    if (i == 0 && j == 0) || (i == 0 && j == 1) || (i == 1 && j == 0) {
                        continue; // overlaps a finder pattern
                    }
                    self.draw_alignment(px, py);
                }
            }
        }
        self.draw_format(Ecc::M, 0); // reserve the area; redrawn later
    }

    fn draw_format(&mut self, ecc: Ecc, mask: usize) {
        let bits = format_word(ecc, mask);
        let n = self.size as i32;
        let bit = |i: u32| (bits >> i) & 1 == 1;
        for i in 0..=5 {
            self.set_fn(8, i as i32, bit(i));
        }
        self.set_fn(8, 7, bit(6));
        self.set_fn(8, 8, bit(7));
        self.set_fn(7, 8, bit(8));
        for i in 9..15 {
            self.set_fn(14 - i as i32, 8, bit(i));
        }
        for i in 0..8 {
            self.set_fn(n - 1 - i as i32, 8, bit(i));
        }
        for i in 8..15 {
            self.set_fn(8, n - 15 + i as i32, bit(i));
        }
        self.set_fn(8, n - 8, true); // dark module
    }

    fn draw_codewords(&mut self, codewords: &[u8]) {
        let n = self.size as i32;
        let total_bits = codewords.len() * 8;
        let mut i = 0usize;
        let mut right = n - 1;
        while right >= 1 {
            if right == 6 {
                right = 5;
            }
            for vert in 0..n {
                for j in 0..2 {
                    let x = right - j;
                    let upward = ((right + 1) & 2) == 0;
                    let y = if upward { n - 1 - vert } else { vert };
                    let idx = y as usize * self.size + x as usize;
                    if !self.is_fn[idx] && i < total_bits {
                        self.modules[idx] = (codewords[i >> 3] >> (7 - (i & 7))) & 1 == 1;
                        i += 1;
                    }
                }
            }
            right -= 2;
        }
        debug_assert_eq!(i, total_bits);
    }

    fn apply_mask(&mut self, mask: usize) {
        for y in 0..self.size {
            for x in 0..self.size {
                let idx = y * self.size + x;
                if !self.is_fn[idx] && mask_bit(mask, x, y) {
                    self.modules[idx] = !self.modules[idx];
                }
            }
        }
    }

    fn penalty(&self) -> u32 {
        let n = self.size;
        let m = &self.modules;
        let mut p = 0u32;
        const P1: [bool; 11] = [true, false, true, true, true, false, true, false, false, false, false];
        const P2: [bool; 11] = [false, false, false, false, true, false, true, true, true, false, true];
        for pass in 0..2 {
            for a in 0..n {
                let line: Vec<bool> =
                    (0..n).map(|b| if pass == 0 { m[a * n + b] } else { m[b * n + a] }).collect();
                let mut run = 1;
                for i in 1..n {
                    if line[i] == line[i - 1] {
                        run += 1;
                        if run == 5 {
                            p += 3;
                        } else if run > 5 {
                            p += 1;
                        }
                    } else {
                        run = 1;
                    }
                }
                for i in 0..n.saturating_sub(10) {
                    let w = &line[i..i + 11];
                    if w == P1 || w == P2 {
                        p += 40;
                    }
                }
            }
        }
        for y in 0..n - 1 {
            for x in 0..n - 1 {
                let c = m[y * n + x];
                if c == m[y * n + x + 1] && c == m[(y + 1) * n + x] && c == m[(y + 1) * n + x + 1] {
                    p += 3;
                }
            }
        }
        let dark = m.iter().filter(|&&b| b).count();
        let total = n * n;
        let k = (dark * 20).abs_diff(total * 10) / total;
        p += k as u32 * 10;
        p
    }
}

fn mask_bit(mask: usize, x: usize, y: usize) -> bool {
    match mask {
        0 => (x + y) % 2 == 0,
        1 => y % 2 == 0,
        2 => x % 3 == 0,
        3 => (x + y) % 3 == 0,
        4 => (x / 3 + y / 2) % 2 == 0,
        5 => x * y % 2 + x * y % 3 == 0,
        6 => (x * y % 2 + x * y % 3) % 2 == 0,
        _ => ((x + y) % 2 + x * y % 3) % 2 == 0,
    }
}

/// 15-bit format information: BCH(15,5) of (ecc << 3 | mask), XOR 0x5412.
fn format_word(ecc: Ecc, mask: usize) -> u32 {
    let data = (ecc.format_bits() << 3) | mask as u32;
    let mut rem = data;
    for _ in 0..10 {
        rem = (rem << 1) ^ ((rem >> 9) * 0x537);
    }
    ((data << 10) | rem) ^ 0x5412
}

/// Builds the final matrix from the *data* codewords (EC is added here).
/// `mask = None` picks the lowest-penalty mask. Returns the chosen mask too.
fn build(version: usize, ecc: Ecc, data: &[u8], mask: Option<usize>) -> (Qr, usize) {
    let all = add_ec_and_interleave(data, version, ecc);
    let mut m = Matrix::new(version);
    m.draw_codewords(&all);
    let chosen = match mask {
        Some(k) => k,
        None => {
            let mut best = (u32::MAX, 0);
            for k in 0..8 {
                m.apply_mask(k);
                m.draw_format(ecc, k);
                let p = m.penalty();
                m.apply_mask(k); // XOR again = undo
                if p < best.0 {
                    best = (p, k);
                }
            }
            best.1
        }
    };
    m.apply_mask(chosen);
    m.draw_format(ecc, chosen);
    (Qr { size: m.size, modules: m.modules }, chosen)
}

/// Encode `text` as a QR code (byte mode). Level M when it fits in versions
/// 1-6, otherwise level L; `None` if it is too long even for that.
pub fn encode(text: &str) -> Option<Qr> {
    let bytes = text.as_bytes();
    for ecc in [Ecc::M, Ecc::L] {
        for version in 1..=MAX_VERSION {
            if let Some(data) = data_codewords(bytes, data_capacity(version, ecc)) {
                return Some(build(version, ecc, &data, None).0);
            }
        }
    }
    None
}

/// Draw with an egui painter: white square (centered in `rect`) with a
/// 4-module quiet zone around the code, black modules.
pub fn paint(qr: &Qr, painter: &Painter, rect: Rect) {
    let total = (qr.size + 8) as f32;
    let side = rect.width().min(rect.height());
    let cell = side / total;
    let origin = rect.center() - vec2(side, side) * 0.5;
    painter.rect_filled(Rect::from_min_size(origin, vec2(side, side)), CornerRadius::ZERO, Color32::WHITE);
    for y in 0..qr.size {
        for x in 0..qr.size {
            if qr.get(x, y) {
                let min = origin + vec2((x + 4) as f32 * cell, (y + 4) as f32 * cell);
                painter.rect_filled(Rect::from_min_size(min, vec2(cell, cell)), CornerRadius::ZERO, Color32::BLACK);
            }
        }
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rs_generator_degree_7() {
        // x^7 + 127x^6 + 122x^5 + 154x^4 + 164x^3 + 11x^2 + 68x + 117
        assert_eq!(rs_generator(7), vec![127, 122, 154, 164, 11, 68, 117]);
    }

    #[test]
    fn rs_known_vector_hello_world_1m() {
        // Classic "HELLO WORLD" 1-M example (alphanumeric mode data codewords).
        let data = [32, 91, 11, 120, 209, 114, 220, 77, 67, 64, 236, 17, 236, 17, 236, 17];
        let ec = rs_remainder(&data, &rs_generator(10));
        assert_eq!(ec, vec![196, 35, 39, 119, 235, 215, 231, 226, 93, 23]);
    }

    #[test]
    fn format_bits_m_mask0() {
        assert_eq!(format_word(Ecc::M, 0), 0b101010000010010);
        // Other well-known values from the spec table.
        assert_eq!(format_word(Ecc::L, 0), 0b111011111000100);
        assert_eq!(format_word(Ecc::M, 7), 0b100101010100000);
    }

    #[test]
    fn hello_world_byte_mode_codewords() {
        let data = data_codewords(b"HELLO WORLD", data_capacity(1, Ecc::M)).unwrap();
        // Worked out by hand: 0100 | 00001011 | 48 45 4C 4C 4F 20 57 4F 52 4C 44 | 0000 | pads
        let expect = [
            0x40, 0xB4, 0x84, 0x54, 0xC4, 0xC4, 0xF2, 0x05, 0x74, 0xF5, 0x24, 0xC4, 0x40, 0xEC, 0x11, 0xEC,
        ];
        assert_eq!(data, expect);
        let (qr, _) = build(1, Ecc::M, &data, Some(0));
        assert_eq!(qr.size, 21);
        check_function_patterns(&qr);
        verify(&qr, b"HELLO WORLD", Some(0));
    }

    fn check_function_patterns(qr: &Qr) {
        let n = qr.size;
        for &(ox, oy) in &[(0, 0), (n - 7, 0), (0, n - 7)] {
            for dy in 0..7 {
                for dx in 0..7 {
                    let ring = dx.max(6 - dx).max(dy).max(6 - dy) as i32; // 3 at border
                    let cdist = (dx as i32 - 3).abs().max((dy as i32 - 3).abs());
                    let _ = ring;
                    assert_eq!(qr.get(ox + dx, oy + dy), cdist != 2, "finder at {ox},{oy}");
                }
            }
        }
        for i in 8..n - 8 {
            assert_eq!(qr.get(i, 6), i % 2 == 0);
            assert_eq!(qr.get(6, i), i % 2 == 0);
        }
        assert!(qr.get(8, n - 8), "dark module");
    }

    // ---- independent verifier (decoder) ----

    fn is_function(version: usize, x: usize, y: usize) -> bool {
        let n = 17 + 4 * version;
        if x < 9 && y < 9 || x >= n - 8 && y < 9 || x < 9 && y >= n - 8 {
            return true;
        }
        if x == 6 || y == 6 {
            return true;
        }
        if version >= 2 {
            let c = n as i32 - 7;
            if (x as i32 - c).abs() <= 2 && (y as i32 - c).abs() <= 2 {
                return true;
            }
        }
        false
    }

    fn read_format(qr: &Qr) -> (Ecc, usize) {
        let n = qr.size;
        let mut a = 0u32;
        let mut b = 0u32;
        // First copy: around the top-left finder.
        let p1: [(usize, usize); 15] = [
            (8, 0), (8, 1), (8, 2), (8, 3), (8, 4), (8, 5), (8, 7), (8, 8),
            (7, 8), (5, 8), (4, 8), (3, 8), (2, 8), (1, 8), (0, 8),
        ];
        for (i, &(x, y)) in p1.iter().enumerate() {
            a |= (qr.get(x, y) as u32) << i;
        }
        // Second copy: bits 0-7 along row 8 at the right edge, 8-14 down column 8.
        for i in 0..8 {
            b |= (qr.get(n - 1 - i, 8) as u32) << i;
        }
        for i in 8..15 {
            b |= (qr.get(8, n - 15 + i) as u32) << i;
        }
        assert_eq!(a, b, "format copies differ");
        for ecc in [Ecc::L, Ecc::M] {
            for mask in 0..8 {
                if format_word(ecc, mask) == a {
                    return (ecc, mask);
                }
            }
        }
        panic!("no valid format word {a:015b}");
    }

    fn verify(qr: &Qr, expect: &[u8], expect_mask: Option<usize>) {
        let n = qr.size;
        let version = (n - 17) / 4;
        assert_eq!(17 + 4 * version, n);
        let (ecc, mask) = read_format(qr);
        if let Some(m) = expect_mask {
            assert_eq!(mask, m);
        }
        assert!(qr.get(8, n - 8), "dark module");
        let (total, ec, blocks) = params(version, ecc);

        // Read bits in zigzag order, unmasking.
        let mut bits: Vec<bool> = Vec::new();
        let mut col = n as i32 - 1;
        let mut up = true;
        while col > 0 {
            if col == 6 {
                col -= 1;
            }
            for step in 0..n {
                let y = if up { n - 1 - step } else { step };
                for dx in 0..2 {
                    let x = (col - dx) as usize;
                    if !is_function(version, x, y) {
                        bits.push(qr.get(x, y) ^ mask_bit(mask, x, y));
                    }
                }
            }
            up = !up;
            col -= 2;
        }
        assert!(bits.len() >= total * 8);
        let cw: Vec<u8> = bits[..total * 8]
            .chunks(8)
            .map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | b as u8))
            .collect();

        // De-interleave.
        let num_short = blocks - total % blocks;
        let short_data = total / blocks - ec;
        let lens: Vec<usize> = (0..blocks).map(|i| short_data + usize::from(i >= num_short)).collect();
        let mut blk: Vec<Vec<u8>> = vec![Vec::new(); blocks];
        let mut it = cw.iter();
        for k in 0..=short_data {
            for i in 0..blocks {
                if k < lens[i] {
                    blk[i].push(*it.next().unwrap());
                }
            }
        }
        for _ in 0..ec {
            for i in 0..blocks {
                blk[i].push(*it.next().unwrap());
            }
        }
        assert!(it.next().is_none());

        // Syndromes S_j = block(alpha^j) must all be zero.
        for (bi, b) in blk.iter().enumerate() {
            let mut alpha_j = 1u8;
            for j in 0..ec {
                let mut s = 0u8;
                for &c in b {
                    s = gf_mul(s, alpha_j) ^ c;
                }
                assert_eq!(s, 0, "syndrome {j} of block {bi} nonzero (v{version})");
                alpha_j = gf_mul(alpha_j, 2);
            }
        }

        // Parse payload: mode 0100, 8-bit count, bytes.
        let data: Vec<u8> = blk.iter().zip(&lens).flat_map(|(b, &l)| b[..l].to_vec()).collect();
        let mut pb: Vec<bool> = Vec::new();
        for &d in &data {
            for i in (0..8).rev() {
                pb.push((d >> i) & 1 == 1);
            }
        }
        let rd = |s: usize, l: usize| pb[s..s + l].iter().fold(0usize, |a, &b| (a << 1) | b as usize);
        assert_eq!(rd(0, 4), 0b0100, "mode");
        let count = rd(4, 8);
        let payload: Vec<u8> = (0..count).map(|i| rd(12 + 8 * i, 8) as u8).collect();
        assert_eq!(payload, expect);
        // Terminator then pad bytes.
        let end = 12 + 8 * count;
        assert!(end <= pb.len());
    }

    #[test]
    fn roundtrip_urls_and_versions() {
        let urls = [
            "https://quiet-river-1234.trycloudflare.com",
            "http://a.b",
            "https://example.com/ab",
            "https://abcdefghijklmnopqrstuvwxyz-0123456789.trycloudflare.com/x",
            "",
            "x",
            "https://example.com/path?query=1&other=%C3%A9",
            "https://very-long-subdomain-name-for-testing.trycloudflare.com/some/long/path/segment/index.html",
            "0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567",
        ];
        let mut seen = [false; MAX_VERSION + 1];
        for u in urls {
            let qr = encode(u).expect("fits");
            verify(&qr, u.as_bytes(), None);
            seen[(qr.size - 17) / 4] = true;
        }
        assert!(seen[1] && seen[2] && seen[3] && seen[4] && seen[5], "versions covered: {seen:?}");
    }

    #[test]
    fn every_version_ecc_and_mask() {
        for ecc in [Ecc::M, Ecc::L] {
            for version in 1..=MAX_VERSION {
                let cap = data_capacity(version, ecc);
                for len in [0, 1, cap - 2 - 1, cap - 2] {
                    let text: Vec<u8> = (0..len).map(|i| (i * 7 + 13) as u8).collect();
                    let data = data_codewords(&text, cap).expect("fits");
                    for mask in 0..8 {
                        let (qr, _) = build(version, ecc, &data, Some(mask));
                        check_function_patterns(&qr);
                        verify(&qr, &text, Some(mask));
                    }
                    let (qr, _) = build(version, ecc, &data, None);
                    verify(&qr, &text, None);
                }
                assert!(data_codewords(&vec![0u8; cap - 1], cap).is_none());
            }
        }
    }

    #[test]
    fn too_long_is_none() {
        assert!(encode(&"a".repeat(200)).is_none());
        assert!(encode(&"a".repeat(132)).is_some()); // V6-L: 136 data cw - 2 header
        assert!(encode(&"a".repeat(135)).is_none());
    }
}
