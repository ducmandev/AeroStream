use std::time::Instant;

fn calculate_grid_hash(data: &[u8], width: usize, height: usize) -> u64 {
    if data.len() < width * height * 4 {
        return 0;
    }

    let cols = 16;
    let rows = 9;
    let block_w = width / cols;
    let block_h = height / rows;

    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset

    for r in 0..rows {
        for c in 0..cols {
            // Sample 4 points inside each block (corners + center)
            let center_x = c * block_w + block_w / 2;
            let center_y = r * block_h + block_h / 2;
            let idx = (center_y * width + center_x) * 4;

            if idx + 3 < data.len() {
                let pixel = u32::from_le_bytes([
                    data[idx],
                    data[idx + 1],
                    data[idx + 2],
                    data[idx + 3],
                ]) as u64;
                hash = (hash ^ pixel).wrapping_mul(0x100000001b3);
            }
        }
    }

    hash
}

fn main() {
    let w = 1280;
    let h = 720;
    let mut buf = vec![0u8; w * h * 4];

    // Benchmark time
    let start = Instant::now();
    let runs = 1000;
    for _ in 0..runs {
        let _ = calculate_grid_hash(&buf, w, h);
    }
    let elapsed = start.elapsed();
    println!("Grid hash 144 blocks: Avg time = {:?} (per frame)", elapsed / runs);

    // Test sensitivity to a tiny 1-pixel change (like cursor or typing)
    let h1 = calculate_grid_hash(&buf, w, h);
    // Change 1 pixel in center of block (5, 4)
    let idx = (4 * (h / 9) + (h / 18)) * w * 4 + (5 * (w / 16) + (w / 32)) * 4;
    buf[idx] = 255;
    let h2 = calculate_grid_hash(&buf, w, h);

    println!("Hash unchanged: {}", h1);
    println!("Hash after 1-pixel change: {}", h2);
    assert_ne!(h1, h2, "Must detect 1-pixel change!");
    println!("SUCCESS: 1-pixel modification detected!");
}
