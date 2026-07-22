#!/usr/bin/env python3
"""Dependency-free exact oracle for the bounded sparse QCGA3D incidence slice.

This is deliberately independent of the Fe fixture. It owns the semantic null
metric, the null-to-orthogonal basis transformation, point embedding, dual
quadric construction, and exact rational KATs. It does not implement dense
Cl(9,6) products or claim general QCGA support.
"""

from fractions import Fraction as Q


DIM = 15
E = range(0, 3)
EO = range(3, 9)
EI = range(9, 15)
EXECUTION_METRIC = (1,) * 9 + (-1,) * 6
EXECUTION_NAMES = (
    "e1", "e2", "e3",
    "p1", "p2", "p3", "p4", "p5", "p6",
    "m1", "m2", "m3", "m4", "m5", "m6",
)
COEFFICIENT_NAMES = ("A", "B", "C", "D", "E", "F", "G", "H", "I", "J")
POINT_SUPPORT = (0, 1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 14)
DUAL_SUPPORT = (0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11)


def vector(**entries):
    names = ("e1", "e2", "e3") + tuple(f"eo{k}" for k in range(1, 7)) + tuple(
        f"ei{k}" for k in range(1, 7)
    )
    result = [Q(0)] * DIM
    for name, value in entries.items():
        result[names.index(name)] = Q(value)
    return result


def null_dot(left, right):
    euclidean = sum(left[k] * right[k] for k in E)
    null_pairs = sum(
        left[3 + k] * right[9 + k] + left[9 + k] * right[3 + k]
        for k in range(6)
    )
    return euclidean - null_pairs


def execution_basis():
    basis = []
    for k in range(3):
        entry = [Q(0)] * DIM
        entry[k] = 1
        basis.append(entry)
    for k in range(6):
        entry = [Q(0)] * DIM
        entry[3 + k], entry[9 + k] = Q(1), -Q(1, 2)
        basis.append(entry)
    for k in range(6):
        entry = [Q(0)] * DIM
        entry[3 + k], entry[9 + k] = Q(1), Q(1, 2)
        basis.append(entry)
    return basis


def point(x, y, z):
    x, y, z = Q(x), Q(y), Q(z)
    result = [Q(0)] * DIM
    result[0:3] = (x, y, z)
    result[3:6] = (Q(1), Q(1), Q(1))
    result[9:15] = (x * x / 2, y * y / 2, z * z / 2, x * y, x * z, y * z)
    return result


def dual_quadric(coefficients):
    A, B, C, D, E_, F, G, H, I, J = map(Q, coefficients)
    result = [Q(0)] * DIM
    result[0:3] = (G, H, I)
    result[3:9] = (-2 * A, -2 * B, -2 * C, -D, -E_, -F)
    result[9:12] = (-J / 3,) * 3
    return result


def fused_polynomial(x, y, z, coefficients):
    x, y, z = Q(x), Q(y), Q(z)
    A, B, C, D, E_, F, G, H, I, J = map(Q, coefficients)
    return (
        A * x * x + B * y * y + C * z * z
        + D * x * y + E_ * x * z + F * y * z
        + G * x + H * y + I * z + J
    )


KATS = (
    ("sphere/on", (3, 4, 0), (1, 1, 1, 0, 0, 0, 0, 0, 0, -25), Q(0)),
    ("sphere/off", (0, 0, 0), (1, 1, 1, 0, 0, 0, 0, 0, 0, -25), Q(-25)),
    # (x-1)^2 + 2(y+2)^2 + 3(z-1)^2 - 12
    ("translated-ellipsoid", (2, -1, 2), (1, 2, 3, 0, 0, 0, -2, 8, -6, 0), Q(-6)),
    # All rotated cross terms are live and asymmetric in sign.
    ("rotated-cross-terms", (2, -1, 3), (5, 5, 2, 6, -4, 2, -3, 7, 1, Q(5, 3)), Q(-Q(22, 3))),
    # J/3 must remain rational until the contraction's three equal contributions.
    ("fractional-J", (1, 2, -1), (2, -1, 3, 1, -2, 4, 5, -3, 2, Q(1, 7)), Q(-Q(41, 7))),
)


def check():
    basis = execution_basis()
    assert len(basis) == DIM
    for row, left in enumerate(basis):
        for column, right in enumerate(basis):
            expected = Q(EXECUTION_METRIC[row]) if row == column else Q(0)
            assert null_dot(left, right) == expected, (
                EXECUTION_NAMES[row], EXECUTION_NAMES[column], null_dot(left, right), expected
            )

    # Reconstructed paper-null vectors pin each pair and all cross-pair zeros.
    for k in range(6):
        p, m = basis[3 + k], basis[9 + k]
        eo = [(p[i] + m[i]) / 2 for i in range(DIM)]
        ei = [m[i] - p[i] for i in range(DIM)]
        assert null_dot(eo, eo) == 0
        assert null_dot(ei, ei) == 0
        assert null_dot(eo, ei) == -1
        for other in range(6):
            if other != k:
                assert null_dot(eo, basis[3 + other]) == 0
                assert null_dot(ei, basis[9 + other]) == 0

    assert tuple(1 << k for k in range(DIM)) == (
        1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384
    )
    assert len(POINT_SUPPORT) == len(set(POINT_SUPPORT)) == 12
    assert len(DUAL_SUPPORT) == len(set(DUAL_SUPPORT)) == 12
    assert tuple(COEFFICIENT_NAMES) == ("A", "B", "C", "D", "E", "F", "G", "H", "I", "J")
    for label, xyz, coefficients, expected in KATS:
        expanded = null_dot(point(*xyz), dual_quadric(coefficients))
        fused = fused_polynomial(*xyz, coefficients)
        assert expanded == fused == expected, (label, expanded, fused, expected)

    print(f"QCGA3D exact oracle: ok ({len(KATS)} KATs, metric R(9,6), sparse incidence)")


if __name__ == "__main__":
    check()
