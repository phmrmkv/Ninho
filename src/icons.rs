//! Small vector icons drawn with the painter (sharp at any screen scale): one per kind of server, plus the tools.
use eframe::egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

fn c(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Accent color of a kind (also used for its tile).
pub fn color(kind: &str) -> Color32 {
    match kind {
        "apache" => c(0xd2_4b_3c),
        "nginx" => c(0x2e_9e_5b),
        "mysql" => c(0x2f_6f_b5),
        "postgres" => c(0x33_67_91),
        "mariadb" => c(0x8a_5a_3c),
        "redis" => c(0xc9_3a_32),
        "memcached" => c(0x4c_9a_6a),
        "php" => c(0x6f_6a_c8),
        "mailpit" => c(0xe0_8a_2c),
        "nodejs" => c(0x5f_a0_3c),
        _ => c(0x5c_6b_73),
    }
}

/// A rounded tile in the kind's color with a white symbol inside.
pub fn draw(p: &Painter, r: Rect, kind: &str) {
    let k = color(kind);
    p.rect_filled(r, CornerRadius::same((r.width() * 0.22) as u8), k);
    let w = Color32::WHITE;
    let s = r.width();
    let ctr = r.center();
    let line = Stroke::new((s * 0.07).max(1.2), w);
    let at = |x: f32, y: f32| pos2(r.left() + s * x, r.top() + s * y);
    match kind {
        // Web servers: a globe.
        "apache" | "nginx" => {
            let rad = s * 0.30;
            p.circle_stroke(ctr, rad, line);
            p.line_segment([pos2(ctr.x - rad, ctr.y), pos2(ctr.x + rad, ctr.y)], line);
            p.line_segment([pos2(ctr.x, ctr.y - rad), pos2(ctr.x, ctr.y + rad)], line);
            let e = Shape::ellipse_stroke(ctr, vec2(rad * 0.45, rad), line);
            p.add(e);
        }
        // Database: a cylinder.
        "mysql" | "postgres" | "mariadb" => {
            let (x0, x1, y0, y1) = (0.26, 0.74, 0.28, 0.74);
            let rx = s * (x1 - x0) / 2.0;
            let ry = s * 0.09;
            let cx = r.left() + s * 0.5;
            p.rect_filled(Rect::from_min_max(at(x0, y0), at(x1, y1)), CornerRadius::ZERO, w);
            p.add(Shape::ellipse_filled(pos2(cx, r.top() + s * y1), vec2(rx, ry), w));
            p.add(Shape::ellipse_filled(pos2(cx, r.top() + s * y0), vec2(rx, ry), w));
            p.add(Shape::ellipse_stroke(pos2(cx, r.top() + s * y0), vec2(rx, ry), Stroke::new(s * 0.05, k)));
            for y in [0.44, 0.59] {
                p.add(Shape::ellipse_stroke(pos2(cx, r.top() + s * y), vec2(rx, ry), Stroke::new(s * 0.045, k)));
            }
        }
        // Key-value caches: stacked layers (Redis) / a memory chip (Memcached).
        "redis" => {
            for (i, y) in [0.66, 0.52, 0.38].iter().enumerate() {
                let pts = vec![at(0.5, y - 0.12), at(0.8, *y), at(0.5, y + 0.12), at(0.2, *y)];
                p.add(Shape::convex_polygon(pts, if i == 2 { w } else { w.gamma_multiply(0.75) }, Stroke::new(s * 0.03, k)));
            }
        }
        "memcached" => {
            let chip = Rect::from_min_max(at(0.3, 0.3), at(0.7, 0.7));
            p.rect_filled(chip, CornerRadius::same((s * 0.06) as u8), w);
            for i in 0..3 {
                let t = 0.38 + i as f32 * 0.12;
                for (a, b) in [(at(t, 0.18), at(t, 0.3)), (at(t, 0.7), at(t, 0.82)), (at(0.18, t), at(0.3, t)), (at(0.7, t), at(0.82, t))] {
                    p.line_segment([a, b], Stroke::new(s * 0.05, w));
                }
            }
        }
        // PHP server: code brackets.
        "php" => {
            let st = Stroke::new(s * 0.08, w);
            p.line_segment([at(0.36, 0.32), at(0.2, 0.5)], st);
            p.line_segment([at(0.2, 0.5), at(0.36, 0.68)], st);
            p.line_segment([at(0.64, 0.32), at(0.8, 0.5)], st);
            p.line_segment([at(0.8, 0.5), at(0.64, 0.68)], st);
            p.line_segment([at(0.56, 0.28), at(0.44, 0.72)], st);
        }
        // Mail: an envelope.
        "mailpit" => {
            let env = Rect::from_min_max(at(0.2, 0.3), at(0.8, 0.7));
            p.rect_filled(env, CornerRadius::same((s * 0.05) as u8), w);
            let st = Stroke::new(s * 0.06, k);
            p.line_segment([at(0.22, 0.33), at(0.5, 0.55)], st);
            p.line_segment([at(0.5, 0.55), at(0.78, 0.33)], st);
        }
        // Node.js: a hexagon.
        "nodejs" => {
            let pts: Vec<Pos2> = (0..6)
                .map(|i| {
                    let a = std::f32::consts::FRAC_PI_3 * i as f32 + std::f32::consts::FRAC_PI_6;
                    pos2(ctr.x + a.cos() * s * 0.3, ctr.y + a.sin() * s * 0.3)
                })
                .collect();
            p.add(Shape::convex_polygon(pts, Color32::TRANSPARENT, Stroke::new(s * 0.08, w)));
        }
        _ => {
            p.circle_filled(ctr, s * 0.18, w);
        }
    }
}
