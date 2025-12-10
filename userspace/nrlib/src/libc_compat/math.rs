//! Math functions for libc compatibility
//!
//! Provides standard C math library functions (libm) for userspace programs.
//! These are implemented using Taylor series and other numerical methods.

use core::f64::consts::PI;

// ============================================================================
// Constants
// ============================================================================

/// Euler's number
pub const E: f64 = 2.718281828459045;

/// Natural logarithm of 2
pub const LN_2: f64 = 0.6931471805599453;

/// Natural logarithm of 10
pub const LN_10: f64 = 2.302585092994046;

// ============================================================================
// Internal Helper Functions
// ============================================================================

/// Internal truncation helper (rounds toward zero)
#[inline]
fn trunc_internal(x: f64) -> f64 {
    if x >= 0.0 {
        x as i64 as f64
    } else {
        -((-x) as i64 as f64)
    }
}

// ============================================================================
// Basic Math Functions
// ============================================================================

/// Compute sine using Taylor series
/// sin(x) = x - x³/3! + x⁵/5! - x⁷/7! + ...
#[unsafe(no_mangle)]
pub extern "C" fn sin(x: f64) -> f64 {
    // Normalize to [-π, π]
    let mut x = x % (2.0 * PI);
    if x > PI {
        x -= 2.0 * PI;
    } else if x < -PI {
        x += 2.0 * PI;
    }

    let x2 = x * x;
    let mut term = x;
    let mut result = x;

    for i in 1..25 {
        term *= -x2 / ((2 * i) * (2 * i + 1)) as f64;
        result += term;
        if term.abs() < 1e-16 {
            break;
        }
    }

    result
}

/// Compute cosine using Taylor series
/// cos(x) = 1 - x²/2! + x⁴/4! - x⁶/6! + ...
#[unsafe(no_mangle)]
pub extern "C" fn cos(x: f64) -> f64 {
    // Normalize to [-π, π]
    let mut x = x % (2.0 * PI);
    if x > PI {
        x -= 2.0 * PI;
    } else if x < -PI {
        x += 2.0 * PI;
    }

    let x2 = x * x;
    let mut term = 1.0;
    let mut result = 1.0;

    for i in 1..25 {
        term *= -x2 / ((2 * i - 1) * (2 * i)) as f64;
        result += term;
        if term.abs() < 1e-16 {
            break;
        }
    }

    result
}

/// Compute tangent
/// tan(x) = sin(x) / cos(x)
#[unsafe(no_mangle)]
pub extern "C" fn tan(x: f64) -> f64 {
    let c = cos(x);
    if c.abs() < 1e-15 {
        if sin(x) >= 0.0 {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        }
    } else {
        sin(x) / c
    }
}

/// Compute arc sine using Newton's method
#[unsafe(no_mangle)]
pub extern "C" fn asin(x: f64) -> f64 {
    if x < -1.0 || x > 1.0 {
        return f64::NAN;
    }
    if x == 1.0 {
        return PI / 2.0;
    }
    if x == -1.0 {
        return -PI / 2.0;
    }

    // Use atan for numerical stability: asin(x) = atan(x / sqrt(1 - x²))
    let denom = sqrt(1.0 - x * x);
    if denom < 1e-15 {
        if x >= 0.0 {
            PI / 2.0
        } else {
            -PI / 2.0
        }
    } else {
        atan(x / denom)
    }
}

/// Compute arc cosine
#[unsafe(no_mangle)]
pub extern "C" fn acos(x: f64) -> f64 {
    if x < -1.0 || x > 1.0 {
        return f64::NAN;
    }
    PI / 2.0 - asin(x)
}

