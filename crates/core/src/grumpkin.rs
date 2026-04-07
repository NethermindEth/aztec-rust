//! Minimal Grumpkin curve arithmetic for contract address derivation.
//!
//! Grumpkin is an embedded curve of BN254 defined by `y^2 = x^3 - 17`
//! over BN254's scalar field (Fr). Only affine point addition and
//! scalar multiplication are implemented — just enough for
//! `compute_contract_address_from_instance`.

use ark_bn254::Fr as ArkFr;
use ark_ff::{AdditiveGroup, BigInteger, Field, PrimeField};

use crate::types::{Fq, Fr, Point};

/// Grumpkin curve parameter: y^2 = x^3 + B, where B = -17.
const B: i64 = -17;

/// Return the Grumpkin generator point G = (1, y) where y = sqrt(1 - 17).
pub fn generator() -> Point {
    let one = ArkFr::from(1u64);
    // rhs = 1^3 + B = 1 - 17 = -16
    let rhs = one + ArkFr::from(B.unsigned_abs()) * (-ArkFr::from(1u64));
    let y = rhs.sqrt().expect("Grumpkin generator y must exist");

    // Pick the lexicographically smaller y (convention).
    let neg_y = -y;
    let chosen = if y.into_bigint() < neg_y.into_bigint() {
        y
    } else {
        neg_y
    };

    Point {
        x: Fr(one),
        y: Fr(chosen),
        is_infinite: false,
    }
}

/// Add two Grumpkin affine points.
pub fn point_add(p: &Point, q: &Point) -> Point {
    if p.is_infinite {
        return *q;
    }
    if q.is_infinite {
        return *p;
    }

    let px = p.x.0;
    let py = p.y.0;
    let qx = q.x.0;
    let qy = q.y.0;

    // If same x but opposite y (or both zero), result is point at infinity.
    if px == qx {
        if py == -qy || (py == ArkFr::ZERO && qy == ArkFr::ZERO) {
            return Point {
                x: Fr::zero(),
                y: Fr::zero(),
                is_infinite: true,
            };
        }
        // Same point → doubling
        return point_double(p);
    }

    // Standard affine addition: λ = (qy - py) / (qx - px)
    let lambda = (qy - py) * (qx - px).inverse().expect("non-zero denominator");
    let x3 = lambda * lambda - px - qx;
    let y3 = lambda * (px - x3) - py;

    Point {
        x: Fr(x3),
        y: Fr(y3),
        is_infinite: false,
    }
}

/// Double a Grumpkin affine point.
fn point_double(p: &Point) -> Point {
    if p.is_infinite || p.y.0 == ArkFr::ZERO {
        return Point {
            x: Fr::zero(),
            y: Fr::zero(),
            is_infinite: true,
        };
    }

    let px = p.x.0;
    let py = p.y.0;

    // Grumpkin has a = 0, so λ = 3x^2 / (2y)
    let three_x_sq = ArkFr::from(3u64) * px * px;
    let two_y = ArkFr::from(2u64) * py;
    let lambda = three_x_sq * two_y.inverse().expect("non-zero y");
    let x3 = lambda * lambda - px - px;
    let y3 = lambda * (px - x3) - py;

    Point {
        x: Fr(x3),
        y: Fr(y3),
        is_infinite: false,
    }
}

/// Scalar multiplication via double-and-add.
///
/// The scalar is an `Fq` element (BN254 base field = Grumpkin scalar field).
pub fn scalar_mul(scalar: &Fq, point: &Point) -> Point {
    if point.is_infinite {
        return *point;
    }
    if scalar.is_zero() {
        return Point {
            x: Fr::zero(),
            y: Fr::zero(),
            is_infinite: true,
        };
    }

    let bits = scalar.0.into_bigint().to_bits_be();

    // Skip leading zeros
    let mut result = Point {
        x: Fr::zero(),
        y: Fr::zero(),
        is_infinite: true,
    };

    for bit in bits {
        result = point_double(&result);
        if bit {
            result = point_add(&result, point);
        }
    }

    result
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn generator_is_on_curve() {
        let g = generator();
        assert!(!g.is_infinite);
        // Verify y^2 = x^3 - 17
        let lhs = g.y.0 * g.y.0;
        let rhs = g.x.0 * g.x.0 * g.x.0 - ArkFr::from(17u64);
        assert_eq!(lhs, rhs, "generator must satisfy Grumpkin equation");
    }

    #[test]
    fn identity_add() {
        let g = generator();
        let inf = Point {
            x: Fr::zero(),
            y: Fr::zero(),
            is_infinite: true,
        };
        let result = point_add(&g, &inf);
        assert_eq!(result, g);
        let result2 = point_add(&inf, &g);
        assert_eq!(result2, g);
    }

    #[test]
    fn scalar_mul_one() {
        let g = generator();
        let result = scalar_mul(&Fq::one(), &g);
        assert_eq!(result, g);
    }

    #[test]
    fn scalar_mul_two_equals_double() {
        let g = generator();
        let doubled = point_double(&g);
        let result = scalar_mul(&Fq::from(2u64), &g);
        assert_eq!(result, doubled);
    }

    #[test]
    fn scalar_mul_zero_returns_infinity() {
        let g = generator();
        let result = scalar_mul(&Fq::zero(), &g);
        assert!(result.is_infinite);
    }

    #[test]
    fn point_add_inverse_returns_infinity() {
        let g = generator();
        let neg_g = Point {
            x: g.x,
            y: Fr(-(g.y.0)),
            is_infinite: false,
        };
        let result = point_add(&g, &neg_g);
        assert!(result.is_infinite);
    }

    #[test]
    fn scalar_mul_associative() {
        let g = generator();
        // 3G = G + 2G
        let two_g = scalar_mul(&Fq::from(2u64), &g);
        let three_g_add = point_add(&g, &two_g);
        let three_g_mul = scalar_mul(&Fq::from(3u64), &g);
        assert_eq!(three_g_add, three_g_mul);
    }

    #[test]
    fn result_is_on_curve() {
        let g = generator();
        let p = scalar_mul(&Fq::from(12345u64), &g);
        assert!(!p.is_infinite);
        let lhs = p.y.0 * p.y.0;
        let rhs = p.x.0 * p.x.0 * p.x.0 - ArkFr::from(17u64);
        assert_eq!(lhs, rhs, "scalar_mul result must be on curve");
    }
}
