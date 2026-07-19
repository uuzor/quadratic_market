use crate::constants::SCALE;
use anchor_lang::prelude::*;

/// Get the current unix timestamp from the clock sysvar
pub fn get_current_timestamp(clock: &Sysvar<Clock>) -> i64 {
    clock.unix_timestamp
}

/// Get the current unix timestamp (convenience function)
pub fn current_unix_timestamp() -> Result<i64> {
    Ok(Clock::get()?.unix_timestamp)
}

// ─── Bitmask Helpers ──────────────────────────────────────────

/// Create a bitmask with the first `num_bits` bits set to 1.
pub fn create_mask(num_bits: u8) -> u16 {
    if num_bits == 0 {
        0
    } else if num_bits >= 16 {
        0xFFFF
    } else {
        (1u16 << num_bits) - 1
    }
}

/// Returns true if `index` is valid (within range of `num_bits`).
pub fn is_valid_index(index: u8, num_bits: u8) -> bool {
    index < num_bits
}

/// Count the number of set bits in a mask.
pub fn count_bits(mask: u16) -> u8 {
    mask.count_ones() as u8
}

/// Returns true if all bits in `expected` are set in `mask`.
pub fn has_all_bits(mask: u16, expected: u16) -> bool {
    mask & expected == expected
}

/// Returns true if any bit in `bits` is set in `mask`.
pub fn has_any_bit(mask: u16, bits: u16) -> bool {
    mask & bits != 0
}

/// Set a bit at `index` in `mask`.
pub fn set_bit(mask: u16, index: u8) -> u16 {
    mask | (1u16 << index)
}

/// Clear a bit at `index` in `mask`.
pub fn clear_bit(mask: u16, index: u8) -> u16 {
    mask & !(1u16 << index)
}

/// Check if a bit is set at `index` in `mask`.
pub fn is_bit_set(mask: u16, index: u8) -> bool {
    mask & (1u16 << index) != 0
}

// ─── Time/Validation Helpers ──────────────────────────────────

/// Returns true if `timestamp` is within the range [start, end).
pub fn is_within_window(timestamp: i64, start: i64, end: i64) -> bool {
    timestamp >= start && timestamp < end
}

/// Returns true if `timestamp` is after `start`.
pub fn is_after(timestamp: i64, start: i64) -> bool {
    timestamp >= start
}

/// Returns true if `timestamp` is before `end`.
pub fn is_before(timestamp: i64, end: i64) -> bool {
    timestamp < end
}

/// Calculate the difference between two timestamps.
pub fn timestamp_diff(a: i64, b: i64) -> i64 {
    // For i64, saturating_sub saturates at i64::MIN, not 0
    // Use checked_sub to get negative values when appropriate
    a.saturating_sub(b)
}

/// Calculate the positive difference between two timestamps.
pub fn positive_timestamp_diff(a: i64, b: i64) -> i64 {
    if a >= b {
        a - b
    } else {
        b - a
    }
}

/// Validate that a slip cancel deadline is reasonable (not too far in future).
pub fn validate_cancel_deadline(deadline: i64, now: i64, max_future_seconds: i64) -> Result<()> {
    require!(
        deadline > now,
        crate::errors::QuadraticMarketError::InvalidAmount
    );
    require!(
        deadline <= now.saturating_add(max_future_seconds),
        crate::errors::QuadraticMarketError::InvalidAmount
    );
    Ok(())
}

// ─── Fixed-Point Math Helpers ─────────────────────────────────

/// Returns the minimum of two Q32.32 values.
pub fn min_fp(a: u64, b: u64) -> u64 {
    std::cmp::min(a, b)
}

/// Returns the maximum of two Q32.32 values.
pub fn max_fp(a: u64, b: u64) -> u64 {
    std::cmp::max(a, b)
}

/// Clamp a Q32.32 value to [min, max].
pub fn clamp_fp(value: u64, min_val: u64, max_val: u64) -> u64 {
    min_fp(max_fp(value, min_val), max_val)
}

/// Convert basis points to a multiplier in Q32.32 (e.g., 10500 bps = 1.05x).
pub fn bps_to_multiplier(bps: u64) -> u64 {
    SCALE + (SCALE / 10000 * bps)
}

/// Convert a multiplier in Q32.32 to basis points (e.g., 1.05x = 10500 bps).
pub fn multiplier_to_bps(multiplier: u64) -> u64 {
    ((multiplier.saturating_sub(SCALE)) * 10000) / SCALE
}

// ───溢出 Overflow Protection Helpers ──────────────────────────

/// Safe addition with overflow check, returning the result or an error.
pub fn safe_add(a: u64, b: u64) -> Result<u64> {
    Ok(a.checked_add(b)
        .ok_or(crate::errors::QuadraticMarketError::MathOverflow)?)
}

/// Safe subtraction with underflow check, returning the result or an error.
pub fn safe_sub(a: u64, b: u64) -> Result<u64> {
    Ok(a.checked_sub(b)
        .ok_or(crate::errors::QuadraticMarketError::MathUnderflow)?)
}