/// Compute arc tangent using Taylor series
#[unsafe(no_mangle)]
pub extern "C" fn atan(x: f64) -> f64 {
    // For |x| > 1, use atan(x) = π/2 - atan(1/x)
    if x.abs() > 1.0 {
        let sign = if x >= 0.0 { 1.0 } else { -1.0 };
        return sign * (PI / 2.0) - atan(1.0 / x);
    }

    // Taylor series: atan(x) = x - x³/3 + x⁵/5 - x⁷/7 + ...
    let x2 = x * x;
    let mut term = x;
    let mut result = x;

    for i in 1..50 {
        term *= -x2;
        let contrib = term / (2 * i + 1) as f64;
        result += contrib;
        if contrib.abs() < 1e-16 {
            break;
        }
    }

    result
}

/// Compute arc tangent of y/x with correct quadrant
#[unsafe(no_mangle)]
pub extern "C" fn atan2(y: f64, x: f64) -> f64 {
    if x > 0.0 {
        atan(y / x)
    } else if x < 0.0 && y >= 0.0 {
        atan(y / x) + PI
    } else if x < 0.0 && y < 0.0 {
        atan(y / x) - PI
    } else if x == 0.0 && y > 0.0 {
        PI / 2.0
    } else if x == 0.0 && y < 0.0 {
        -PI / 2.0
    } else {
        0.0 // x == 0 && y == 0
    }
}

// ============================================================================
// Exponential and Logarithmic Functions
// ============================================================================

/// Compute e^x using Taylor series
/// exp(x) = 1 + x + x²/2! + x³/3! + ...
#[unsafe(no_mangle)]
pub extern "C" fn exp(x: f64) -> f64 {
    // Handle special cases
    if x == 0.0 {
        return 1.0;
    }
    if x > 709.0 {
        return f64::INFINITY;
    }
    if x < -709.0 {
        return 0.0;
    }

    // For large |x|, use exp(x) = exp(x/2)² to improve convergence
    if x.abs() > 1.0 {
        let half = exp(x / 2.0);
        return half * half;
    }

    let mut result = 1.0;
    let mut term = 1.0;

    for i in 1..100 {
        term *= x / i as f64;
        result += term;
        if term.abs() < 1e-16 {
            break;
        }
    }

    result
}

/// Compute 2^x
#[unsafe(no_mangle)]
pub extern "C" fn exp2(x: f64) -> f64 {
    exp(x * LN_2)
}

/// Compute 10^x
#[unsafe(no_mangle)]
pub extern "C" fn exp10(x: f64) -> f64 {
    exp(x * LN_10)
}

/// Compute natural logarithm using the series expansion
/// For x near 1: ln(x) = 2 * (z + z³/3 + z⁵/5 + ...) where z = (x-1)/(x+1)
#[unsafe(no_mangle)]
pub extern "C" fn log(x: f64) -> f64 {
    if x <= 0.0 {
        return f64::NAN;
    }
    if x == 1.0 {
        return 0.0;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }

    // Reduce x to [0.5, 2] range: x = m * 2^e, then ln(x) = ln(m) + e*ln(2)
    let mut e = 0i32;
    let mut m = x;

    while m >= 2.0 {
        m /= 2.0;
        e += 1;
    }
    while m < 0.5 {
        m *= 2.0;
        e -= 1;
    }

    // Now compute ln(m) where m is in [0.5, 2]
    let z = (m - 1.0) / (m + 1.0);
    let z2 = z * z;
    let mut term = z;
    let mut result = z;

    for i in 1..50 {
        term *= z2;
        let contrib = term / (2 * i + 1) as f64;
        result += contrib;
        if contrib.abs() < 1e-16 {
            break;
        }
    }

    2.0 * result + e as f64 * LN_2
}

/// Compute base-2 logarithm
#[unsafe(no_mangle)]
pub extern "C" fn log2(x: f64) -> f64 {
    log(x) / LN_2
}

/// Compute base-10 logarithm
#[unsafe(no_mangle)]
pub extern "C" fn log10(x: f64) -> f64 {
    log(x) / LN_10
}

