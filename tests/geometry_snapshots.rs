use herdr_witch::sigil::geometry::build;
use herdr_witch::sigil::sprites::load_assets;

fn ascii(n: usize) -> String {
    let runes = load_assets(None).runes;
    let g = build(n, &runes);
    let mut grid = vec![vec!['.'; n]; n];
    let mut put = |x: i32, y: i32, c: char| {
        if x >= 0 && y >= 0 && (x as usize) < n && (y as usize) < n {
            grid[y as usize][x as usize] = c;
        }
    };
    for &(x, y) in &g.halo {
        put(x, y, '+');
    }
    for &(x, y, _) in &g.core {
        put(x, y, '#');
    }
    for &(x, y) in g.runes.iter().flatten() {
        put(x, y, 'r');
    }
    for &(x, y) in &g.eye {
        put(x, y, 'o');
    }
    let mut s = format!("n = {n}\n");
    for row in grid {
        s.extend(row);
        s.push('\n');
    }
    s
}

#[test]
fn halfblock_widths() {
    let out: String = (8..=60).map(|n| ascii(n) + "\n").collect();
    insta::assert_snapshot!(out);
}

#[test]
fn octant_widths() {
    let out: String = (16..=120).step_by(2).map(|n| ascii(n) + "\n").collect();
    insta::assert_snapshot!(out);
}
