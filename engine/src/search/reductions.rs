//! Late move reductions: how many plies shallower a late quiet move is
//! searched.
//!
//! With good move ordering the best move is nearly always among the first
//! few, so the moves at the back of the list are searched less deeply. The
//! reduction grows with the depth left and with the move's place in the
//! list, both logarithmically. The table is computed at compile time, with
//! a logarithm made of plain arithmetic, so it is the same on every machine
//! and the bench stays the same everywhere.

/// Depths and move indices beyond the table use its last row and column.
const SIZE: usize = 64;

/// The reduction for depth `d` and move index `i` is
/// `BASE + ln(d) * ln(i) / DIVISOR`, rounded down: a common formula, to be
/// tuned later.
const BASE: f64 = 0.75;
const DIVISOR: f64 = 2.25;

static TABLE: [[i32; SIZE]; SIZE] = table();

/// The reduction, in plies, for a move at `index` (0 for the first move
/// searched) in a node with `depth` plies left.
pub(super) fn reduction(depth: i32, index: usize) -> i32 {
    let depth = (depth.max(0) as usize).min(SIZE - 1);
    TABLE[depth][index.min(SIZE - 1)]
}

const fn table() -> [[i32; SIZE]; SIZE] {
    let mut table = [[0; SIZE]; SIZE];
    let mut depth = 1;
    while depth < SIZE {
        let mut index = 1;
        while index < SIZE {
            let r = BASE + ln(depth as f64) * ln(index as f64) / DIVISOR;
            table[depth][index] = r as i32;
            index += 1;
        }
        depth += 1;
    }
    table
}

/// The natural logarithm of `x >= 1`, from arithmetic alone: `f64::ln` is
/// not available at compile time, and its last bits may differ between
/// platforms.
const fn ln(mut x: f64) -> f64 {
    // x = m * 2^k with 1 <= m < 2, so ln x = k ln 2 + ln m.
    let mut k = 0.0;
    while x >= 2.0 {
        x /= 2.0;
        k += 1.0;
    }
    k * ln_near_one(2.0) + ln_near_one(x)
}

/// ln m for 1 <= m <= 2, by the series ln m = 2 (y + y³/3 + y⁵/5 + ...)
/// with y = (m - 1) / (m + 1) <= 1/3, which converges quickly.
const fn ln_near_one(m: f64) -> f64 {
    let y = (m - 1.0) / (m + 1.0);
    let y2 = y * y;
    let mut power = y;
    let mut sum = 0.0;
    let mut n = 1.0;
    while n < 60.0 {
        sum += power / n;
        power *= y2;
        n += 2.0;
    }
    2.0 * sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logarithm_is_accurate() {
        for x in [1.0, 1.5, 2.0, 3.0, 10.0, 63.0] {
            assert!((ln(x) - f64::ln(x)).abs() < 1e-12, "ln({x})");
        }
    }

    #[test]
    fn reductions_grow_with_depth_and_move_index() {
        // The first two moves and shallow nodes are never reduced by much.
        assert_eq!(reduction(10, 0), 0);
        assert_eq!(reduction(10, 1), 0);
        assert_eq!(reduction(1, 30), 0);
        assert_eq!(reduction(3, 2), 1);
        for depth in 1..SIZE as i32 {
            for index in 1..SIZE {
                assert!(reduction(depth, index) >= reduction(depth - 1, index));
                assert!(reduction(depth, index) >= reduction(depth, index - 1));
            }
        }
        // Beyond the table: its edge.
        assert_eq!(reduction(500, 500), reduction(63, 63));
        assert!(reduction(63, 63) < 10);
    }
}