/// Compute ln(1 + x) with better precision for small x
#[unsafe(no_mangle)]
pub extern "C" fn log1p(x: f64) -> f64 {
    if x.abs() < 1e-4 {
        // Use Taylor series directly for small x
        let mut term = x;
        let mut result = x;
        for i in 2..50 {
            term *= -x;
            let contrib = term / i as f64;
            result += contrib;
            if contrib.abs() < 1e-16 {
                break;
            }
        }
        result
    } else {
        log(1.0 + x)
    }
}

/// Compute exp(x) - 1 with better precision for small x
#[unsafe(no_mangle)]
pub extern "C" fn expm1(x: f64) -> f64 {
    if x.abs() < 1e-4 {
        // Use Taylor series directly
        let mut term = x;
        let mut result = x;
        for i in 2..50 {
            term *= x / i as f64;
            result += term;
            if term.abs() < 1e-16 {
                break;
            }
        }
        result
    } else {
        exp(x) - 1.0
    }
}

// ============================================================================
// Power Functions
// ============================================================================

/// Compute x^y
#[unsafe(no_mangle)]
pub extern "C" fn pow(x: f64, y: f64) -> f64 {
    // Special cases
    if y == 0.0 {
        return 1.0;
    }
    if x == 0.0 {
        return if y > 0.0 { 0.0 } else { f64::INFINITY };
    }
    if x == 1.0 {
        return 1.0;
    }
    if y == 1.0 {
        return x;
    }
    if y == 2.0 {
        return x * x;
    }
    if y == -1.0 {
        return 1.0 / x;
    }
    if y == 0.5 {
        return sqrt(x);
    }

    // For integer exponents, use repeated multiplication
    if y == trunc_internal(y) && y.abs() <= 100.0 {
        let mut result = 1.0;
        let mut exp = y.abs() as i32;
        let mut base = x;

        // Fast exponentiation
        while exp > 0 {
            if exp & 1 == 1 {
                result *= base;
            }
            base *= base;
            exp >>= 1;
        }

        if y < 0.0 {
            1.0 / result
        } else {
            result
        }
    } else if x > 0.0 {
        // General case: x^y = exp(y * ln(x))
        exp(y * log(x))
    } else {
        // Negative base with non-integer exponent
        f64::NAN
    }
}

/// Compute square root using Newton's method
#[unsafe(no_mangle)]
pub extern "C" fn sqrt(x: f64) -> f64 {
    if x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 || x == 1.0 {
        return x;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }

    // Initial guess
    let mut guess = x;
    if x > 1.0 {
        guess = x / 2.0;
    }

    // Newton's method: x_{n+1} = (x_n + S/x_n) / 2
    for _ in 0..50 {
        let new_guess = (guess + x / guess) / 2.0;
        if (new_guess - guess).abs() < guess * 1e-15 {
            break;
        }
        guess = new_guess;
    }

    guess
}

/// Compute cube root
#[unsafe(no_mangle)]
pub extern "C" fn cbrt(x: f64) -> f64 {
    if x == 0.0 {
        return 0.0;
    }

    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let abs_x = x.abs();

    // Newton's method for cube root: x_{n+1} = (2*x_n + S/x_n²) / 3
    let mut guess = abs_x / 3.0;
    if abs_x > 1.0 {
        guess = abs_x / 3.0;
    } else {
        guess = abs_x;
    }

    for _ in 0..50 {
        let new_guess = (2.0 * guess + abs_x / (guess * guess)) / 3.0;
        if (new_guess - guess).abs() < guess * 1e-15 {
            break;
        }
        guess = new_guess;
    }

    sign * guess
}

/// Compute hypotenuse: sqrt(x² + y²)
#[unsafe(no_mangle)]
pub extern "C" fn hypot(x: f64, y: f64) -> f64 {
    // Use formula that avoids overflow
    let ax = x.abs();
    let ay = y.abs();

    if ax > ay {
        let r = ay / ax;
        ax * sqrt(1.0 + r * r)
    } else if ay > 0.0 {
        let r = ax / ay;
        ay * sqrt(1.0 + r * r)
    } else {
        0.0
    }
}

