use crate::constants::{LN2_FP, SCALE};
use crate::errors::QuadraticMarketError;
use anchor_lang::prelude::*;

/// Compute e^x for x in Q32.32 format, returning Q32.32.
/// Uses Taylor series: e^x = 1 + x + x^2/2! + ... + x^10/10!
/// Expects x <= 0 (after normalization). For x > 0, returns SCALE (1.0).
pub fn exp_q32(x: i64) -> u64 {
    const MAX_NEG: i64 = -20 * (SCALE as i64); // e^(-20) ≈ 2e-9, effectively 0

    if x <= MAX_NEG {
        return 0;
    }
    if x >= 0 {
        return SCALE;
    }

    // Taylor series: e^x = Σ x^n / n! for n=0..10
    let mut result: i64 = SCALE as i64; // term 0: 1.0
    let mut term: i64 = SCALE as i64; // running term = x^n / n!

    for n in 1..=10_i64 {
        // term = term * x / n (in Q32.32)
        // Do (term * x) >> 32 first, then divide by n (integer)
        let product = (term as i128).checked_mul(x as i128).unwrap_or(0);
        term = ((product >> 32) / (n as i128)) as i64;
        result = result.checked_add(term).unwrap_or(result);
    }

    if result < 0 {
        0
    } else {
        result as u64
    }
}

/// Compute ln(x) for x in Q32.32 format, returning Q32.32.
/// Uses argument reduction: ln(x) = ln(m) + k * ln(2)
/// where x = m * 2^k and 0.5 <= m < 1.0 (i.e., m in Q32.32 is [SCALE/2, SCALE))
/// Then ln(1+y) = y - y^2/2 + y^3/3 - ... for |y| < 1
pub fn ln_q32(x: u64) -> Result<i64> {
    require!(x > 0, QuadraticMarketError::MathUnderflow);

    if x == SCALE {
        return Ok(0); // ln(1) = 0
    }

    // Argument reduction: find k such that x/2^k is in [0.5, 1.0) in Q32.32
    // i.e., SCALE/2 <= x/2^k < SCALE
    let mut k: i64 = 0;
    let mut m = x;

    // Reduce m until it's < SCALE (1.0)
    while m >= SCALE {
        m >>= 1;
        k += 1;
    }

    // Increase m until it's >= SCALE/2 (0.5)
    while m > 0 && m < SCALE / 2 {
        m <<= 1;
        k -= 1;
    }

    if m == 0 {
        return Err(QuadraticMarketError::MathUnderflow.into());
    }

    // Now compute ln(m) where m is in [SCALE/2, SCALE), i.e., 0.5 <= m < 1.0
    // Let y = m/SCALE - 1, so y is in [-0.5, 0)
    // ln(1+y) = y - y^2/2 + y^3/3 - y^4/4 + ...
    // We need y in Q32.32
    let y = (m as i64) - (SCALE as i64); // m - 1.0 in Q32.32, range [-SCALE/2, 0)

    let mut ln_m: i64 = 0;
    let mut y_power: i64 = y; // y^1

    for n in 1..=20_i64 {
        if n % 2 == 1 {
            // Odd: add y^n / n
            ln_m = ln_m
                .checked_add(y_power / n)
                .ok_or(QuadraticMarketError::MathOverflow)?;
        } else {
            // Even: subtract y^n / n
            ln_m = ln_m
                .checked_sub(y_power / n)
                .ok_or(QuadraticMarketError::MathOverflow)?;
        }
        // y_power = y_power * y >> 32
        y_power = ((y_power as i128 * y as i128) >> 32) as i64;

        // Early exit if term is negligible
        if y_power == 0 {
            break;
        }
    }

    // ln(x) = ln(m) + k * ln(2)
    let ln_x = ln_m
        .checked_add(k * LN2_FP)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    Ok(ln_x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exp_zero() {
        // e^0 = 1.0
        let result = exp_q32(0);
        assert_eq!(result, SCALE);
    }

    #[test]
    fn test_exp_negative_one() {
        // e^(-1) ≈ 0.367879
        let neg_one = -(SCALE as i64); // -1.0 in Q32.32
        let result = exp_q32(neg_one);
        let expected = (0.367879 * SCALE as f64) as u64;
        let tolerance = SCALE / 1000; // 0.1% tolerance
        assert!(
            (result as i64 - expected as i64).unsigned_abs() < tolerance as u64,
            "exp(-1) = {} expected {} (tolerance {})",
            result,
            expected,
            tolerance
        );
    }

    #[test]
    fn test_exp_very_negative() {
        // e^(-21) should be 0 (below threshold)
        let result = exp_q32(-21 * SCALE as i64);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_ln_one() {
        // ln(1) = 0
        let result = ln_q32(SCALE).unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_ln_e() {
        // ln(e) ≈ 1.0
        // e in Q32.32 ≈ 2.71828 * 2^32
        let e_fp = (2.71828 * SCALE as f64) as u64;
        let result = ln_q32(e_fp).unwrap();
        let expected = SCALE as i64; // 1.0 in Q32.32
        let tolerance = SCALE as i64 / 100; // 1% tolerance
        assert!(
            (result - expected).unsigned_abs() < tolerance as u64,
            "ln(e) = {} expected {}",
            result,
            expected
        );
    }

    #[test]
    fn test_ln_two() {
        // ln(2) ≈ 0.6931
        let two_fp = 2 * SCALE;
        let result = ln_q32(two_fp).unwrap();
        let expected = LN2_FP;
        let tolerance = SCALE as i64 / 100;
        assert!(
            (result - expected).unsigned_abs() < tolerance as u64,
            "ln(2) = {} expected {}",
            result,
            expected
        );
    }

    #[test]
    fn test_exp_ln_roundtrip() {
        // ln(e^x) should ≈ x for negative x
        let x = -(SCALE as i64 / 2); // -0.5
        let exp_x = exp_q32(x);
        if exp_x > 0 {
            let ln_result = ln_q32(exp_x).unwrap();
            let tolerance = SCALE as i64 / 50; // 2% tolerance
            assert!(
                (ln_result - x).unsigned_abs() < tolerance as u64,
                "ln(e^({})) = {} expected {}",
                x,
                ln_result,
                x
            );
        }
    }
}