/// Safe multiplication with overflow check, returning the result or an error.
pub fn safe_mul(a: u64, b: u64) -> Result<u64> {
    Ok(a.checked_mul(b)
        .ok_or(crate::errors::QuadraticMarketError::MathOverflow)?)
}

/// Safe division with zero check, returning the result or an error.
pub fn safe_div(a: u64, b: u64) -> Result<u64> {
    require!(b != 0, crate::errors::QuadraticMarketError::MathOverflow);
    Ok(a.checked_div(b)
        .ok_or(crate::errors::QuadraticMarketError::MathOverflow)?)
}

/// Saturating addition (returns max value on overflow instead of error).
pub fn sat_add(a: u64, b: u64) -> u64 {
    a.saturating_add(b)
}

/// Saturating subtraction (returns 0 on underflow instead of error).
pub fn sat_sub(a: u64, b: u64) -> u64 {
    a.saturating_sub(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Bitmask Helper Tests ────────────────────────────────────

    #[test]
    fn test_create_mask() {
        assert_eq!(create_mask(0), 0);
        assert_eq!(create_mask(1), 0b1);
        assert_eq!(create_mask(3), 0b111);
        assert_eq!(create_mask(8), 0b11111111);
        assert_eq!(create_mask(16), 0xFFFF);
        assert_eq!(create_mask(20), 0xFFFF); // Clamped to 16 bits
    }

    #[test]
    fn test_is_valid_index() {
        assert!(is_valid_index(0, 5));
        assert!(is_valid_index(4, 5));
        assert!(!is_valid_index(5, 5));
        assert!(!is_valid_index(10, 5));
    }

    #[test]
    fn test_count_bits() {
        assert_eq!(count_bits(0b0000), 0);
        assert_eq!(count_bits(0b0101), 2);
        assert_eq!(count_bits(0b1111), 4);
        assert_eq!(count_bits(0xFFFF), 16);
    }

    #[test]
    fn test_bit_operations() {
        let mask: u16 = 0b0101; // bits 0 and 2 set

        assert!(has_all_bits(mask, 0b0101));
        assert!(has_all_bits(mask, 0b0100));
        assert!(!has_all_bits(mask, 0b1000));

        // has_any_bit checks if ANY bit in the second arg is set in mask
        assert!(has_any_bit(mask, 0b0100)); // bit 2 is set in both
        assert!(has_any_bit(mask, 0b0001)); // bit 0 is set in both
        assert!(has_any_bit(mask, 0b0101)); // both bits 0 and 2 are set
        assert!(!has_any_bit(mask, 0b1010)); // bits 1 and 3 not in mask

        assert_eq!(set_bit(mask, 3), 0b1101);
        assert_eq!(clear_bit(mask, 0), 0b0100);
        assert_eq!(is_bit_set(mask, 0), true);
        assert_eq!(is_bit_set(mask, 1), false);
    }

    // ─── Time Helper Tests ───────────────────────────────────────

    #[test]
    fn test_is_within_window() {
        assert!(is_within_window(100, 50, 150));
        assert!(is_within_window(100, 100, 150));
        assert!(!is_within_window(100, 150, 200)); // After window
        assert!(!is_within_window(100, 50, 100)); // End is exclusive
    }

    #[test]
    fn test_timestamp_diff() {
        assert_eq!(timestamp_diff(100, 50), 50);
        assert_eq!(timestamp_diff(50, 100), -50); // Can be negative
    }

    #[test]
    fn test_positive_timestamp_diff() {
        assert_eq!(positive_timestamp_diff(100, 50), 50);
        assert_eq!(positive_timestamp_diff(50, 100), 50); // Always positive
    }

    // ─── Fixed-Point Helper Tests ─────────────────────────────────

    #[test]
    fn test_min_max_fp() {
        let a = 1_000_000_000u64; // ~0.23
        let b = 4_000_000_000u64; // ~0.93

        assert_eq!(min_fp(a, b), a);
        assert_eq!(max_fp(a, b), b);
    }

    #[test]
    fn test_bps_conversions() {
        // 500 bps = 5% = SCALE + 5%
        let mult = bps_to_multiplier(500); // 5%
        let expected = SCALE + (SCALE / 10000 * 500);
        assert_eq!(mult, expected);

        // Round trip (approximate due to integer math)
        let bps = multiplier_to_bps(mult);
        assert!(bps >= 499 && bps <= 501); // Allow 1 bps tolerance
    }

    // ─── Overflow Protection Tests ─────────────────────────────────

    #[test]
    fn test_safe_arithmetic() {
        assert_eq!(safe_add(1, 2).unwrap(), 3);
        assert!(safe_add(u64::MAX, 1).is_err());

        assert_eq!(safe_sub(5, 3).unwrap(), 2);
        assert!(safe_sub(3, 5).is_err());

        assert_eq!(safe_mul(3, 4).unwrap(), 12);
        assert!(safe_mul(u64::MAX, 2).is_err());

        assert_eq!(safe_div(12, 4).unwrap(), 3);
        assert!(safe_div(12, 0).is_err());
    }

    #[test]
    fn test_saturating_arithmetic() {
        assert_eq!(sat_add(u64::MAX, 1), u64::MAX);
        assert_eq!(sat_sub(3, 5), 0);
    }
}