// ============================================================================
// Hyperbolic Functions
// ============================================================================

/// Compute hyperbolic sine
/// sinh(x) = (e^x - e^(-x)) / 2
#[unsafe(no_mangle)]
pub extern "C" fn sinh(x: f64) -> f64 {
    if x.abs() < 1e-4 {
        // Taylor series for small x
        x + x * x * x / 6.0
    } else {
        (exp(x) - exp(-x)) / 2.0
    }
}

/// Compute hyperbolic cosine
/// cosh(x) = (e^x + e^(-x)) / 2
#[unsafe(no_mangle)]
pub extern "C" fn cosh(x: f64) -> f64 {
    (exp(x) + exp(-x)) / 2.0
}

/// Compute hyperbolic tangent
/// tanh(x) = sinh(x) / cosh(x)
#[unsafe(no_mangle)]
pub extern "C" fn tanh(x: f64) -> f64 {
    if x > 20.0 {
        return 1.0;
    }
    if x < -20.0 {
        return -1.0;
    }

    let e2x = exp(2.0 * x);
    (e2x - 1.0) / (e2x + 1.0)
}

/// Compute inverse hyperbolic sine
/// asinh(x) = ln(x + sqrt(x² + 1))
#[unsafe(no_mangle)]
pub extern "C" fn asinh(x: f64) -> f64 {
    if x.abs() < 1e-4 {
        x
    } else {
        log(x + sqrt(x * x + 1.0))
    }
}

/// Compute inverse hyperbolic cosine
/// acosh(x) = ln(x + sqrt(x² - 1))
#[unsafe(no_mangle)]
pub extern "C" fn acosh(x: f64) -> f64 {
    if x < 1.0 {
        return f64::NAN;
    }
    log(x + sqrt(x * x - 1.0))
}

/// Compute inverse hyperbolic tangent
/// atanh(x) = ln((1+x)/(1-x)) / 2
#[unsafe(no_mangle)]
pub extern "C" fn atanh(x: f64) -> f64 {
    if x <= -1.0 || x >= 1.0 {
        return f64::NAN;
    }
    log((1.0 + x) / (1.0 - x)) / 2.0
}

// ============================================================================
// Rounding and Remainder Functions
// ============================================================================

/// Round to nearest integer, away from zero for halfway cases
#[unsafe(no_mangle)]
pub extern "C" fn round(x: f64) -> f64 {
    let t = trunc_internal(x);
    let f = x - t;

    if f.abs() >= 0.5 {
        if x >= 0.0 {
            t + 1.0
        } else {
            t - 1.0
        }
    } else {
        t
    }
}

/// Round toward positive infinity
#[unsafe(no_mangle)]
pub extern "C" fn ceil(x: f64) -> f64 {
    let t = trunc_internal(x);
    if x > t {
        t + 1.0
    } else {
        t
    }
}

/// Round toward negative infinity
#[unsafe(no_mangle)]
pub extern "C" fn floor(x: f64) -> f64 {
    let t = trunc_internal(x);
    if x < t {
        t - 1.0
    } else {
        t
    }
}

/// Truncate to integer (round toward zero)
#[unsafe(no_mangle)]
pub extern "C" fn trunc(x: f64) -> f64 {
    trunc_internal(x)
}

/// Compute floating-point remainder
#[unsafe(no_mangle)]
pub extern "C" fn fmod(x: f64, y: f64) -> f64 {
    if y == 0.0 {
        return f64::NAN;
    }
    x - trunc_internal(x / y) * y
}

/// Compute remainder with rounding to nearest
#[unsafe(no_mangle)]
pub extern "C" fn remainder(x: f64, y: f64) -> f64 {
    if y == 0.0 {
        return f64::NAN;
    }
    x - round(x / y) * y
}

// ============================================================================
// Other Functions
// ============================================================================

/// Absolute value
#[unsafe(no_mangle)]
pub extern "C" fn fabs(x: f64) -> f64 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}

/// Copy sign of y to x
#[unsafe(no_mangle)]
pub extern "C" fn copysign(x: f64, y: f64) -> f64 {
    let abs_x = if x < 0.0 { -x } else { x };
    if y < 0.0 {
        -abs_x
    } else {
        abs_x
    }
}

/// Return larger of x and y
#[unsafe(no_mangle)]
pub extern "C" fn fmax(x: f64, y: f64) -> f64 {
    if x.is_nan() {
        return y;
    }
    if y.is_nan() {
        return x;
    }
    if x > y {
        x
    } else {
        y
    }
}

/// Return smaller of x and y
#[unsafe(no_mangle)]
pub extern "C" fn fmin(x: f64, y: f64) -> f64 {
    if x.is_nan() {
        return y;
    }
    if y.is_nan() {
        return x;
    }
    if x < y {
        x
    } else {
        y
    }
}

/// Positive difference: max(x - y, 0)
#[unsafe(no_mangle)]
pub extern "C" fn fdim(x: f64, y: f64) -> f64 {
    if x > y {
        x - y
    } else {
        0.0
    }
}

/// Fused multiply-add: x * y + z
#[unsafe(no_mangle)]
pub extern "C" fn fma(x: f64, y: f64, z: f64) -> f64 {
    x * y + z
}

/// Compute x * 2^n
#[unsafe(no_mangle)]
pub extern "C" fn ldexp(x: f64, n: i32) -> f64 {
    x * pow(2.0, n as f64)
}

/// Compute x * 2^n (same as ldexp)
#[unsafe(no_mangle)]
pub extern "C" fn scalbn(x: f64, n: i32) -> f64 {
    ldexp(x, n)
}

// ============================================================================
// Float versions (32-bit)
// ============================================================================

#[unsafe(no_mangle)]
pub extern "C" fn sinf(x: f32) -> f32 {
    sin(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn cosf(x: f32) -> f32 {
    cos(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn tanf(x: f32) -> f32 {
    tan(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn asinf(x: f32) -> f32 {
    asin(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn acosf(x: f32) -> f32 {
    acos(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn atanf(x: f32) -> f32 {
    atan(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn atan2f(y: f32, x: f32) -> f32 {
    atan2(y as f64, x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn expf(x: f32) -> f32 {
    exp(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn exp2f(x: f32) -> f32 {
    exp2(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn logf(x: f32) -> f32 {
    log(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn log2f(x: f32) -> f32 {
    log2(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn log10f(x: f32) -> f32 {
    log10(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn powf(x: f32, y: f32) -> f32 {
    pow(x as f64, y as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn sqrtf(x: f32) -> f32 {
    sqrt(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn cbrtf(x: f32) -> f32 {
    cbrt(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn hypotf(x: f32, y: f32) -> f32 {
    hypot(x as f64, y as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn sinhf(x: f32) -> f32 {
    sinh(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn coshf(x: f32) -> f32 {
    cosh(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn tanhf(x: f32) -> f32 {
    tanh(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn roundf(x: f32) -> f32 {
    round(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn ceilf(x: f32) -> f32 {
    ceil(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn floorf(x: f32) -> f32 {
    floor(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn truncf(x: f32) -> f32 {
    trunc(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn fmodf(x: f32, y: f32) -> f32 {
    fmod(x as f64, y as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn fabsf(x: f32) -> f32 {
    fabs(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn fmaxf(x: f32, y: f32) -> f32 {
    fmax(x as f64, y as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn fminf(x: f32, y: f32) -> f32 {
    fmin(x as f64, y as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn copysignf(x: f32, y: f32) -> f32 {
    copysign(x as f64, y as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn ldexpf(x: f32, n: i32) -> f32 {
    ldexp(x as f64, n) as f32
}
